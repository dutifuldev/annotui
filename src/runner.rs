use std::{
    fs,
    io::{self, IsTerminal, Read, Write},
    path::Path,
};

use anyhow::{bail, Context};
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, ModifierKeyCode, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui_textarea::Scrolling;
use unicode_normalization::UnicodeNormalization;

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
    let resolved_left = resolve_destination(left)?;
    let resolved_right = resolve_destination(right)?;
    if resolved_left == resolved_right {
        return Ok(true);
    }
    if left.exists() && right.exists() {
        return same_file::is_same_file(left, right)
            .with_context(|| format!("compare {} and {}", left.display(), right.display()));
    }
    if paths_normalize_equally(&resolved_left, &resolved_right) {
        return Ok(true);
    }
    if paths_case_fold_equally(&resolved_left, &resolved_right)
        && nearest_existing_ancestor(&resolved_left)
            .or_else(|| nearest_existing_ancestor(&resolved_right))
            .is_some_and(filesystem_is_case_insensitive)
    {
        return Ok(true);
    }
    Ok(false)
}

fn paths_normalize_equally(left: &Path, right: &Path) -> bool {
    left.to_str()
        .zip(right.to_str())
        .is_some_and(|(left, right)| {
            left != right && normalize_path_text(left) == normalize_path_text(right)
        })
}

fn paths_case_fold_equally(left: &Path, right: &Path) -> bool {
    left.to_str()
        .zip(right.to_str())
        .is_some_and(|(left, right)| {
            normalize_path_text(left).to_lowercase() == normalize_path_text(right).to_lowercase()
        })
}

fn normalize_path_text(path: &str) -> String {
    path.nfc().collect()
}

fn nearest_existing_ancestor(path: &Path) -> Option<&Path> {
    path.ancestors().find(|ancestor| ancestor.exists())
}

fn filesystem_is_case_insensitive(path: &Path) -> bool {
    path.ancestors().any(|ancestor| {
        let Some(file_name) = ancestor.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        let Some(toggled_name) = toggle_first_ascii_letter(file_name) else {
            return false;
        };
        let toggled = ancestor.with_file_name(toggled_name);
        toggled.exists() && same_file::is_same_file(ancestor, toggled).unwrap_or(false)
    })
}

fn toggle_first_ascii_letter(value: &str) -> Option<String> {
    let mut toggled = value.to_owned();
    let (index, character) = value
        .char_indices()
        .find(|(_, character)| character.is_ascii_alphabetic())?;
    let replacement = if character.is_ascii_lowercase() {
        character.to_ascii_uppercase()
    } else {
        character.to_ascii_lowercase()
    };
    toggled.replace_range(
        index..index + character.len_utf8(),
        &replacement.to_string(),
    );
    Some(toggled)
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
    if !path
        .try_exists()
        .with_context(|| format!("inspect comments file {}", path.display()))?
    {
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
        Event::Key(key) => handle_key_event(key, app),
        Event::Mouse(mouse) => handle_mouse(mouse, app, hit_areas),
        Event::Paste(text) => {
            if let Some(editor) = app.editor.as_mut() {
                editor.textarea.insert_str(text);
            }
        }
        Event::FocusGained | Event::FocusLost | Event::Resize(_, _) => {}
    }
}

fn handle_key_event(key: KeyEvent, app: &mut App) {
    if key.kind == KeyEventKind::Release {
        if is_shift_modifier(key.code) {
            app.finish_shift_selection();
        }
        return;
    }

    if app.editor.is_none()
        && app.keyboard_shift_anchor.is_some()
        && !key.modifiers.contains(KeyModifiers::SHIFT)
        && !is_shift_modifier(key.code)
    {
        let finalized = app.finish_shift_selection();
        if finalized && key.code == KeyCode::Enter {
            return;
        }
    }
    handle_key(key, app);
}

fn is_shift_modifier(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Modifier(ModifierKeyCode::LeftShift | ModifierKeyCode::RightShift)
    )
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
        KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::ALT) => {
            app.toggle_word_wrap();
        }
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.extend_shift_selection(1);
        }
        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.extend_shift_selection(-1);
        }
        KeyCode::Down | KeyCode::Char('j') => app.move_browse_focus(1),
        KeyCode::Up | KeyCode::Char('k') => app.move_browse_focus(-1),
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
        KeyCode::Enter => app.open_focused_editor(),
        KeyCode::Esc => app.cancel_selection(),
        KeyCode::Char('e') => {
            app.edit_comment_at_cursor();
        }
        KeyCode::Char('d')
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            app.delete_comment_at_cursor();
        }
        KeyCode::Char(']') => app.jump_comment(true),
        KeyCode::Char('[') => app.jump_comment(false),
        _ => {}
    }
}

