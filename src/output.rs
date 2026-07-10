use std::fmt::Write as _;

use anyhow::Context;
use clap::ValueEnum;

use crate::{domain::ReviewDocument, source::SourceBuffer};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Comments,
    Full,
    Json,
}

/// Formats a completed review for stdout or a file.
///
/// # Errors
///
/// Returns an error when JSON serialization fails.
pub fn format_review(
    format: OutputFormat,
    source: &SourceBuffer,
    review: &ReviewDocument,
) -> anyhow::Result<String> {
    match format {
        OutputFormat::Comments => Ok(format_comments(source, review)),
        OutputFormat::Full => Ok(format_full(source, review)),
        OutputFormat::Json => serde_json::to_string_pretty(review).context("serialize review JSON"),
    }
}

#[must_use]
pub fn format_comments(source: &SourceBuffer, review: &ReviewDocument) -> String {
    review
        .comments
        .iter()
        .map(|comment| {
            let quote = (comment.start_line..=comment.end_line)
                .filter_map(|line| source.line(line))
                .map(quote_line)
                .collect::<Vec<_>>()
                .join("\n");
            format!("{quote}\n\n{}", comment.body)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[must_use]
pub fn format_full(source: &SourceBuffer, review: &ReviewDocument) -> String {
    let mut blocks = Vec::new();
    let mut quote = String::new();

    for line_number in 1..=source.line_count() {
        if !quote.is_empty() {
            quote.push('\n');
        }
        let _ = write!(
            quote,
            "{}",
            quote_line(source.line(line_number).unwrap_or_default())
        );

        let comments = review
            .comments
            .iter()
            .filter(|comment| comment.end_line == line_number)
            .map(|comment| comment.body.clone())
            .collect::<Vec<_>>();
        if !comments.is_empty() {
            blocks.push(std::mem::take(&mut quote));
            blocks.extend(comments);
        }
    }

    if !quote.is_empty() {
        blocks.push(quote);
    }
    blocks.join("\n\n")
}

fn quote_line(line: &str) -> String {
    format!("> {line}")
}

#[cfg(test)]
mod tests {
    use crate::domain::{Comment, ReviewDocument};

    use super::*;

    fn fixture() -> (SourceBuffer, ReviewDocument) {
        let source = SourceBuffer::from_bytes("sample", b"alpha\nbeta\ngamma\ndelta\n").unwrap();
        let mut review = ReviewDocument::empty(source.source_ref());
        review.upsert_comment(Comment {
            id: 1,
            start_line: 2,
            end_line: 3,
            body: "human comment here ...".into(),
        });
        (source, review)
    }

    #[test]
    fn comments_mode_only_quotes_the_anchored_range() {
        let (source, review) = fixture();
        assert_eq!(
            format_comments(&source, &review),
            "> beta\n> gamma\n\nhuman comment here ..."
        );
    }

    #[test]
    fn full_mode_quotes_every_line_and_inserts_comments_inline() {
        let (source, review) = fixture();
        assert_eq!(
            format_full(&source, &review),
            "> alpha\n> beta\n> gamma\n\nhuman comment here ...\n\n> delta"
        );
    }

    #[test]
    fn comments_mode_is_empty_without_comments() {
        let source = SourceBuffer::from_bytes("sample", b"alpha\n").unwrap();
        let review = ReviewDocument::empty(source.source_ref());
        assert!(format_comments(&source, &review).is_empty());
    }

    #[test]
    fn dispatcher_serializes_json_and_multiple_comment_blocks() {
        let (source, mut review) = fixture();
        review.upsert_comment(Comment {
            id: 2,
            start_line: 4,
            end_line: 4,
            body: "second".into(),
        });
        let comments = format_review(OutputFormat::Comments, &source, &review).unwrap();
        assert!(comments.contains("human comment here ...\n\n> delta\n\nsecond"));

        let json = format_review(OutputFormat::Json, &source, &review).unwrap();
        let decoded: ReviewDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, review);
        assert_eq!(
            format_review(OutputFormat::Full, &source, &review).unwrap(),
            format_full(&source, &review)
        );
    }

    #[test]
    fn markdown_output_preserves_comment_indentation() {
        let source = SourceBuffer::from_bytes("sample", b"alpha\n").unwrap();
        let mut review = ReviewDocument::empty(source.source_ref());
        review.upsert_comment(Comment {
            id: 1,
            start_line: 1,
            end_line: 1,
            body: "    code()  ".into(),
        });
        assert_eq!(format_comments(&source, &review), "> alpha\n\n    code()  ");
        assert_eq!(format_full(&source, &review), "> alpha\n\n    code()  ");
    }
}
