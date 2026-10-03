#!/usr/bin/env bash
# Checks that the Go and Rust versions behave the same:
#   1. the scripted demo (--demo) prints the same text;
#   2. the same typed commands (scripts/prompt_input.txt) give the same output
#      at the prompt, and reopening the saved chat shows the same history.
# Times and file paths change on every run, so they are removed first.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/chatcmp.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

echo "Building..."
( cd "$ROOT/go-chat" && go build -o "$WORK/chat-go" ./cmd/chat )
( cd "$ROOT/rust-chat" && cargo build -q && cp target/debug/rust-chat "$WORK/chat-rust" )

strip_variable() {
  sed -E \
    -e 's/\[[0-9]{2}:[0-9]{2}:[0-9]{2}\]/[TIME]/' \
    -e 's/\[[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]+Z\]/[TIME]/' \
    -e 's#database [^ ,]*\.db#database DB#'
}

check() {   # $1 = what was compared
  if diff -u "$WORK/go.out" "$WORK/rust.out"; then
    echo "PASS: $1: Go and Rust identical ($(wc -l < "$WORK/go.out" | tr -d ' ') lines)."
  else
    echo "FAIL: $1: the outputs above differ."
    exit 1
  fi
}

# 1. Scripted demo.
"$WORK/chat-go" --demo 2>/dev/null > "$WORK/go.out"
"$WORK/chat-rust" --demo 2>/dev/null > "$WORK/rust.out"
check "scripted demo"

# 2. Typed commands, then reopen the same database and ask for the history.
for lang in go rust; do
  {
    "$WORK/chat-$lang" --db "$WORK/$lang.db" < "$ROOT/scripts/prompt_input.txt"
    echo "--- reopen"
    printf 'history\nquit\n' | "$WORK/chat-$lang" --db "$WORK/$lang.db"
  } 2>&1 | strip_variable > "$WORK/$lang.out"
done
check "command prompt"
