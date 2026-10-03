# Chat Spec: concurrent chat in Go and Rust

A **text-based** chat application implemented twice—once in **Go** and once in **Rust**—from the same design spec. Both versions model users and one-to-one messages, validate the same rules, and (in the later modules) simulate concurrent senders, persist history in SQLite, and search by user or keyword.

There is no GUI and no network server. “Multiple users” are **simulated in-process**. The two language implementations are meant to print the **same demo output** so behavior can be compared side by side.

## What it does

### Core behavior (spec)

| Requirement | How it is implemented |
|---|---|
| Simulate multiple users sending messages | Simulated users (`alice`, `bob`, …) send a fixed set of lines. In the session layer, each user is a **goroutine** (Go) or **async task** (Rust) writing to a shared channel. |
| Store history with timestamp and user ID | Each `Message` has sender ID, recipient ID, body, and a UTC timestamp (whole seconds). SQLite stores a `users` table and a `messages` table; the database assigns the message ID on save. |
| Filter / search by user or keyword | Three search kinds: by sender, by keyword (case-insensitive), or both. Saved rows are searched with SQL; unsaved pending messages are filtered in memory. A viewer only searches conversations they sent or received. |
| Text UI | Command-line demo that prints validation results, users, and messages. |

### Message and user rules

- **User IDs** are signed 64-bit integers (SQLite’s integer type).
- **Usernames** are unique, 3–20 characters, `a-z`, `0-9`, or `_` only.
- **Bodies** are 1–500 **characters** (Unicode scalar values / runes), not bytes. Example: `"é"` is one character and two UTF-8 bytes.
- **One-to-one only**: sending a message to yourself is rejected.
- **Timestamps** are UTC, truncated to whole seconds, printed as `YYYY-MM-DDTHH:MM:SSZ`.
- A new message has **no database ID** until it is saved (`Option<i64>` / `None` in Rust; `ID == 0` in Go).

### Session lifecycle (storage / concurrency modules)

1. **Open** — load the existing conversation between two users into a read-only history list.
2. **Chat** — new messages go into an in-memory **pending** list. They are saved in **one SQLite transaction** every 100 messages.
3. **Close** — flush remaining pending messages. A failed save rolls back and retries later; nothing is dropped.

Messages that do not belong to the open pair (for example Carol writing into Alice↔Bob) are rejected.

## Language-specific design

### Rust (`rust-chat`)

- **Memory safety**: ownership moves messages into the pending list and into channel events. After a send, the old variable cannot be used. `Option` makes “unsaved” distinct from a real ID.
- **Enums and structs**: `User`, `Message`, `UserId` newtype, `ModelError` / `MessageError` / `Search` enums. `match` must cover every case.
- **Async concurrency**: Tokio tasks for simulated users and the session; `mpsc` channels for `ChatEvent`s; `spawn_blocking` for SQLite so the async workers are not blocked on disk I/O.

### Go (`go-chat`)

- **Goroutines and channels**: one session goroutine owns history/pending/store. Simulated users send on a `chan Event`. No shared mutable state across goroutines by design (“share memory by communicating”).
- **Performance**: a buffered channel back-pressures fast senders; batch inserts in a single transaction; WAL mode so reads can proceed during a write.
- **Types**: named `UserID` so it is not mixed with a raw `int64` without a conversion. Errors are sentinel values checked with `errors.Is`.

## Project layout

```
chat-spec/
├── README.md                 # this file
├── requirements.txt          # toolchain and library inventory
├── scripts/
│   ├── test_all.sh           # Go tests + Rust tests + output compare
│   └── compare.sh            # both demos must print identical text
├── go-chat/
│   ├── go.mod                # module chatcompare, Go 1.22
│   ├── cmd/chat/main.go      # demo binary
│   └── chat/                 # library: model, store, session, search, simulator
└── rust-chat/
    ├── Cargo.toml
    ├── Cargo.lock
    └── src/
        ├── main.rs           # demo binary
        ├── model.rs          # User + Message (wired into the demo)
        ├── users.rs / messages.rs
        ├── store.rs          # SQLite
        ├── session.rs        # async session task
        ├── search.rs
        └── simulator.rs
```

**Current runnable entry point** is the **Step 1 data-model demo**: it constructs users and messages, prints which names/bodies are valid, and shows unsaved vs saved IDs. Storage, session, search, and simulator compile as part of the Go `chat` package (SQLite via `github.com/mattn/go-sqlite3`); they are not yet what `cmd/chat` prints.

## Prerequisites

Install both toolchains (see `requirements.txt` for versions):

```bash
# Go 1.22 or newer
go version

# Rust edition 2021 (rustc + cargo)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustc --version
cargo --version
```

On macOS, Xcode Command Line Tools are enough for a later CGO SQLite driver:

```bash
xcode-select --install
```

## Build, run, and test

Commands assume the repository root is `chat-spec/`.

### Go

```bash
cd go-chat

go test ./...          # unit tests
go test -race ./...    # optional: data-race detector for concurrency tests
go run ./cmd/chat      # demo (no install step required)

# compile a binary
go build -o chat ./cmd/chat
./chat
```

### Rust

```bash
cd rust-chat

cargo test             # unit tests (including tests inside model.rs)
cargo run              # demo

# optimized binary
cargo build --release
./target/release/rust-chat
```

The first `cargo run` / `cargo test` downloads crates from crates.io (currently `chrono` 0.4.45) and may take a minute.

### Both languages, plus identical-output check

```bash
chmod +x scripts/*.sh
./scripts/test_all.sh
```

That script:

1. Runs `go vet` and `go test` in `go-chat/`
2. Runs `cargo test` in `rust-chat/`
3. Runs both demos and `diff`s their stdout (`scripts/compare.sh`)

Success looks like:

```text
PASS: Go and Rust print identical output (25 lines).
```

## Demo walkthrough

Both binaries print the same sections:

1. **Usernames** — `alice`, `bob`, `user_42` succeed; `al`, `Alice`, `a-b-c`, and a 21-character name fail.
2. **Users** — Alice (id 1) and Bob (id 2) created at a fixed UTC time so the output is deterministic.
3. **Messages** — a valid pair of messages; self-send, empty body, and 501-character body fail; 500 accented `é` characters succeed (character count, not byte count).
4. **Pending** — valid messages still show `(unsaved)`.
5. **After saving** — IDs `1` and `2` are assigned the way SQLite would, then printed as `(id N)`.

## SQLite schema (storage modules)

Identical SQL in `go-chat/chat/store.go` and `rust-chat/src/store.rs`:

- `users(id, username UNIQUE, created_at)` with a length check on username
- `messages(id, sender_id, recipient_id, body, timestamp)` with foreign keys, a body length check, and `CHECK (sender_id <> recipient_id)`
- Index on `(sender_id, recipient_id, timestamp)`
- `PRAGMA journal_mode = WAL` and `foreign_keys = ON`

Use path `:memory:` for tests and the demo so no file is left on disk.

## Search (search modules)

| Kind | Meaning |
|---|---|
| By user | Messages that `sender` sent **to the viewer** |
| By keyword | Viewer’s sent or received messages whose body contains the word (case ignored) |
| User + keyword | Both filters combined |

`%` and `_` in a keyword are escaped so they match literally, not as SQL `LIKE` wildcards.
