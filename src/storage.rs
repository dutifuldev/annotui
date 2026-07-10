use std::{
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
    let mut unresolved = normalize_lexically(&absolute);
    for _ in 0..40 {
        let mut resolved = PathBuf::new();
        let components = unresolved.components().collect::<Vec<_>>();
        let mut followed = false;

        for (index, component) in components.iter().enumerate() {
            resolved.push(component.as_os_str());
            match fs::symlink_metadata(&resolved) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    let target = fs::read_link(&resolved)
                        .with_context(|| format!("resolve destination {}", path.display()))?;
                    let parent = resolved.parent().unwrap_or_else(|| Path::new("/"));
                    let mut next = if target.is_absolute() {
                        target
                    } else {
                        parent.join(target)
                    };
                    for remaining in &components[index + 1..] {
                        next.push(remaining.as_os_str());
                    }
                    unresolved = normalize_lexically(&next);
                    followed = true;
                    break;
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("resolve destination {}", path.display()));
                }
            }
        }
        if !followed {
            return Ok(normalize_lexically(&resolved));
        }
    }
    anyhow::bail!("too many symbolic links while resolving {}", path.display())
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
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
}
