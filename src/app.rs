use ratatui_textarea::{CursorMove, TextArea};

use crate::{
    domain::{Comment, ReviewDocument},
    source::SourceBuffer,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub current: usize,
}

impl Selection {
    #[must_use]
    pub fn normalized(self) -> (usize, usize) {
        if self.anchor <= self.current {
            (self.anchor, self.current)
        } else {
            (self.current, self.anchor)
        }
    }

    #[must_use]
    pub fn contains(self, line: usize) -> bool {
        let (start, end) = self.normalized();
        start <= line && line <= end
    }
}

#[derive(Debug)]
pub struct CommentEditor {
    pub comment_id: Option<u64>,
    pub start_line: usize,
    pub end_line: usize,
    pub textarea: TextArea<'static>,
}

impl CommentEditor {
    fn new(comment_id: Option<u64>, start_line: usize, end_line: usize, body: &str) -> Self {
        let mut textarea = if body.is_empty() {
            TextArea::default()
        } else {
            TextArea::from(body.split('\n'))
        };
        textarea.set_placeholder_text("Write a comment…");
        textarea.set_cursor_line_style(ratatui::style::Style::default());
        textarea.move_cursor(CursorMove::Bottom);
        textarea.move_cursor(CursorMove::End);
        Self {
            comment_id,
            start_line,
            end_line,
            textarea,
        }
    }

    pub fn body(&self) -> String {
        self.textarea.lines().join("\n").trim().to_owned()
    }
}

#[derive(Debug)]
pub struct App {
    pub source: SourceBuffer,
    pub review: ReviewDocument,
    pub cursor_line: usize,
    pub selection: Option<Selection>,
    pub editor: Option<CommentEditor>,
    pub active_comment_id: Option<u64>,
    pub scroll_row: usize,
    pub horizontal_scroll: usize,
    pub should_quit: bool,
    pub follow_cursor: bool,
    pub status: Option<String>,
    pub dirty: bool,
}

