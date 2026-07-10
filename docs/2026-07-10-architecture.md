---
title: "Architecture"
author: "Onur Solmaz <2453968+osolmaz@users.noreply.github.com>"
date: "2026-07-10"
---

# Architecture

annotui keeps the reviewed text and comments independent of terminal behavior. The
source is immutable; only the review document changes.

```text
file / stdin / --buffer
          │
          ▼
   immutable source ───── review document
          │                     ▲
          ▼                     │
 virtual document rows ── app actions
          │                     ▲
          ├── Ratatui rendering │
          └── semantic hit map ─┘
```

`domain` owns the versioned review data and validation. `source` owns UTF-8 line
indexing and exact-byte hashing. Neither knows about Ratatui, Crossterm, the
filesystem, or process state.

`app` owns cursor, selection, editor, and viewport state. `render` projects that state
into virtual source/comment/editor rows and returns explicit rectangular hit targets.
Mouse input resolves through those targets rather than converting screen rows directly
to source lines, which remains correct when inline comments change the layout.

`runner` is the IO boundary. It reads the selected source, owns the event loop, loads
and saves sidecars, and writes the final format. `terminal` owns raw mode, alternate
screen, mouse capture, bracketed paste, and cleanup on both normal exit and panic.

The test strategy mirrors these boundaries: domain and formatter unit tests, reducer
and hit-test event tests, Ratatui `TestBackend` layout tests, and a tmux smoke test that
injects actual SGR mouse sequences through a pseudoterminal.
