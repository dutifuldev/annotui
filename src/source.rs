use std::path::Path;

use sha2::{Digest, Sha256};

use crate::domain::SourceRef;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBuffer {
    name: String,
    sha256: String,
    lines: Vec<String>,
}

impl SourceBuffer {
    /// Builds a source buffer from exact UTF-8 input bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the input is not valid UTF-8.
    pub fn from_bytes(name: impl Into<String>, bytes: &[u8]) -> Result<Self, std::str::Utf8Error> {
        let text = std::str::from_utf8(bytes)?;
        let mut lines = text
            .split_terminator('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
            .collect::<Vec<_>>();
        if lines.is_empty() {
            lines.push(String::new());
        }

        Ok(Self {
            name: name.into(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            lines,
        })
    }

    /// Builds a source buffer whose display name is the given path.
    ///
    /// # Errors
    ///
    /// Returns an error when the input is not valid UTF-8.
    pub fn from_path(path: &Path, bytes: &[u8]) -> Result<Self, std::str::Utf8Error> {
        Self::from_bytes(path.display().to_string(), bytes)
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    #[must_use]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, one_based: usize) -> Option<&str> {
        one_based
            .checked_sub(1)
            .and_then(|index| self.lines.get(index))
            .map(String::as_str)
    }

    #[must_use]
    pub fn source_ref(&self) -> SourceRef {
        SourceRef {
            name: self.name.clone(),
            sha256: self.sha256.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_unix_and_windows_lines_without_an_artificial_trailing_line() {
        let source = SourceBuffer::from_bytes("input", b"one\r\ntwo\n").unwrap();
        assert_eq!(source.lines(), ["one", "two"]);
        assert_eq!(source.line(2), Some("two"));
        assert_eq!(source.line(0), None);
    }

    #[test]
    fn empty_input_has_one_selectable_line() {
        let source = SourceBuffer::from_bytes("empty", b"").unwrap();
        assert_eq!(source.lines(), [""]);
        assert_eq!(source.sha256().len(), 64);
    }

    #[test]
    fn path_names_and_invalid_utf8_are_handled_explicitly() {
        let source = SourceBuffer::from_path(Path::new("folder/file.txt"), b"hello").unwrap();
        assert_eq!(source.name(), "folder/file.txt");
        assert_eq!(source.line_count(), 1);
        assert!(SourceBuffer::from_bytes("bad", &[0xff]).is_err());
    }
}