impl App {
    #[must_use]
    pub fn new(source: SourceBuffer, review: ReviewDocument) -> Self {
        Self {
            source,
            review,
            cursor_line: 1,
            selection: None,
            editor: None,
            active_comment_id: None,
            scroll_row: 0,
            horizontal_scroll: 0,
            should_quit: false,
            follow_cursor: true,
            status: None,
            dirty: false,
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let maximum = self.source.line_count();
        self.cursor_line = self
            .cursor_line
            .saturating_add_signed(delta)
            .clamp(1, maximum);
        if let Some(selection) = &mut self.selection {
            selection.current = self.cursor_line;
        }
        self.follow_cursor = true;
        self.active_comment_id = None;
        self.status = None;
    }

    pub fn move_to_line(&mut self, line: usize) {
        self.cursor_line = line.clamp(1, self.source.line_count());
        if let Some(selection) = &mut self.selection {
            selection.current = self.cursor_line;
        }
        self.follow_cursor = true;
        self.active_comment_id = None;
    }

    pub fn begin_selection(&mut self, line: usize) {
        self.move_to_line(line);
        self.selection = Some(Selection {
            anchor: self.cursor_line,
            current: self.cursor_line,
        });
        self.status = None;
    }

    pub fn extend_selection(&mut self, line: usize) {
        if self.selection.is_none() {
            self.begin_selection(line);
            return;
        }
        self.cursor_line = line.clamp(1, self.source.line_count());
        if let Some(selection) = &mut self.selection {
            selection.current = self.cursor_line;
        }
    }

    pub fn cancel_selection(&mut self) {
        self.selection = None;
        self.status = None;
    }

    pub fn open_selected_editor(&mut self) {
        let selection = self.selection.unwrap_or(Selection {
            anchor: self.cursor_line,
            current: self.cursor_line,
        });
        let (start_line, end_line) = selection.normalized();
        self.cursor_line = end_line;
        self.editor = Some(CommentEditor::new(None, start_line, end_line, ""));
        self.follow_cursor = true;
        self.status = None;
    }

    pub fn begin_edit(&mut self, comment_id: u64) -> bool {
        let Some(comment) = self
            .review
            .comments
            .iter()
            .find(|comment| comment.id == comment_id)
            .cloned()
        else {
            return false;
        };
        self.cursor_line = comment.end_line;
        self.active_comment_id = Some(comment.id);
        self.selection = Some(Selection {
            anchor: comment.start_line,
            current: comment.end_line,
        });
        self.editor = Some(CommentEditor::new(
            Some(comment.id),
            comment.start_line,
            comment.end_line,
            &comment.body,
        ));
        self.follow_cursor = true;
        self.status = None;
        true
    }

    pub fn edit_comment_at_cursor(&mut self) -> bool {
        let id = self.comment_id_at_cursor();
        id.is_some_and(|id| self.begin_edit(id))
    }

    pub fn submit_editor(&mut self) -> bool {
        let Some(editor) = &self.editor else {
            return false;
        };
        let body = editor.body();
        if body.is_empty() {
            self.status = Some("Comment cannot be empty".into());
            return false;
        }
        let Some(id) = editor.comment_id.or_else(|| self.review.next_comment_id()) else {
            self.status = Some("No comment IDs available".into());
            return false;
        };
        self.review.upsert_comment(Comment {
            id,
            start_line: editor.start_line,
            end_line: editor.end_line,
            body,
        });
        self.editor = None;
        self.selection = None;
        self.status = Some("Comment saved".into());
        self.active_comment_id = Some(id);
        self.dirty = true;
        true
    }

    pub fn cancel_editor(&mut self) {
        self.editor = None;
        self.selection = None;
        self.status = Some("Edit cancelled".into());
    }

    pub fn delete_comment_at_cursor(&mut self) -> bool {
        let id = self.comment_id_at_cursor();
        let removed = id.is_some_and(|id| self.review.remove_comment(id));
        if removed {
            self.status = Some("Comment deleted".into());
            self.active_comment_id = None;
            self.dirty = true;
        } else {
            self.status = Some("No comment on this line".into());
        }
        removed
    }

    pub fn jump_comment(&mut self, forward: bool) {
        if self.review.comments.is_empty() {
            self.active_comment_id = None;
            return;
        }
        let active_index = self.active_comment_id.and_then(|id| {
            self.review
                .comments
                .iter()
                .position(|comment| comment.id == id)
        });
        let target_index = if let Some(index) = active_index {
            if forward {
                (index + 1) % self.review.comments.len()
            } else {
                index
                    .checked_sub(1)
                    .unwrap_or(self.review.comments.len() - 1)
            }
        } else if forward {
            self.review
                .comments
                .iter()
                .position(|comment| comment.start_line > self.cursor_line)
                .unwrap_or(0)
        } else {
            self.review
                .comments
                .iter()
                .rposition(|comment| comment.end_line < self.cursor_line)
                .unwrap_or(self.review.comments.len() - 1)
        };
        let comment = &self.review.comments[target_index];
        self.cursor_line = comment.start_line;
        self.active_comment_id = Some(comment.id);
        self.follow_cursor = true;
        self.status = Some(format!(
            "Comment {}/{} · e edit · d delete",
            target_index + 1,
            self.review.comments.len()
        ));
    }

    fn comment_id_at_cursor(&self) -> Option<u64> {
        self.active_comment_id
            .filter(|id| {
                self.review
                    .comments
                    .iter()
                    .any(|comment| comment.id == *id && comment.contains_line(self.cursor_line))
            })
            .or_else(|| {
                self.review
                    .comments
                    .iter()
                    .find(|comment| comment.contains_line(self.cursor_line))
                    .map(|comment| comment.id)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let source = SourceBuffer::from_bytes("sample", b"one\ntwo\nthree\n").unwrap();
        let review = ReviewDocument::empty(source.source_ref());
        App::new(source, review)
    }

    #[test]
    fn reverse_selection_normalizes_before_editing() {
        let mut app = app();
        app.begin_selection(3);
        app.extend_selection(1);
        let selection = app.selection.unwrap();
        assert!(selection.contains(1));
        assert!(selection.contains(2));
        assert!(selection.contains(3));
        assert!(!selection.contains(4));
        app.open_selected_editor();
        assert_eq!(app.cursor_line, 3);
        let editor = app.editor.unwrap();
        assert_eq!((editor.start_line, editor.end_line), (1, 3));
    }

    #[test]
    fn comment_can_be_added_reopened_and_deleted() {
        let mut app = app();
        app.begin_selection(2);
        app.open_selected_editor();
        app.editor.as_mut().unwrap().textarea.insert_str("hello");
        assert!(app.submit_editor());
        assert_eq!(app.review.comments[0].body, "hello");

        app.cursor_line = 2;
        assert!(app.edit_comment_at_cursor());
        app.editor.as_mut().unwrap().textarea.insert_str(" world");
        assert!(app.submit_editor());
        assert_eq!(app.review.comments[0].body, "hello world");

        app.cursor_line = 2;
        assert!(app.delete_comment_at_cursor());
        assert!(app.review.comments.is_empty());
    }

    #[test]
    fn empty_comment_stays_open() {
        let mut app = app();
        app.open_selected_editor();
        assert!(!app.submit_editor());
        assert!(app.editor.is_some());
        assert_eq!(app.status.as_deref(), Some("Comment cannot be empty"));
    }

    #[test]
    fn cursor_selection_and_cancellation_stay_within_source() {
        let mut app = app();
        app.move_cursor(-10);
        assert_eq!(app.cursor_line, 1);
        app.begin_selection(2);
        app.move_cursor(10);
        assert_eq!(app.cursor_line, 3);
        assert_eq!(app.selection.unwrap().normalized(), (2, 3));
        app.cancel_selection();
        assert!(app.selection.is_none());
        assert!(app.status.is_none());
    }

    #[test]
    fn missing_comment_actions_report_without_mutating() {
        let mut app = app();
        assert!(!app.begin_edit(42));
        assert!(!app.edit_comment_at_cursor());
        assert!(!app.delete_comment_at_cursor());
        assert_eq!(app.status.as_deref(), Some("No comment on this line"));
        assert!(!app.dirty);
    }

    #[test]
    fn comment_jumps_wrap_in_both_directions() {
        let mut app = app();
        for (id, line) in [(1, 1), (2, 3)] {
            app.review.upsert_comment(Comment {
                id,
                start_line: line,
                end_line: line,
                body: format!("comment {id}"),
            });
        }
        app.cursor_line = 1;
        app.jump_comment(true);
        assert_eq!(app.cursor_line, 3);
        app.jump_comment(true);
        assert_eq!(app.cursor_line, 1);
        app.jump_comment(false);
        assert_eq!(app.cursor_line, 3);
        app.jump_comment(false);
        assert_eq!(app.cursor_line, 1);
    }

    #[test]
    fn cancelled_editor_discards_draft() {
        let mut app = app();
        app.open_selected_editor();
        app.editor.as_mut().unwrap().textarea.insert_str("draft");
        app.cancel_editor();
        assert!(app.editor.is_none());
        assert!(app.review.comments.is_empty());
        assert_eq!(app.status.as_deref(), Some("Edit cancelled"));
    }

    #[test]
    fn co_located_comments_can_be_selected_edited_and_deleted_individually() {
        let mut app = app();
        for id in [1, 2] {
            app.review.upsert_comment(Comment {
                id,
                start_line: 2,
                end_line: 2,
                body: format!("comment {id}"),
            });
        }
        app.cursor_line = 1;
        app.jump_comment(true);
        assert_eq!(app.active_comment_id, Some(1));
        app.jump_comment(true);
        assert_eq!(app.active_comment_id, Some(2));
        assert!(app.edit_comment_at_cursor());
        assert_eq!(app.editor.as_ref().unwrap().comment_id, Some(2));
        app.cancel_editor();
        assert!(app.delete_comment_at_cursor());
        assert_eq!(app.review.comments.len(), 1);
        assert_eq!(app.review.comments[0].id, 1);
    }

    #[test]
    fn inactive_backward_jumps_choose_previous_or_wrap() {
        let mut app = app();
        for (id, line) in [(1, 1), (2, 3)] {
            app.review.upsert_comment(Comment {
                id,
                start_line: line,
                end_line: line,
                body: format!("comment {id}"),
            });
        }
        app.cursor_line = 3;
        app.jump_comment(false);
        assert_eq!(app.active_comment_id, Some(1));
        app.active_comment_id = None;
        app.cursor_line = 1;
        app.jump_comment(false);
        assert_eq!(app.active_comment_id, Some(2));
    }

    #[test]
    fn stale_active_comment_falls_back_to_a_comment_on_the_cursor() {
        let mut app = app();
        for (id, line) in [(1, 1), (2, 2)] {
            app.review.upsert_comment(Comment {
                id,
                start_line: line,
                end_line: line,
                body: format!("comment {id}"),
            });
        }
        app.active_comment_id = Some(2);
        app.cursor_line = 1;
        assert!(app.edit_comment_at_cursor());
        assert_eq!(app.editor.as_ref().unwrap().comment_id, Some(1));
    }
}
