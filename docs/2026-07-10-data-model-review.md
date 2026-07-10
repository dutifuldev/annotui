---
title: "Data model review"
author: "Onur Solmaz <2453968+osolmaz@users.noreply.github.com>"
date: "2026-07-10"
---

# Data model review

The version 1 JSON model was reviewed with Schemator before the Rust types were
implemented.

Context: annotui stores one immutable source identity and local line-range comments.
Terminal layout, cursor state, timestamps, users, and speculative diff anchors are not
stable data and were intentionally excluded.

Commands:

```sh
npx -y @dutifuldev/schemator run \
  --source annotui-schema.md \
  --context annotui-schema-context.md \
  --out annotui-schemator
npx -y @dutifuldev/schemator report \
  --run annotui-schemator \
  --out annotui-schemator/final-report.md
npx -y @dutifuldev/schemator diff \
  --run annotui-schemator \
  --out annotui-schemator/graph-diff.md
```

The review converged after one iteration with nine initial and final fields, zero
applied or skipped changes, zero manual structural proposals, and zero consistency
warnings. The initial and final field graphs were identical:

| Field | Type | Required |
| --- | --- | --- |
| `version` | number | yes |
| `source` | object | yes |
| `source.name` | string | yes |
| `source.sha256` | string | yes |
| `comments` | array | yes |
| `comments[].id` | number | yes |
| `comments[].start_line` | number | yes |
| `comments[].end_line` | number | yes |
| `comments[].body` | string | yes |

The accepted product model is documented in
[the output format reference](2026-07-10-output-formats.md) and implemented by the
public Rust types in `src/domain.rs`.