fn handle_mouse(mouse: MouseEvent, app: &mut App, hit_areas: &[HitArea]) {
    let target = hit_test(hit_areas, mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => match target {
            Some(HitTarget::SourceLine(line)) if app.editor.is_none() => {
                app.begin_selection(line);
                app.mouse_drag_anchor = Some(line);
            }
            Some(HitTarget::Comment(id)) if app.editor.is_none() => {
                app.mouse_drag_anchor = None;
                app.begin_edit(id);
            }
            _ => app.mouse_drag_anchor = None,
        },
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.mouse_drag_anchor.is_some() {
                if let Some(HitTarget::SourceLine(line)) = target {
                    app.extend_selection(line);
                }
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let active = app.mouse_drag_anchor.take().is_some();
            if active && app.editor.is_none() && app.selection.is_some() {
                if let Some(HitTarget::SourceLine(line)) = target {
                    app.extend_selection(line);
                }
                app.open_selected_editor();
            }
        }
        MouseEventKind::Up(_)
        | MouseEventKind::Down(_)
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => app.mouse_drag_anchor = None,
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
        MouseEventKind::Moved | MouseEventKind::Drag(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use crossterm::event::{KeyEvent, MouseEvent};
    use ratatui::layout::Rect;
    use tempfile::tempdir;

    use crate::{app::WordWrap, domain::ReviewDocument, source::SourceBuffer};

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
    fn shift_arrows_open_the_selected_range_when_shift_is_released() {
        let mut app = app();
        handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT), &mut app);
        handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT), &mut app);
        assert_eq!(app.selection.unwrap().normalized(), (1, 3));

        handle_event(
            Event::Key(KeyEvent::new_with_kind(
                KeyCode::Modifier(ModifierKeyCode::LeftShift),
                KeyModifiers::SHIFT,
                KeyEventKind::Release,
            )),
            &mut app,
            &[],
        );

        let editor = app.editor.as_ref().unwrap();
        assert_eq!((editor.start_line, editor.end_line), (1, 3));
    }

    #[test]
    fn shift_up_selects_a_reverse_range() {
        let mut app = app();
        app.move_to_line(3);
        handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT), &mut app);
        handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT), &mut app);
        handle_event(
            Event::Key(KeyEvent::new_with_kind(
                KeyCode::Modifier(ModifierKeyCode::RightShift),
                KeyModifiers::SHIFT,
                KeyEventKind::Release,
            )),
            &mut app,
            &[],
        );

        let editor = app.editor.as_ref().unwrap();
        assert_eq!((editor.start_line, editor.end_line), (1, 3));
    }

    #[test]
    fn first_unshifted_key_finalizes_selection_on_legacy_terminals() {
        let mut app = app();
        handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT), &mut app);

        handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            &mut app,
            &[],
        );

        let editor = app.editor.as_ref().unwrap();
        assert_eq!((editor.start_line, editor.end_line), (1, 2));
        assert_eq!(editor.body(), "x");
    }

    #[test]
    fn enter_finalizes_a_legacy_shift_selection_without_submitting_it() {
        let mut app = app();
        handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT), &mut app);

        handle_event(
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            &mut app,
            &[],
        );

        let editor = app.editor.as_ref().unwrap();
        assert_eq!((editor.start_line, editor.end_line), (1, 2));
        assert_eq!(editor.body(), "");
        assert!(app.status.is_none());
    }

    #[test]
    fn modified_d_does_not_delete_a_comment() {
        let mut app = app();
        app.review.upsert_comment(crate::domain::Comment {
            id: 1,
            start_line: 1,
            end_line: 1,
            body: "keep me".into(),
        });

        handle_key(
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
            &mut app,
        );

        assert_eq!(app.review.comments.len(), 1);
    }

    #[test]
    fn alt_z_toggles_word_wrap() {
        let mut app = app();
        assert_eq!(app.word_wrap, WordWrap::On);

        handle_key(
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::ALT),
            &mut app,
        );
        assert_eq!(app.word_wrap, WordWrap::Off);
        assert_eq!(app.status.as_deref(), Some("Word wrap off"));

        handle_key(
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::ALT),
            &mut app,
        );
        assert_eq!(app.word_wrap, WordWrap::On);
        assert_eq!(app.status.as_deref(), Some("Word wrap on"));
    }

    #[test]
    fn arrows_and_enter_open_an_existing_comment_for_keyboard_editing() {
        let mut app = app();
        app.review.upsert_comment(crate::domain::Comment {
            id: 1,
            start_line: 1,
            end_line: 1,
            body: "original".into(),
        });

        handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &mut app);
        assert_eq!(app.active_comment_id, Some(1));
        handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);
        assert_eq!(app.editor.as_ref().unwrap().comment_id, Some(1));
        handle_key(
            KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE),
            &mut app,
        );
        handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);

        assert_eq!(app.review.comments[0].body, "original!");
        assert_eq!(app.active_comment_id, Some(1));
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
    fn unrelated_mouse_releases_do_not_open_a_keyboard_selection() {
        let mut app = app();
        app.begin_selection(1);
        let hits = [HitArea::new(
            Rect::new(0, 1, 80, 1),
            HitTarget::SourceLine(1),
        )];

        handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Right),
                column: 5,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            &mut app,
            &hits,
        );
        assert!(app.editor.is_none());

        handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
            &mut app,
            &hits,
        );
        handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 5,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            &mut app,
            &hits,
        );
        assert!(app.editor.is_none());
        assert!(app.selection.is_some());
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

    #[test]
    fn recognizes_case_and_normalization_equivalent_paths() {
        assert!(paths_case_fold_equally(
            Path::new("/tmp/review.json"),
            Path::new("/TMP/REVIEW.JSON")
        ));
        assert!(!paths_case_fold_equally(
            Path::new("/tmp/review.json"),
            Path::new("/tmp/output.json")
        ));
        assert!(paths_normalize_equally(
            Path::new("/tmp/é.json"),
            Path::new("/tmp/e\u{301}.json")
        ));
        assert_eq!(
            toggle_first_ascii_letter("review.json").as_deref(),
            Some("Review.json")
        );
    }

    #[cfg(unix)]
    #[test]
    fn sidecar_metadata_errors_are_reported_before_review() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        symlink(&second, &first).unwrap();
        symlink(&first, &second).unwrap();
        let cli = Cli::try_parse_from([
            "annotui".into(),
            "--buffer".into(),
            "hello".into(),
            "--comments".into(),
            first.into_os_string(),
        ])
        .unwrap();
        let source = read_source(&cli).unwrap();

        assert!(read_review(&cli, &source)
            .unwrap_err()
            .to_string()
            .contains("inspect comments file"));
    }
}
