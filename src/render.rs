use std::collections::BTreeMap;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    app::{App, WordWrap},
    input::{HitArea, HitTarget},
};

const EDITOR_HEIGHT: usize = 5;
const MINIMUM_HEIGHT: u16 = 7;
const TAB_STOP: usize = 4;
// fast-agent declares Black on Gray for prompt_toolkit's bottom toolbar, whose
// built-in `reverse` style renders the effective terminal colors as Gray on Black.
const FOOTER_FOREGROUND: Color = Color::Gray;
const FOOTER_BACKGROUND: Color = Color::Black;

#[derive(Debug, Clone, PartialEq, Eq)]
enum DocumentRow {
    Source {
        line_number: usize,
        text: String,
        first: bool,
        commented: bool,
        active: bool,
    },
    Comment {
        id: u64,
        text: String,
        first: bool,
    },
    Editor,
}

pub fn render_app(frame: &mut Frame<'_>, app: &mut App) -> Vec<HitArea> {
    let area = frame.area();
    if area.height < MINIMUM_HEIGHT || area.width < 20 {
        frame.render_widget(
            Paragraph::new(format!(
                "annotui needs at least 20 columns and {MINIMUM_HEIGHT} rows"
            ))
            .style(Style::default().fg(Color::Red)),
            area,
        );
        return Vec::new();
    }

    let header = Rect::new(area.x, area.y, area.width, 1);
    let content = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(2),
    );
    let footer = Rect::new(
        area.x,
        area.y.saturating_add(area.height.saturating_sub(1)),
        area.width,
        1,
    );

    render_header(frame, app, header);
    let hits = render_document(frame, app, content);
    render_footer(frame, app, footer);
    hits
}

fn render_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let title = format!(
        " annotui · {} · {} lines · {} comments ",
        app.source.name(),
        app.source.line_count(),
        app.review.comments.len()
    );
    frame.render_widget(
        Paragraph::new(title).style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        area,
    );
}

fn render_document(frame: &mut Frame<'_>, app: &mut App, area: Rect) -> Vec<HitArea> {
    let line_digits = app.source.line_count().to_string().len();
    let text_width = match app.word_wrap {
        WordWrap::On => source_text_width(area.width, line_digits),
        WordWrap::Off => usize::MAX,
    };
    let rows = document_rows(app, text_width);
    update_scroll(app, &rows, usize::from(area.height));
    let visible = rows
        .iter()
        .enumerate()
        .skip(app.scroll_row)
        .take(usize::from(area.height));
    let mut hits = Vec::new();
    let mut editor_rect: Option<Rect> = None;

    for (screen_offset, (_, row)) in visible.enumerate() {
        let rect = Rect::new(
            area.x,
            area.y
                .saturating_add(u16::try_from(screen_offset).unwrap_or(u16::MAX)),
            area.width,
            1,
        );
        match row {
            DocumentRow::Source { line_number, .. } => {
                render_source_row(frame, app, rect, row, line_digits);
                hits.push(HitArea::new(rect, HitTarget::SourceLine(*line_number)));
            }
            DocumentRow::Comment { id, text, first } => {
                render_comment_row(
                    frame,
                    rect,
                    text,
                    *first,
                    line_digits,
                    app.active_comment_id == Some(*id),
                );
                hits.push(HitArea::new(rect, HitTarget::Comment(*id)));
            }
            DocumentRow::Editor => extend_editor_rect(&mut editor_rect, rect),
        }
    }

    if let (Some(rect), Some(editor)) = (editor_rect, app.editor.as_mut()) {
        let range = if editor.start_line == editor.end_line {
            format!(" Comment on line {} ", editor.start_line)
        } else {
            format!(
                " Comment on lines {}–{} ",
                editor.start_line, editor.end_line
            )
        };
        editor.textarea.set_block(
            Block::default()
                .title(range)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        );
        frame.render_widget(&editor.textarea, rect);
    }
    hits
}

