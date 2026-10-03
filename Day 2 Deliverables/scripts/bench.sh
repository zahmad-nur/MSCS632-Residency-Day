#!/usr/bin/env bash
# Performance test from the spec: two users send 10,000 messages in total,
# saved to a database file every 100 messages. Five runs per language.
# Usage: ./scripts/bench.sh [messages]        (default 10000)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MESSAGES="${1:-10000}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/chatbench.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

echo "Building release versions..."
( cd "$ROOT/go-chat" && go build -o "$WORK/chat-go" ./cmd/chat )
( cd "$ROOT/rust-chat" && cargo build --release -q && cp target/release/rust-chat "$WORK/chat-rust" )

# Peak memory comes from /usr/bin/time: macOS uses -l (bytes), Linux uses -v (kilobytes).
if [ "$(uname)" = "Darwin" ]; then
  TIME_FLAG=-l; peak_mb() { awk '/maximum resident set size/ {printf "%.1f", $1/1048576}' "$1"; }
else
  TIME_FLAG=-v; peak_mb() { awk -F: '/Maximum resident set size/ {printf "%.1f", $2/1024}' "$1"; }
fi

echo "Workload: $MESSAGES messages, 2 users, saved every 100, database file with WAL"
for lang in go rust; do
  for run in 1 2 3 4 5; do
    rm -f "$WORK"/bench.db*
    /usr/bin/time "$TIME_FLAG" "$WORK/chat-$lang" --demo --messages "$MESSAGES" --db "$WORK/bench.db" > /dev/null 2> "$WORK/err"
    ms=$(awk '/^time:/ {print $2; exit}' "$WORK/err")
    echo "$lang run $run: ${ms} ms, peak memory $(peak_mb "$WORK/err") MB, database $(wc -c < "$WORK/bench.db" | tr -d ' ') bytes"
  done
done
echo "Report the median of the five runs for each language (spec, Table 6)."
