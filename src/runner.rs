use std::{
    fs,
    io::{self, IsTerminal, Read, Write},
    path::Path,
};

use anyhow::{bail, Context};
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui_textarea::Scrolling;

use crate::{
    app::App,
    cli::Cli,
    domain::ReviewDocument,
    input::{hit_test, HitArea, HitTarget},
    output::format_review,
    render::render_app,
    source::SourceBuffer,
    storage::{load_review, resolve_destination, save_review},
    terminal::TerminalGuard,
};

/// Runs one interactive review and writes its selected output format.
///
/// # Errors
///
/// Returns an error for invalid input, terminal failures, invalid sidecars, or output failures.
pub fn run(cli: &Cli) -> anyhow::Result<()> {
    ensure_distinct_destinations(cli)?;
    let source = read_source(cli)?;
    let review = read_review(cli, &source)?;
    let mut app = App::new(source, review);

    {
        let (_guard, mut terminal) = TerminalGuard::enter(!cli.no_mouse)?;
        let mut hit_areas = Vec::new();
        while !app.should_quit {
            terminal.draw(|frame| hit_areas = render_app(frame, &mut app))?;
            handle_event(event::read()?, &mut app, &hit_areas);
        }
    }

    app.review.validate(app.source.line_count())?;
    if let Some(path) = &cli.comments {
        save_review(path, &app.review)?;
    }
    let output = format_review(cli.format, &app.source, &app.review)?;
    write_output(cli.output.as_deref(), &output)
}

fn read_source(cli: &Cli) -> anyhow::Result<SourceBuffer> {
    let (name, bytes) = if let Some(buffer) = &cli.buffer {
        (
            cli.source_name.as_deref().unwrap_or("(buffer)").to_owned(),
            buffer.as_bytes().to_vec(),
        )
    } else if let Some(path) = &cli.input {
        if path == Path::new("-") {
            (
                cli.source_name.as_deref().unwrap_or("(stdin)").to_owned(),
                read_stdin()?,
            )
        } else {
            let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
            (
                cli.source_name
                    .clone()
                    .unwrap_or_else(|| path.display().to_string()),
                bytes,
            )
        }
    } else if !io::stdin().is_terminal() {
        (
            cli.source_name.as_deref().unwrap_or("(stdin)").to_owned(),
            read_stdin()?,
        )
    } else {
        bail!("provide a file, pipe standard input, or pass --buffer")
    };

    if name.trim().is_empty() {
        bail!("source name must not be empty")
    }
    SourceBuffer::from_bytes(name, &bytes).context("input must be valid UTF-8")
}

fn ensure_distinct_destinations(cli: &Cli) -> anyhow::Result<()> {
    let mut paths = Vec::with_capacity(3);
    if let Some(input) = cli.input.as_deref().filter(|path| *path != Path::new("-")) {
        paths.push(("input", input));
    }
    if let Some(comments) = cli.comments.as_deref() {
        paths.push(("--comments", comments));
    }
    if let Some(output) = cli.output.as_deref() {
        paths.push(("--output", output));
    }

    for (index, (left_name, left_path)) in paths.iter().enumerate() {
        for (right_name, right_path) in &paths[index + 1..] {
            if paths_alias(left_path, right_path)? {
                bail!("{left_name} and {right_name} must refer to different files")
            }
        }
    }
    Ok(())
}

fn paths_alias(left: &Path, right: &Path) -> anyhow::Result<bool> {
    if resolve_destination(left)? == resolve_destination(right)? {
        return Ok(true);
    }
    if left.exists() && right.exists() {
        return same_file::is_same_file(left, right)
            .with_context(|| format!("compare {} and {}", left.display(), right.display()));
    }
    Ok(false)
}

fn read_stdin() -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut bytes)
        .context("read standard input")?;
    Ok(bytes)
}

fn read_review(cli: &Cli, source: &SourceBuffer) -> anyhow::Result<ReviewDocument> {
    let Some(path) = &cli.comments else {
        return Ok(ReviewDocument::empty(source.source_ref()));
    };
    if !path.exists() {
        return Ok(ReviewDocument::empty(source.source_ref()));
    }
    let mut review = load_review(path)?;
    review.validate(source.line_count())?;
    if review.source.sha256 != source.sha256() {
        bail!(
            "comments in {} belong to different source content",
            path.display()
        );
    }
    review.source = source.source_ref();
    review.sort_comments();
    Ok(review)
}

