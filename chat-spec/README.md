# Chat App, Rust and Go: built step by step from the spec

Each step adds one piece of the design spec in **both** languages, with tests,
and a check that both versions print exactly the same output.

| Step | What it adds | Status |
|---|---|---|
| 1 | `User` and `Message` types and their rules (spec, Table 1) | done |
| 2 | SQLite storage: schema, saving in a transaction | next |
| 3 | Chat session: history and pending lists, save every 100 and on close | |
| 4 | Two simulated users sending at the same time | |
| 5 | Search by user and by keyword with SQL | |
| 6 | Command prompt, demo, performance test | |

## Step 1: the data model

| Rule from the spec | Rust (`rust-chat/src/model.rs`) | Go (`go-chat/chat/model.go`) |
|---|---|---|
| IDs are signed 64-bit | `UserId(i64)` newtype | `type UserID int64` |
| Username: 3-20 chars of a-z, 0-9, _ | `validate_username`, used by `User::new` | `ValidateUsername`, used by `NewUser` |
| Body: 1-500 characters (not bytes) | `chars().count()` | `utf8.RuneCountInString` |
| One-to-one: no messages to yourself | `ModelError::SelfMessage` | `ErrSelfMessage` |
| Timestamps in UTC, whole seconds | `DateTime<Utc>`, `trunc_subsecs(0)` | `time.Time`, `.UTC().Truncate(time.Second)` |
| No database ID until saved | `id: Option<i64>` (`None` = unsaved) | `ID int64` (`0` = unsaved), `IsSaved()` |
| Errors | one `ModelError` enum, matched exhaustively | error values checked with `errors.Is` |

## Run it

```bash
# Go
cd go-chat
go test ./...        # unit tests
go run ./cmd/chat    # the demo
cd ..

# Rust
cd rust-chat
cargo test           # unit tests
cargo run            # the demo
cd ..

# Everything, plus the check that Go and Rust print the same output
chmod +x scripts/*.sh
./scripts/test_all.sh
```

Expected last line: `PASS: Go and Rust print identical output (25 lines).`
