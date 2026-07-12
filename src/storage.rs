use std::{
    collections::VecDeque,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::domain::ReviewDocument;

/// Loads a review sidecar from JSON.
///
/// # Errors
///
/// Returns an error when the file cannot be read or parsed.
pub fn load_review(path: &Path) -> Result<ReviewDocument> {
    let bytes = fs::read(path).with_context(|| format!("read comments from {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("parse comments from {}", path.display()))
}

/// Atomically saves a review sidecar next to its destination.
///
/// # Errors
///
/// Returns an error when serialization or any filesystem operation fails.
pub fn save_review(path: &Path, review: &ReviewDocument) -> Result<()> {
    let destination = resolve_destination(path)?;
    let parent = destination.parent().unwrap_or_else(|| Path::new("/"));
    fs::create_dir_all(parent)
        .with_context(|| format!("create comments directory {}", parent.display()))?;
    let bytes = serde_json::to_vec_pretty(review).context("serialize comments")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("create temporary comments file in {}", parent.display()))?;
    match fs::metadata(&destination) {
        Ok(metadata) => temporary
            .as_file()
            .set_permissions(metadata.permissions())
            .with_context(|| format!("preserve permissions from {}", destination.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("read permissions from {}", destination.display()));
        }
    }
    temporary
        .write_all(&bytes)
        .with_context(|| format!("write temporary comments file for {}", path.display()))?;
    temporary
        .write_all(b"\n")
        .with_context(|| format!("finish temporary comments file for {}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("sync temporary comments file for {}", path.display()))?;
    temporary
        .persist(&destination)
        .map_err(|error| error.error)
        .with_context(|| format!("replace comments file {}", destination.display()))?;
    Ok(())
}

pub(crate) fn resolve_destination(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .context("resolve current directory")?
            .join(path)
    };
    let mut pending = owned_components(&absolute);
    let mut resolved = PathBuf::new();
    let mut followed_links = 0;
    while let Some(component) = pending.pop_front() {
        match component {
            OwnedComponent::Root => {
                resolved.clear();
                resolved.push(Path::new("/"));
            }
            OwnedComponent::Prefix(prefix) => {
                resolved.clear();
                resolved.push(prefix);
            }
            OwnedComponent::Parent => {
                resolved.pop();
            }
            OwnedComponent::Normal(name) => {
                let candidate = resolved.join(&name);
                match fs::symlink_metadata(&candidate) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        followed_links += 1;
                        if followed_links > 40 {
                            anyhow::bail!(
                                "too many symbolic links while resolving {}",
                                path.display()
                            );
                        }
                        let target = fs::read_link(&candidate)
                            .with_context(|| format!("resolve destination {}", path.display()))?;
                        for target_component in owned_components(&target).into_iter().rev() {
                            pending.push_front(target_component);
                        }
                    }
                    Ok(_) => resolved.push(name),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => resolved.push(name),
                    Err(error) => {
                        return Err(error)
                            .with_context(|| format!("resolve destination {}", path.display()));
                    }
                }
            }
        }
    }
    Ok(resolved)
}

#[derive(Debug)]
enum OwnedComponent {
    Prefix(OsString),
    Root,
    Parent,
    Normal(OsString),
}

fn owned_components(path: &Path) -> VecDeque<OwnedComponent> {
    path.components()
        .filter_map(|component| match component {
            Component::Prefix(prefix) => {
                Some(OwnedComponent::Prefix(prefix.as_os_str().to_owned()))
            }
            Component::RootDir => Some(OwnedComponent::Root),
            Component::CurDir => None,
            Component::ParentDir => Some(OwnedComponent::Parent),
            Component::Normal(name) => Some(OwnedComponent::Normal(name.to_owned())),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::{domain::ReviewDocument, source::SourceBuffer};

    use super::*;

    #[test]
    fn review_round_trips_through_an_atomic_sidecar() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("review.json");
        let source = SourceBuffer::from_bytes("sample", b"hello\n").unwrap();
        let review = ReviewDocument::empty(source.source_ref());

        save_review(&path, &review).unwrap();
        assert_eq!(load_review(&path).unwrap(), review);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn save_creates_parent_directories_and_load_reports_invalid_json() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("nested/review.json");
        let source = SourceBuffer::from_bytes("sample", b"hello\n").unwrap();
        let review = ReviewDocument::empty(source.source_ref());
        save_review(&path, &review).unwrap();
        assert_eq!(load_review(&path).unwrap(), review);

        std::fs::write(&path, b"not json").unwrap();
        let error = load_review(&path).unwrap_err().to_string();
        assert!(error.contains("parse comments"));
    }

    #[cfg(unix)]
    #[test]
    fn save_preserves_existing_sidecar_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().unwrap();
        let path = directory.path().join("review.json");
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let source = SourceBuffer::from_bytes("sample", b"hello\n").unwrap();
        save_review(&path, &ReviewDocument::empty(source.source_ref())).unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_through_a_symlink_updates_its_target_and_preserves_the_link() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let target = directory.path().join("review.json");
        let link = directory.path().join("review-link.json");
        std::fs::write(&target, "{}").unwrap();
        symlink(&target, &link).unwrap();
        let source = SourceBuffer::from_bytes("sample", b"hello\n").unwrap();
        let review = ReviewDocument::empty(source.source_ref());

        save_review(&link, &review).unwrap();

        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(load_review(&target).unwrap(), review);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_resolved_before_parent_components() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let root = directory.path().join("root");
        let other = directory.path().join("other");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(other.join("child")).unwrap();
        symlink(other.join("child"), root.join("link")).unwrap();

        let resolved = resolve_destination(&root.join("link/../review.json")).unwrap();
        let resolved_parent = resolved.parent().unwrap().canonicalize().unwrap();
        let expected_parent = other.canonicalize().unwrap();

        assert_eq!(
            resolved_parent.join("review.json"),
            expected_parent.join("review.json")
        );
    }
}
