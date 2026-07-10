---
title: "Output formats"
author: "Onur Solmaz <2453968+osolmaz@users.noreply.github.com>"
date: "2026-07-10"
---

# Output formats

annotui separates its terminal interface from final output. The TUI reads and writes
the controlling terminal at `/dev/tty`; the selected output format goes to stdout or
the path passed to `--output`.

## `comments`

`comments` is the default. Each comment becomes one Markdown block containing only
its selected source lines, followed by a blank line and the comment body:

```markdown
> first selected line
> second selected line

This is the comment.
```

Comments are ordered by start line, end line, then ID. Multiple comment blocks are
separated by a blank line. An empty review produces no output.

## `full`

`full` emits every source line as a Markdown blockquote. A comment is inserted after
the final line in its selected range:

```markdown
> first line
> second line

This comment covers the first two lines.

> third line
```

Source lines remain in their original order. The source is quoted so comments stay
visually distinct from the reviewed text.

## `json`

`json` emits the same versioned document used by `--comments` sidecars:

```json
{
  "version": 1,
  "source": {
    "name": "src/main.rs",
    "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
  },
  "comments": [
    {
      "id": 1,
      "start_line": 12,
      "end_line": 16,
      "body": "This branch can be simplified."
    }
  ]
}
```

Field rules:

- `version` is currently `1`.
- `source.name` is the displayed source name.
- `source.sha256` is the lowercase SHA-256 of the exact input bytes.
- `id` is a positive integer unique within the document.
- `start_line` and `end_line` are one-based and inclusive.
- `body` must contain a non-whitespace Markdown character. Its original indentation
  and surrounding whitespace are preserved.
- Unknown fields are rejected when a sidecar is loaded.

Sidecars are saved atomically. A loaded sidecar must have a supported version, valid
ranges, unique IDs, and a source hash matching the current input.
