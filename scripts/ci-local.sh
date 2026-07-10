#!/usr/bin/env bash
set -euo pipefail

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

cargo fmt --check
cargo check --workspace
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo llvm-cov --workspace --all-targets \
  --ignore-filename-regex 'src/(main|runner|terminal)\.rs' \
  --fail-under-lines 85 \
  --summary-only
cargo audit
cargo mutants --workspace --timeout 120 \
  -f src/app.rs \
  -f src/domain.rs \
  -f src/output.rs \
  -f src/source.rs
slophammer-rs dry . --format json
slophammer-rs boundaries . --format json
slophammer-rs unsafe . --format json
slophammer-rs check . --format json
npx -y @simpledoc/simpledoc check
scripts/e2e-tmux.sh
git diff --check
