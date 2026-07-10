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
the comment. Existing comments remain visible below their ranges and can be clicked
to edit them.

## Install

annotui currently builds from source with Rust 1.88 or newer:

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

Keyboard controls:

| Key | Action |
| --- | --- |
| `j` / `k`, arrows | Move between source lines |
| `v` | Start or cancel range selection |
| `Enter` | Comment on the cursor/range, or save an active comment |
| `Ctrl-O` | Insert a newline in a comment |
| `Ctrl-A` / `Ctrl-E` | Move to the beginning/end of the editor line |
| `Esc` | Cancel selection or editing |
| `e` / `d` | Edit or delete a comment on the cursor line |
| `[` / `]` | Jump to the previous/next comment |
| `h` / `l` | Scroll long source lines horizontally |
| `q` | Write output and quit |

Pass `--no-mouse` when terminal mouse capture is undesirable. Native terminal text
selection is unavailable while mouse capture is active.

## Requirements and limits

annotui is a Unix utility and requires a controlling terminal at `/dev/tty`. Input
must currently be valid UTF-8. Source text is never modified.

## License

[MIT](LICENSE)
