#!/usr/bin/env bash
# Runs the unit tests of both apps, then checks both demos print the same output.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== Go tests =="
( cd go-chat && go vet ./... && go test -count=1 ./... )

echo
echo "== Rust tests =="
( cd rust-chat && cargo test -q )

echo
echo "== Same output (Go versus Rust) =="
./scripts/compare.sh