fn document_rows(app: &App, text_width: usize) -> Vec<DocumentRow> {
    let mut rows = Vec::new();
    let editing_id = app.editor.as_ref().and_then(|editor| editor.comment_id);
    let editor_end = app.editor.as_ref().map(|editor| editor.end_line);
    let active_range = app.active_comment_id.and_then(|id| {
        app.review
            .comments
            .iter()
            .find(|comment| comment.id == id)
            .map(|comment| (comment.start_line, comment.end_line))
    });
    let mut comments_by_end = BTreeMap::<usize, Vec<_>>::new();
    let mut range_events = BTreeMap::<usize, (usize, usize)>::new();
    for comment in &app.review.comments {
        range_events.entry(comment.start_line).or_default().0 += 1;
        if comment.end_line < app.source.line_count() {
            range_events.entry(comment.end_line + 1).or_default().1 += 1;
        }
    }
    for comment in app
        .review
        .comments
        .iter()
        .filter(|comment| Some(comment.id) != editing_id)
    {
        comments_by_end
            .entry(comment.end_line)
            .or_default()
            .push(comment);
    }

    let mut events = range_events.into_iter().peekable();
    let mut comments_covering_line = 0usize;
    for line_number in 1..=app.source.line_count() {
        if events
            .peek()
            .is_some_and(|(event_line, _)| *event_line == line_number)
        {
            let (_, (starts, ends)) = events.next().unwrap_or_default();
            comments_covering_line = comments_covering_line.saturating_sub(ends);
            comments_covering_line = comments_covering_line.saturating_add(starts);
        }
        let commented = comments_covering_line > 0;
        let active =
            active_range.is_some_and(|(start, end)| start <= line_number && line_number <= end);
        for (index, text) in
            wrapped_line(app.source.line(line_number).unwrap_or_default(), text_width)
                .into_iter()
                .enumerate()
        {
            rows.push(DocumentRow::Source {
                line_number,
                text,
                first: index == 0,
                commented,
                active,
            });
        }
        if let Some(comments) = comments_by_end.get(&line_number) {
            for comment in comments {
                for (index, body_line) in comment.body.lines().enumerate() {
                    rows.push(DocumentRow::Comment {
                        id: comment.id,
                        text: body_line.to_owned(),
                        first: index == 0,
                    });
                }
            }
        }
        if editor_end == Some(line_number) {
            rows.extend(std::iter::repeat_n(DocumentRow::Editor, EDITOR_HEIGHT));
        }
    }
    rows
}

fn update_scroll(app: &mut App, rows: &[DocumentRow], viewport_height: usize) {
    let maximum = rows.len().saturating_sub(viewport_height);
    app.scroll_row = app.scroll_row.min(maximum);
    if !app.follow_cursor || viewport_height == 0 {
        return;
    }

    let source_row = rows
        .iter()
        .position(|row| {
            matches!(
                row,
                DocumentRow::Source { line_number, .. } if *line_number == app.cursor_line
            )
        })
        .unwrap_or(0);
    let focused_comment_row = app.active_comment_id.and_then(|id| {
        rows.iter().position(|row| {
            matches!(
                row,
                DocumentRow::Comment {
                    id: row_id,
                    first: true,
                    ..
                } if *row_id == id
            )
        })
    });
    if let Some(focus_row) = focused_comment_row {
        if focus_row < app.scroll_row {
            app.scroll_row = focus_row;
        } else if focus_row >= app.scroll_row.saturating_add(viewport_height) {
            app.scroll_row = focus_row.saturating_add(1).saturating_sub(viewport_height);
        }
        app.scroll_row = app.scroll_row.min(maximum);
        app.follow_cursor = false;
        return;
    }
    let target_row = if app.editor.is_some() {
        rows.iter()
            .enumerate()
            .skip(source_row)
            .take_while(|(_, row)| {
                !matches!(
                    row,
                    DocumentRow::Source { line_number, .. }
                        if *line_number > app.cursor_line
                )
            })
            .map(|(index, _)| index)
            .last()
            .unwrap_or(source_row)
    } else {
        source_row
    };

    if source_row < app.scroll_row {
        app.scroll_row = source_row;
    } else if target_row >= app.scroll_row.saturating_add(viewport_height) {
        app.scroll_row = target_row.saturating_add(1).saturating_sub(viewport_height);
    }
    app.scroll_row = app.scroll_row.min(maximum);
    app.follow_cursor = false;
}

