#!/usr/bin/env bash
set -euo pipefail

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
session="annotui-e2e-$$"
tmpdir=$(mktemp -d)

cleanup() {
  tmux kill-session -t "$session" 2>/dev/null || true
  rm -rf "$tmpdir"
}
trap cleanup EXIT

wait_for_pane() {
  expected=$1
  for _ in $(seq 1 100); do
    if tmux capture-pane -p -t "$session" 2>/dev/null | grep -Fq "$expected"; then
      return 0
    fi
    sleep 0.05
  done
  tmux capture-pane -p -t "$session" 2>/dev/null || true
  return 1
}

wait_for_file() {
  file=$1
  for _ in $(seq 1 100); do
    if [ -s "$file" ]; then
      return 0
    fi
    sleep 0.05
  done
  return 1
}

start_tui() {
  output=$1
  extra_args=$2
  tmux kill-session -t "$session" 2>/dev/null || true
  rm -f "$tmpdir/status" "$tmpdir/error.log"
  tmux new-session -d -s "$session" -x 100 -y 30 \
    "cd '$repo_root' && target/debug/annotui '$tmpdir/input.txt' $extra_args >'$output' 2>'$tmpdir/error.log'; printf '%s' \$? >'$tmpdir/status'"
  tmux set-option -t "$session" remain-on-exit on
  wait_for_pane "annotui ·"
}

finish_tui() {
  tmux send-keys -t "$session" q
  wait_for_file "$tmpdir/status"
  if [ "$(cat "$tmpdir/status")" != "0" ]; then
    cat "$tmpdir/error.log" >&2
    exit 1
  fi
}

printf 'quoted part line 1\nquoted part line 2\nunquoted line\n' >"$tmpdir/input.txt"
cargo build --quiet

start_tui "$tmpdir/output.md" "--comments '$tmpdir/review.json'"

# Header occupies row 1. Inject SGR mouse press/drag/release over source rows 2 and 3.
tmux send-keys -t "$session" Escape '[<0;10;2M'
tmux send-keys -t "$session" Escape '[<32;10;3M'
tmux send-keys -t "$session" Escape '[<0;10;3m'
wait_for_pane "Comment on lines 1"

tmux send-keys -t "$session" -l "human comment here ..."
tmux send-keys -t "$session" Enter
wait_for_pane "Comment saved"
wait_for_pane "┃ quoted part line 1"
wait_for_pane "┃ quoted part line 2"
finish_tui

printf '> quoted part line 1\n> quoted part line 2\n\nhuman comment here ...\n' >"$tmpdir/expected.md"
diff -u "$tmpdir/expected.md" "$tmpdir/output.md"

start_tui "$tmpdir/full.md" "--comments '$tmpdir/review.json' --format full"
finish_tui
printf '> quoted part line 1\n> quoted part line 2\n\nhuman comment here ...\n\n> unquoted line\n' >"$tmpdir/expected-full.md"
diff -u "$tmpdir/expected-full.md" "$tmpdir/full.md"

start_tui "$tmpdir/review-output.json" "--comments '$tmpdir/review.json' --format json"
finish_tui
python3 - "$tmpdir/review-output.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    review = json.load(handle)

assert review["version"] == 1
assert len(review["source"]["sha256"]) == 64
assert review["comments"] == [
    {
        "id": 1,
        "start_line": 1,
        "end_line": 2,
        "body": "human comment here ...",
    }
]
PY

start_tui "$tmpdir/shift-output.md" "--comments '$tmpdir/shift-review.json' --no-mouse"

# Select all three source lines with Shift-Down, then inject a left-Shift release
# using the Kitty keyboard protocol requested by the application.
tmux send-keys -t "$session" Escape '[1;2B'
tmux send-keys -t "$session" Escape '[1;2B'
tmux send-keys -t "$session" Escape '[57441;2:3u'
wait_for_pane "Comment on lines 1"

tmux send-keys -t "$session" -l "keyboard range"
tmux send-keys -t "$session" Enter
wait_for_pane "Comment saved"
wait_for_pane "┃ quoted part line 1"
wait_for_pane "┃ quoted part line 2"
wait_for_pane "┃ unquoted line"
finish_tui

printf '> quoted part line 1\n> quoted part line 2\n> unquoted line\n\nkeyboard range\n' >"$tmpdir/expected-shift.md"
diff -u "$tmpdir/expected-shift.md" "$tmpdir/shift-output.md"
