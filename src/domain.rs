use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const REVIEW_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub name: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comment {
    pub id: u64,
    pub start_line: usize,
    pub end_line: usize,
    pub body: String,
}

impl Comment {
    #[must_use]
    pub fn contains_line(&self, line: usize) -> bool {
        self.start_line <= line && line <= self.end_line
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDocument {
    pub version: u32,
    pub source: SourceRef,
    pub comments: Vec<Comment>,
}

impl ReviewDocument {
    #[must_use]
    pub fn empty(source: SourceRef) -> Self {
        Self {
            version: REVIEW_FORMAT_VERSION,
            source,
            comments: Vec::new(),
        }
    }

    #[must_use]
    pub fn next_comment_id(&self) -> Option<u64> {
        let used = self
            .comments
            .iter()
            .map(|comment| comment.id)
            .collect::<std::collections::BTreeSet<_>>();
        let mut candidate = 1_u64;
        for id in used {
            if id > candidate {
                return Some(candidate);
            }
            candidate = candidate.checked_add(1)?;
        }
        Some(candidate)
    }

    pub fn upsert_comment(&mut self, comment: Comment) {
        if let Some(existing) = self
            .comments
            .iter_mut()
            .find(|existing| existing.id == comment.id)
        {
            *existing = comment;
        } else {
            self.comments.push(comment);
        }
        self.sort_comments();
    }

    pub fn remove_comment(&mut self, id: u64) -> bool {
        let before = self.comments.len();
        self.comments.retain(|comment| comment.id != id);
        self.comments.len() != before
    }

    pub fn sort_comments(&mut self) {
        self.comments
            .sort_by_key(|comment| (comment.start_line, comment.end_line, comment.id));
    }

    /// Validates version, source metadata, IDs, line ranges, and comment bodies.
    ///
    /// # Errors
    ///
    /// Returns a [`ReviewValidationError`] when any document invariant is broken.
    pub fn validate(&self, line_count: usize) -> Result<(), ReviewValidationError> {
        if self.version != REVIEW_FORMAT_VERSION {
            return Err(ReviewValidationError::UnsupportedVersion(self.version));
        }
        if self.source.name.trim().is_empty() {
            return Err(ReviewValidationError::EmptySourceName);
        }
        if self.source.sha256.len() != 64
            || !self
                .source
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(ReviewValidationError::InvalidSourceHash);
        }

        let mut ids = std::collections::BTreeSet::new();
        for comment in &self.comments {
            if comment.id == 0 || !ids.insert(comment.id) {
                return Err(ReviewValidationError::InvalidCommentId(comment.id));
            }
            if comment.start_line == 0
                || comment.end_line < comment.start_line
                || comment.end_line > line_count
            {
                return Err(ReviewValidationError::InvalidRange {
                    id: comment.id,
                    start: comment.start_line,
                    end: comment.end_line,
                    line_count,
                });
            }
            if comment.body.trim().is_empty() {
                return Err(ReviewValidationError::EmptyBody(comment.id));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReviewValidationError {
    #[error("unsupported review format version {0}")]
    UnsupportedVersion(u32),
    #[error("source name must not be empty")]
    EmptySourceName,
    #[error("source sha256 must contain 64 lowercase hexadecimal characters")]
    InvalidSourceHash,
    #[error("comment ID {0} must be positive and unique")]
    InvalidCommentId(u64),
    #[error("comment {id} has invalid range {start}-{end}; source has {line_count} lines")]
    InvalidRange {
        id: u64,
        start: usize,
        end: usize,
        line_count: usize,
    },
    #[error("comment {0} has an empty body")]
    EmptyBody(u64),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> SourceRef {
        SourceRef {
            name: "input.txt".into(),
            sha256: "a".repeat(64),
        }
    }

    #[test]
    fn next_id_and_upsert_are_deterministic() {
        let mut review = ReviewDocument::empty(source());
        review.upsert_comment(Comment {
            id: 4,
            start_line: 2,
            end_line: 2,
            body: "later".into(),
        });
        review.upsert_comment(Comment {
            id: 1,
            start_line: 1,
            end_line: 1,
            body: "first".into(),
        });
        review.upsert_comment(Comment {
            id: 4,
            start_line: 2,
            end_line: 3,
            body: "updated".into(),
        });

        assert_eq!(review.next_comment_id(), Some(2));
        assert_eq!(
            review.comments.iter().map(|c| c.id).collect::<Vec<_>>(),
            [1, 4]
        );
        assert_eq!(review.comments[1].body, "updated");
    }

    #[test]
    fn validation_rejects_bad_ranges_and_bodies() {
        let mut review = ReviewDocument::empty(source());
        review.comments.push(Comment {
            id: 1,
            start_line: 2,
            end_line: 4,
            body: "comment".into(),
        });
        assert!(matches!(
            review.validate(3),
            Err(ReviewValidationError::InvalidRange { .. })
        ));

        review.comments[0].end_line = 3;
        review.comments[0].body = "  ".into();
        assert_eq!(review.validate(3), Err(ReviewValidationError::EmptyBody(1)));
    }

    #[test]
    fn validation_checks_document_metadata_and_unique_ids() {
        let mut review = ReviewDocument::empty(source());
        assert!(review.validate(1).is_ok());
        review.version = 2;
        assert_eq!(
            review.validate(1),
            Err(ReviewValidationError::UnsupportedVersion(2))
        );
        review.version = REVIEW_FORMAT_VERSION;
        review.source.name = " ".into();
        assert_eq!(
            review.validate(1),
            Err(ReviewValidationError::EmptySourceName)
        );
        review.source.name = "input".into();
        review.source.sha256 = "ABC".into();
        assert_eq!(
            review.validate(1),
            Err(ReviewValidationError::InvalidSourceHash)
        );
        review.source.sha256 = "A".repeat(64);
        assert_eq!(
            review.validate(1),
            Err(ReviewValidationError::InvalidSourceHash)
        );
        review.source.sha256 = "b".repeat(63);
        assert_eq!(
            review.validate(1),
            Err(ReviewValidationError::InvalidSourceHash)
        );

        review.source.sha256 = "b".repeat(64);
        review.comments = vec![
            Comment {
                id: 1,
                start_line: 1,
                end_line: 1,
                body: "one".into(),
            },
            Comment {
                id: 1,
                start_line: 1,
                end_line: 1,
                body: "two".into(),
            },
        ];
        assert_eq!(
            review.validate(1),
            Err(ReviewValidationError::InvalidCommentId(1))
        );
        assert!(review.remove_comment(1));
        assert!(!review.remove_comment(1));
    }

    #[test]
    fn line_containment_and_each_range_boundary_are_validated() {
        let comment = Comment {
            id: 1,
            start_line: 2,
            end_line: 3,
            body: "body".into(),
        };
        assert!(!comment.contains_line(1));
        assert!(comment.contains_line(2));
        assert!(comment.contains_line(3));
        assert!(!comment.contains_line(4));

        let mut review = ReviewDocument::empty(source());
        review.comments.push(comment);
        review.comments[0].start_line = 0;
        assert!(matches!(
            review.validate(3),
            Err(ReviewValidationError::InvalidRange { .. })
        ));
        review.comments[0].start_line = 3;
        review.comments[0].end_line = 2;
        assert!(matches!(
            review.validate(3),
            Err(ReviewValidationError::InvalidRange { .. })
        ));
        review.comments[0].start_line = 2;
        review.comments[0].end_line = 4;
        assert!(matches!(
            review.validate(3),
            Err(ReviewValidationError::InvalidRange { .. })
        ));
    }

    #[test]
    fn id_allocation_fills_gaps_including_below_the_maximum() {
        let mut review = ReviewDocument::empty(source());
        review.comments = vec![
            Comment {
                id: 2,
                start_line: 1,
                end_line: 1,
                body: "two".into(),
            },
            Comment {
                id: u64::MAX,
                start_line: 1,
                end_line: 1,
                body: "maximum".into(),
            },
        ];
        assert_eq!(review.next_comment_id(), Some(1));
        review.comments.push(Comment {
            id: 1,
            start_line: 1,
            end_line: 1,
            body: "one".into(),
        });
        assert_eq!(review.next_comment_id(), Some(3));
    }
}
