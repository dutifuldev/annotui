# Contributing

annotui accepts focused changes that preserve the separation between review data,
terminal presentation, and IO. Add or update tests for every behavior change.

Run the local gate before opening a pull request:

```sh
scripts/ci-local.sh
```

The gate requires Rust formatting, compilation, tests, Clippy, 85% line coverage,
dependency audit, mutation discovery, Slophammer, documentation validation, and the
tmux mouse smoke test. See [docs/2026-07-10-architecture.md](docs/2026-07-10-architecture.md) before moving
code across module boundaries.
