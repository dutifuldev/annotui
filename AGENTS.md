# AGENTS.md

- Run `scripts/ci-local.sh` before finishing a change.
- Keep `#![forbid(unsafe_code)]` and the Slophammer unsafe policy enabled.
- Keep domain and source behavior independent of Ratatui, Crossterm, filesystem IO,
  process state, and terminal coordinates.
- Validate external JSON before it enters app state.
- Add tests for every behavior change, including mouse hit testing when layout changes.
- Prefer the standard library or existing dependencies over adding a crate.
- Follow the Slophammer Rust guidance at
  https://github.com/dutifuldev/slophammer/blob/main/docs/AGENT_ENTRYPOINT.md.
