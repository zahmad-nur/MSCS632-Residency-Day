#!/usr/bin/env bash
# Runs the Go demo and the Rust demo and checks that they print the same text.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/chatcmp.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

( cd "$ROOT/go-chat" && go run ./cmd/chat ) > "$WORK/go.out"
( cd "$ROOT/rust-chat" && cargo run -q ) > "$WORK/rust.out"

if diff -u "$WORK/go.out" "$WORK/rust.out"; then
  echo "PASS: Go and Rust print identical output ($(wc -l < "$WORK/go.out" | tr -d ' ') lines)."
else
  echo "FAIL: the outputs above differ."
  exit 1
fi