fn write_output(path: Option<&Path>, output: &str) -> anyhow::Result<()> {
    if output.is_empty() {
        if let Some(path) = path {
            fs::write(path, []).with_context(|| format!("clear output {}", path.display()))?;
        }
        return Ok(());
    }
    if let Some(path) = path {
        let mut file =
            fs::File::create(path).with_context(|| format!("create output {}", path.display()))?;
        writeln!(file, "{output}").with_context(|| format!("write output {}", path.display()))?;
    } else {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{output}").context("write standard output")?;
    }
    Ok(())
}

pub fn handle_event(event: Event, app: &mut App, hit_areas: &[HitArea]) {
    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release => handle_key(key, app),
        Event::Mouse(mouse) => handle_mouse(mouse, app, hit_areas),
        Event::Paste(text) => {
            if let Some(editor) = app.editor.as_mut() {
                editor.textarea.insert_str(text);
            }
        }
        Event::FocusGained | Event::FocusLost | Event::Resize(_, _) | Event::Key(_) => {}
    }
}

pub fn handle_key(key: KeyEvent, app: &mut App) {
    if app.editor.is_some() {
        handle_editor_key(key, app);
    } else {
        handle_browse_key(key, app);
    }
}

fn handle_editor_key(key: KeyEvent, app: &mut App) {
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) => app.cancel_editor(),
        (KeyCode::Enter, modifiers)
            if modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
        {
            if let Some(editor) = app.editor.as_mut() {
                editor.textarea.insert_newline();
            }
        }
        (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
            if let Some(editor) = app.editor.as_mut() {
                editor.textarea.insert_newline();
            }
        }
        (KeyCode::Enter, _) => {
            app.submit_editor();
        }
        _ => {
            if let Some(editor) = app.editor.as_mut() {
                editor.textarea.input(key);
            }
        }
    }
}

fn handle_browse_key(key: KeyEvent, app: &mut App) {
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Down | KeyCode::Char('j') => app.move_cursor(1),
        KeyCode::Up | KeyCode::Char('k') => app.move_cursor(-1),
        KeyCode::PageDown => app.move_cursor(10),
        KeyCode::PageUp => app.move_cursor(-10),
        KeyCode::Char('g') => app.move_to_line(1),
        KeyCode::Char('G') => app.move_to_line(app.source.line_count()),
        KeyCode::Char('v') => {
            if app.selection.is_some() {
                app.cancel_selection();
            } else {
                app.begin_selection(app.cursor_line);
            }
        }
        KeyCode::Enter => app.open_selected_editor(),
        KeyCode::Esc => app.cancel_selection(),
        KeyCode::Char('e') => {
            app.edit_comment_at_cursor();
        }
        KeyCode::Char('d') => {
            app.delete_comment_at_cursor();
        }
        KeyCode::Char(']') => app.jump_comment(true),
        KeyCode::Char('[') => app.jump_comment(false),
        KeyCode::Char('h') | KeyCode::Left => {
            app.horizontal_scroll = app.horizontal_scroll.saturating_sub(4);
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.horizontal_scroll = app.horizontal_scroll.saturating_add(4);
        }
        _ => {}
    }
}

