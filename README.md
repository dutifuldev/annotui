# annotui

annotui is a mouse-first terminal UI for commenting on files and piped text.
It gives an immutable buffer a GitHub-style review surface, then writes the review
as concise Markdown or JSON.

```sh
annotui README.md
git diff | annotui
annotui --buffer "first line
second line"
```

Drag across source lines and release to open the inline comment editor. Enter saves
the comment. Existing comments remain visible below their ranges; reach them with
Up/Down and press Enter, or click one, to edit it.

## Install

Install from crates.io with Rust 1.88 or newer:

```sh
cargo install annotui
```

Or build from source:

```sh
git clone https://github.com/dutifuldev/annotui.git
cd annotui
cargo install --path .
```

## Output

The default `comments` format writes only reviewed ranges and comments:

```markdown
> quoted part line 1
> quoted part line 2

human comment here ...
```

The TUI uses `/dev/tty`, leaving standard output clean for pipelines:

```sh
git diff | annotui > review.md
annotui proposal.md --format full > reviewed-proposal.md
annotui src/main.rs --format json > review.json
```

`--format full` quotes the whole input and inserts comments after their selected
ranges. `--format json` writes the versioned line-range model. See
[the output format reference](docs/2026-07-10-output-formats.md) for exact behavior.

Use `--comments review.json` to load and save a JSON sidecar while still choosing an
independent final output format:

```sh
annotui proposal.md --comments proposal.review.json
annotui proposal.md --comments proposal.review.json --format full
```

The sidecar includes a source hash. annotui refuses to apply it to different content
instead of silently moving comments to the wrong lines.
`--comments` and `--output` must name different files.

## Controls

Mouse controls:

- Drag source lines and release to comment on the range.
- Click a comment to edit it.
- Use the wheel to scroll the document or active editor.
- Saved ranges keep a green rail beside every source line they cover.
- Long source lines wrap to the available terminal width.

Keyboard controls:

| Key | Action |
| --- | --- |
| `j` / `k`, Up/Down | Move between source lines and inline comments |
| `Shift-Up` / `Shift-Down` | Select a range; release Shift to write the comment |
| `v` | Start or cancel range selection |
| `Enter` | Comment on a source range, edit a focused comment, or save the editor |
| `Ctrl-O` | Insert a newline in a comment |
| `Esc` | Cancel selection or editing |
| `e` / `d` | Edit or delete a focused comment or one on the cursor line |
| `[` / `]` | Jump to the previous/next comment |
| `Alt-Z` | Toggle source-line word wrapping (on by default) |
| `q` | Write output and quit |

Pass `--no-mouse` when terminal mouse capture is undesirable. Native terminal text
selection is unavailable while mouse capture is active.

## Requirements and limits

annotui is a Unix utility and requires a controlling terminal at `/dev/tty`. Input
must currently be valid UTF-8. Source text is never modified.

Fun fact: [annotui was built in 13 user messages](docs/2026-07-10-built-in-13-messages.md).

## License

[MIT](LICENSE)