fn render_source_row(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    row: &DocumentRow,
    line_digits: usize,
) {
    let DocumentRow::Source {
        line_number,
        text,
        first,
        commented,
        active,
    } = row
    else {
        return;
    };
    let selected = app
        .selection
        .is_some_and(|selection| selection.contains(*line_number));
    let cursor = *first
        && app.cursor_line == *line_number
        && app.editor.is_none()
        && app.active_comment_id.is_none();
    let marker = if cursor { "▶" } else { " " };
    let number = if *first {
        format!("{marker}{line_number:>line_digits$} ")
    } else {
        format!(" {continuation:>line_digits$} ", continuation = "·")
    };
    let style = if selected {
        Style::default().fg(Color::White).bg(Color::DarkGray)
    } else {
        Style::default()
    };
    let number_style = if selected {
        Style::default().fg(Color::Gray).bg(Color::DarkGray)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let rail = if *commented { "┃" } else { "│" };
    let rail_style = if *active {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else if *commented {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        number_style
    };
    let line = Line::from(vec![
        Span::styled(number, number_style),
        Span::styled(rail, rail_style),
        Span::styled(" ", style),
        Span::styled(text.to_owned(), style),
    ]);
    frame.render_widget(Paragraph::new(line).style(style), area);
}

fn render_comment_row(
    frame: &mut Frame<'_>,
    area: Rect,
    text: &str,
    first: bool,
    line_digits: usize,
    active: bool,
) {
    let prefix = if first {
        let marker = if active { "▶─ " } else { "└─ " };
        format!("{}{marker}", " ".repeat(line_digits + 2))
    } else {
        " ".repeat(line_digits + 5)
    };
    let style = if active {
        Style::default().fg(Color::Black).bg(Color::Green)
    } else {
        Style::default().fg(Color::Green).bg(Color::Rgb(20, 35, 25))
    };
    frame.render_widget(
        Paragraph::new(format!("{prefix}{}", expand_tabs(text))).style(style),
        area,
    );
}

fn extend_editor_rect(editor_rect: &mut Option<Rect>, row: Rect) {
    if let Some(rect) = editor_rect {
        rect.height = rect.height.saturating_add(1);
    } else {
        *editor_rect = Some(row);
    }
}

fn source_text_width(area_width: u16, line_digits: usize) -> usize {
    usize::from(area_width)
        .saturating_sub(line_digits + 4)
        .max(1)
}

fn wrapped_line(text: &str, width: usize) -> Vec<String> {
    let expanded = expand_tabs(text);
    let graphemes = expanded.graphemes(true).collect::<Vec<_>>();
    if graphemes.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut start = 0;
    while start < graphemes.len() {
        let mut end = start;
        let mut columns = 0usize;
        let mut overflowed = false;
        while end < graphemes.len() {
            let grapheme_width = graphemes[end].width();
            if end > start && columns.saturating_add(grapheme_width) > width {
                overflowed = true;
                break;
            }
            columns = columns.saturating_add(grapheme_width);
            end += 1;
            if columns >= width {
                break;
            }
        }

        if overflowed {
            if let Some(break_index) = (start + 1..end)
                .rev()
                .find(|index| graphemes[*index].chars().all(char::is_whitespace))
            {
                end = break_index;
            }
        }

        lines.push(graphemes[start..end].concat().trim_end().to_owned());
        start = end;
        while start < graphemes.len() && graphemes[start].chars().all(char::is_whitespace) {
            start += 1;
        }
    }
    lines
}

fn expand_tabs(text: &str) -> String {
    let mut expanded = String::with_capacity(text.len());
    let mut column = 0;
    for grapheme in text.graphemes(true) {
        if grapheme == "\t" {
            let spaces = TAB_STOP - column % TAB_STOP;
            expanded.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            expanded.push_str(grapheme);
            column += grapheme.width();
        }
    }
    expanded
}

fn render_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let help = if app.editor.is_some() {
        " Enter save · Esc cancel "
    } else if let Some(selection) = app.selection {
        let (start, end) = selection.normalized();
        let finish = if app.keyboard_shift_anchor.is_some() {
            "release Shift or Enter"
        } else {
            "Enter"
        };
        return render_footer_text(
            frame,
            area,
            &format!(" Lines {start}–{end} selected · {finish} to comment · Esc cancel "),
        );
    } else {
        " drag or Shift-↑/↓ select · Enter comment · e edit · d delete · Alt-Z wrap · q quit "
    };
    let text = app.status.as_deref().unwrap_or(help);
    render_footer_text(frame, area, text);
}

fn render_footer_text(frame: &mut Frame<'_>, area: Rect, text: &str) {
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(FOOTER_FOREGROUND).bg(FOOTER_BACKGROUND)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use crate::{domain::ReviewDocument, source::SourceBuffer};

    use super::*;

    #[test]
    fn renders_source_selection_and_mouse_targets() {
        let source = SourceBuffer::from_bytes("sample", b"one\ntwo\nthree\n").unwrap();
        let review = ReviewDocument::empty(source.source_ref());
        let mut app = App::new(source, review);
        app.begin_selection(2);
        app.extend_selection(3);
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut hits = Vec::new();

        terminal
            .draw(|frame| hits = render_app(frame, &mut app))
            .unwrap();

        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("annotui · sample · 3 lines"));
        assert!(rendered.contains("two"));
        assert!(hits
            .iter()
            .any(|hit| hit.target == HitTarget::SourceLine(3)));
    }

    #[test]
    fn footer_matches_fast_agent_effective_status_bar_colors() {
        let source = SourceBuffer::from_bytes("sample", b"one\n").unwrap();
        let review = ReviewDocument::empty(source.source_ref());
        let mut app = App::new(source, review);
        let backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| drop(render_app(frame, &mut app)))
            .unwrap();

        let footer_cell = &terminal.backend().buffer()[(0, 7)];
        assert_eq!(footer_cell.fg, Color::Gray);
        assert_eq!(footer_cell.bg, Color::Black);
    }

    #[test]
    fn cramped_terminal_shows_a_clear_requirement() {
        let source = SourceBuffer::from_bytes("sample", b"one\n").unwrap();
        let review = ReviewDocument::empty(source.source_ref());
        let mut app = App::new(source, review);
        let backend = TestBackend::new(15, 2);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| drop(render_app(frame, &mut app)))
            .unwrap();
        assert!(terminal.backend().to_string().contains("annotui needs"));

        let source = SourceBuffer::from_bytes("sample", b"one\n").unwrap();
        let review = ReviewDocument::empty(source.source_ref());
        let mut app = App::new(source, review);
        let backend = TestBackend::new(40, MINIMUM_HEIGHT - 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| drop(render_app(frame, &mut app)))
            .unwrap();
        assert!(terminal.backend().to_string().contains("annotui needs"));
    }

    #[test]
    fn renders_existing_comments_and_inline_editor() {
        let source = SourceBuffer::from_bytes("sample", b"one\ntwo\nthree\n").unwrap();
        let mut review = ReviewDocument::empty(source.source_ref());
        review.upsert_comment(crate::domain::Comment {
            id: 1,
            start_line: 1,
            end_line: 2,
            body: "first line\nsecond line".into(),
        });
        let mut app = App::new(source, review);
        let backend = TestBackend::new(80, 15);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| hits = render_app(frame, &mut app))
            .unwrap();
        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("1 ┃ one"));
        assert!(rendered.contains("2 ┃ two"));
        assert!(rendered.contains("3 │ three"));
        assert!(rendered.contains("└─ first line"));
        assert!(hits.iter().any(|hit| hit.target == HitTarget::Comment(1)));

        app.move_to_line(2);
        app.move_browse_focus(1);
        terminal
            .draw(|frame| hits = render_app(frame, &mut app))
            .unwrap();
        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("▶─ first line"));
        assert!(!rendered.contains("▶2 ┃ two"));

        app.begin_edit(1);
        terminal
            .draw(|frame| hits = render_app(frame, &mut app))
            .unwrap();
        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("Comment on lines 1–2"));
        assert!(!rendered.contains("Ctrl-O"));
        assert!(!rendered.contains("Ctrl-A"));

        app.cancel_editor();
        app.review.comments[0].body = "\tcode".into();
        terminal
            .draw(|frame| drop(render_app(frame, &mut app)))
            .unwrap();
        assert!(terminal.backend().to_string().contains("▶─     code"));
    }

    #[test]
    fn following_cursor_scrolls_long_documents() {
        let text = (1..=30)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let source = SourceBuffer::from_bytes("long", text.as_bytes()).unwrap();
        let review = ReviewDocument::empty(source.source_ref());
        let mut app = App::new(source, review);
        app.move_to_line(30);
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| drop(render_app(frame, &mut app)))
            .unwrap();
        assert!(app.scroll_row > 0);
    }

    #[test]
    fn source_lines_word_wrap_with_logical_line_hit_targets() {
        let source =
            SourceBuffer::from_bytes("sample", b"alpha beta gamma delta epsilon\nsecond line\n")
                .unwrap();
        let review = ReviewDocument::empty(source.source_ref());
        let mut app = App::new(source, review);
        let backend = TestBackend::new(24, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut hits = Vec::new();

        terminal
            .draw(|frame| hits = render_app(frame, &mut app))
            .unwrap();

        assert!(terminal.backend().to_string().contains("· │"));
        assert!(
            hits.iter()
                .filter(|hit| hit.target == HitTarget::SourceLine(1))
                .count()
                > 1
        );
        assert_eq!(
            wrapped_line("alpha beta gamma", 10),
            ["alpha beta", "gamma"]
        );
        assert_eq!(wrapped_line("界界界", 4), ["界界", "界"]);
        assert_eq!(wrapped_line("e\u{301}abc", 2), ["e\u{301}a", "bc"]);

        app.toggle_word_wrap();
        terminal
            .draw(|frame| hits = render_app(frame, &mut app))
            .unwrap();
        assert_eq!(
            hits.iter()
                .filter(|hit| hit.target == HitTarget::SourceLine(1))
                .count(),
            1
        );
    }

    #[test]
    fn keyboard_focused_comment_is_scrolled_into_view() {
        let text = (1..=30)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let source = SourceBuffer::from_bytes("long", text.as_bytes()).unwrap();
        let mut review = ReviewDocument::empty(source.source_ref());
        review.upsert_comment(crate::domain::Comment {
            id: 1,
            start_line: 20,
            end_line: 20,
            body: "focused comment".into(),
        });
        let mut app = App::new(source, review);
        app.jump_comment(true);
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| drop(render_app(frame, &mut app)))
            .unwrap();

        let focused_row = document_rows(&app, 35)
            .iter()
            .position(|row| {
                matches!(
                    row,
                    DocumentRow::Comment {
                        id: 1,
                        first: true,
                        ..
                    }
                )
            })
            .unwrap();
        let content_height = 6;
        assert!(focused_row >= app.scroll_row);
        assert!(focused_row < app.scroll_row + content_height);
        assert!(terminal
            .backend()
            .to_string()
            .contains("▶─ focused comment"));
    }

    #[test]
    fn tabs_expand_to_four_column_stops_before_wrapping() {
        assert_eq!(expand_tabs("\talpha"), "    alpha");
        assert_eq!(expand_tabs("a\tb"), "a   b");
        assert_eq!(expand_tabs("界\tb"), "界  b");
        assert_eq!(wrapped_line("a\tb", 4), ["a", "b"]);
    }
}