fn handle_mouse(mouse: MouseEvent, app: &mut App, hit_areas: &[HitArea]) {
    let target = hit_test(hit_areas, mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => match target {
            Some(HitTarget::SourceLine(line)) if app.editor.is_none() => app.begin_selection(line),
            Some(HitTarget::Comment(id)) if app.editor.is_none() => {
                app.begin_edit(id);
            }
            _ => {}
        },
        MouseEventKind::Drag(MouseButton::Left) => {
            if let Some(HitTarget::SourceLine(line)) = target {
                app.extend_selection(line);
            }
        }
        MouseEventKind::Up(_) => {
            if app.editor.is_none() && app.selection.is_some() {
                if let Some(HitTarget::SourceLine(line)) = target {
                    app.extend_selection(line);
                }
                app.open_selected_editor();
            }
        }
        MouseEventKind::ScrollDown => {
            if let Some(editor) = app.editor.as_mut() {
                editor.textarea.scroll(Scrolling::PageDown);
            } else {
                app.scroll_row = app.scroll_row.saturating_add(3);
                app.follow_cursor = false;
            }
        }
        MouseEventKind::ScrollUp => {
            if let Some(editor) = app.editor.as_mut() {
                editor.textarea.scroll(Scrolling::PageUp);
            } else {
                app.scroll_row = app.scroll_row.saturating_sub(3);
                app.follow_cursor = false;
            }
        }
        MouseEventKind::ScrollLeft => {
            app.horizontal_scroll = app.horizontal_scroll.saturating_sub(4);
        }
        MouseEventKind::ScrollRight => {
            app.horizontal_scroll = app.horizontal_scroll.saturating_add(4);
        }
        MouseEventKind::Moved | MouseEventKind::Down(_) | MouseEventKind::Drag(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use crossterm::event::{KeyEvent, MouseEvent};
    use ratatui::layout::Rect;
    use tempfile::tempdir;

    use crate::{domain::ReviewDocument, source::SourceBuffer};

    use super::*;

    fn app() -> App {
        let source = SourceBuffer::from_bytes("sample", b"one\ntwo\nthree\n").unwrap();
        let review = ReviewDocument::empty(source.source_ref());
        App::new(source, review)
    }

    #[test]
    fn keyboard_selection_opens_and_submits_editor() {
        let mut app = app();
        handle_key(
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
            &mut app,
        );
        handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &mut app);
        handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);
        handle_key(
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            &mut app,
        );
        handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);

        assert_eq!(app.review.comments[0].start_line, 1);
        assert_eq!(app.review.comments[0].end_line, 2);
        assert_eq!(app.review.comments[0].body, "x");
    }

    #[test]
    fn mouse_drag_release_opens_editor_for_the_range() {
        let mut app = app();
        let hits = [
            HitArea::new(Rect::new(0, 1, 80, 1), HitTarget::SourceLine(1)),
            HitArea::new(Rect::new(0, 2, 80, 1), HitTarget::SourceLine(2)),
        ];
        for mouse in [
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 5,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 5,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
        ] {
            handle_mouse(mouse, &mut app, &hits);
        }
        let editor = app.editor.unwrap();
        assert_eq!((editor.start_line, editor.end_line), (1, 2));
    }

    #[test]
    fn rejects_empty_source_names_before_entering_the_tui() {
        let cli =
            Cli::try_parse_from(["annotui", "--buffer", "hello", "--source-name", "   "]).unwrap();
        assert!(read_source(&cli)
            .unwrap_err()
            .to_string()
            .contains("source name must not be empty"));
    }

    #[test]
    fn rejects_lexically_equivalent_sidecar_and_output_paths() {
        let directory = tempdir().unwrap();
        let comments = directory.path().join("review.json");
        let output = directory.path().join("nested/../review.json");
        let cli = Cli::try_parse_from([
            "annotui".into(),
            "--buffer".into(),
            "hello".into(),
            "--comments".into(),
            comments.into_os_string(),
            "--output".into(),
            output.into_os_string(),
        ])
        .unwrap();
        assert!(ensure_distinct_destinations(&cli)
            .unwrap_err()
            .to_string()
            .contains("different files"));

        let distinct = Cli::try_parse_from([
            "annotui",
            "--buffer",
            "hello",
            "--comments",
            "comments.json",
            "--output",
            "output.md",
        ])
        .unwrap();
        assert!(ensure_distinct_destinations(&distinct).is_ok());
    }

    #[test]
    fn rejects_output_that_aliases_the_input() {
        let directory = tempdir().unwrap();
        let input = directory.path().join("input.txt");
        std::fs::write(&input, "hello").unwrap();
        let cli = Cli::try_parse_from([
            "annotui".into(),
            input.clone().into_os_string(),
            "--output".into(),
            input.into_os_string(),
        ])
        .unwrap();
        let error = ensure_distinct_destinations(&cli).unwrap_err().to_string();
        assert!(error.contains("input and --output"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_dangling_symlink_that_would_alias_another_destination() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let comments = directory.path().join("review.json");
        let output = directory.path().join("output-link");
        symlink(&comments, &output).unwrap();
        let cli = Cli::try_parse_from([
            "annotui".into(),
            "--buffer".into(),
            "hello".into(),
            "--comments".into(),
            comments.into_os_string(),
            "--output".into(),
            output.into_os_string(),
        ])
        .unwrap();
        assert!(ensure_distinct_destinations(&cli).is_err());
    }

    #[test]
    fn rejects_hard_linked_input_and_output() {
        let directory = tempdir().unwrap();
        let input = directory.path().join("input.txt");
        let output = directory.path().join("output.txt");
        std::fs::write(&input, "hello").unwrap();
        std::fs::hard_link(&input, &output).unwrap();
        let cli = Cli::try_parse_from([
            "annotui".into(),
            input.into_os_string(),
            "--output".into(),
            output.into_os_string(),
        ])
        .unwrap();
        assert!(ensure_distinct_destinations(&cli).is_err());
    }
}
