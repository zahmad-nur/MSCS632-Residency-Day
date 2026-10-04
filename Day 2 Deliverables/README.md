# One Chat App, Rust and Go

A CLI-based chat application developed in both Rust and Go. Both versions are command-line programs with simulated users and no networking. Two users can chat one-to-one, with every message stored in SQLite along with a timestamp and user IDs. Messages can also be filtered and searched by user or keyword.


## The Data Model

| Rule from the spec | Rust (`rust-chat/src/users.rs`, `messages.rs`) | Go (`go-chat/chat/users.go`, `messages.go`) |
|---|---|---|
| IDs are signed 64-bit | `UserId(i64)` newtype | `type UserID int64` |
| Username: 3-20 chars of a-z, 0-9, _ | `validate_username`, used by `User::new` | `ValidateUsername`, used by `NewUser` |
| Body: 1-500 characters (not bytes) | `chars().count()` | `utf8.RuneCountInString` |
| One-to-one: no messages to yourself | `MessageError::SelfMessage` | `ErrSelfMessage` |
| Timestamps in UTC, whole seconds | `DateTime<Utc>`, `trunc_subsecs(0)` | `time.Time`, `.UTC().Truncate(time.Second)` |
| No database ID until saved | `id: Option<i64>` (`None` = unsaved) | `ID int64` (`0` = unsaved), `IsSaved()` |
| Errors | `UserError` and `MessageError` enums, matched exhaustively | error values checked with `errors.Is` |

## Store and Search

| Piece | Rust (`store.rs`, `search.rs`) | Go (`store.go`, `search.go`) |
|---|---|---|
| SQLite library | `rusqlite` (bundled C SQLite) | `mattn/go-sqlite3` (same C SQLite, needs a C compiler: `xcode-select --install` on a Mac) |
| Schema | identical SQL: `users`, `messages`, CHECK rules, WAL | same |
| Save a batch | `save_batch(Vec<Message>)`: the batch is **moved in** and handed back with IDs, or handed back unsaved inside `SaveFailed` | `SaveBatch([]Message)`: writes IDs **into the caller's slice** (shared memory) after the commit |
| Rollback | automatic when the transaction value is dropped | explicit `defer tx.Rollback()` |
| Search kinds | `enum Search { User, Keyword, UserAndKeyword }`, `match` must cover all | `Search` struct with a `Kind` constant, `switch` needs a `default` |
| Search pending (unsaved) | returns `Vec<&Message>`: borrowed references, no copies | returns `[]Message`: copies |
| Reading rows | typed `row.get` inside a closure | `rows.Scan(&field, ...)` with pointers |

## Simulated Users (concurrency)

One **session** owns the chat state (history list, pending list, database). The two simulated users run at the same time and send it events over a **channel**; nobody else touches the state, so no lock is needed.

| Piece | Rust (`event.rs`, `session.rs`, `simulator.rs`) | Go (`event.go`, `session.go`, `simulator.go`) |
|---|---|---|
| Event type | `enum ChatEvent { Send, History, Search, Close }`, each case holds only its own data | `Event` struct with a `Kind` constant; every field exists for every kind |
| Concurrency | `tokio::spawn` async tasks, `#[tokio::main]` runtime (a library) | `go` keyword, goroutines (built into the language) |
| Channel | `tokio::sync::mpsc` (256 slots), replies on `oneshot` | `make(chan Event, 256)`, replies on a reply channel |
| Sending a message | `m` is **moved** into the channel; using it afterwards will not compile | the channel gets a **copy**; `m` is still usable |
| Exhaustive handling | `match` must cover every `ChatEvent` case | `switch` silently ignores an unhandled kind |
| Who may touch the state | the compiler guarantees only the session task owns it | a rule we follow; checked at run time by `go test -race` |
| Save every 100 | `pending.drain(..)` moves the batch out; saved messages freed right after the save | `SaveBatch(pending)`, then `clear(pending)` and `pending[:0]`; freed on the next GC run |
| Database calls | moved to a blocking thread with `spawn_blocking`, the store is moved there and back | called directly; the Go runtime handles the blocking call |
| After close | the session **hands the store back** to the caller | the caller still has the same `*Store` pointer all along |

Performance test (Day 3 Report, Table 2): `./scripts/bench.sh` runs 10,000 messages five times per language and prints time and peak memory. Use the numbers from your own machine.

## Extra: saved keywords (Day 1 Report, Appendix)

A user saves keywords and reruns them later. They live in a new `saved_keywords` table (user, keyword, created time), stored trimmed and lowercase, one row per user and keyword (`UNIQUE`). A rerun is the spec's keyword search over saved messages plus the pending list.

| Piece | Rust (`keywords.rs`) | Go (`keywords.go`) |
|---|---|---|
| Result of saving | `enum SaveOutcome { Added, AlreadySaved }` | `(keyword, added bool, err)` |
| New session events | 3 new `ChatEvent` cases | 3 new `EventKind` constants + fields on `Event` |
| Forgetting to handle a new event | **compile error** listing each missing case | builds, vets and runs; the sender waits forever |

See `docs/EVIDENCE_unhandled_events.md` for the real compiler and test output, ready for the report.

## The command prompt (live demo)

Running the program now starts an interactive prompt. The prompt is one more task / goroutine talking to the session over the channel, like the simulated users; `simulate` starts those users in the background, so messages keep arriving while you type. The scripted demo is still there with `--demo`.

```text
commands:
  send <from> <to> <text>             send a message
  history                             the whole conversation (saved and pending)
  search user <name>                  messages <name> sent in this chat
  search keyword <word>               messages containing <word> (capitals ignored)
  search user <name> keyword <word>   both filters at once
  keyword save <user> <word>          save a keyword for <user>
  keyword forget <user> <word>        remove one of <user>'s saved keywords
  keywords <user>                     rerun <user>'s saved keywords
  simulate <count>                    the two users send <count> messages in the background
  help                                show this list
  quit                                save everything and exit
```

Options: `--db FILE` (default `chat.db`), `--users alice,bob`, `--pause-ms N` (pause between simulated messages, default 300), `--quiet` (do not print messages as they arrive).

| Piece | Rust (`commands.rs`, `prompt.rs`) | Go (`chat/commands.go`, `cmd/chat/prompt.go`) |
|---|---|---|
| Commands | `enum Command`, parsed with slice patterns (`["send", from, to, text @ ..]`) | `Command` struct with a `Kind`, parsed with a `switch` on the first word |
| Reading the keyboard | async `next_line().await`: the thread is free for other tasks while waiting | `bufio.Scanner`: blocks this goroutine only |
| "Did the session accept my message?" | `Send { message, ack: Option<oneshot::Sender<bool>> }`: `None` for simulated users | `Ack chan bool` on `Event`: `nil` for simulated users; the session must check for `nil`, because sending on a `nil` channel blocks forever |
| Stopping simulated users on quit | `JoinHandle::abort()` cancels a task from outside | a goroutine cannot be stopped from outside: we `close(stop)` and each one checks it in a `select` |

### Suggested live demo (Sunday)

```text
go run ./cmd/chat --db demo.db          (or: cargo run -- --db demo.db)
send alice bob hey bob, did you finish the rust part?
send bob alice yes! lunch?
send alice alice talking to myself       <- refused: one-to-one only
simulate 30                              <- alice and bob chat in the background...
search keyword rust                      <- ...while you search
keyword save alice lunch
keywords alice                           <- saved and still-pending matches
simulate 150                             <- crosses 100: "(saved 100 messages to the database)"
quit                                     <- saves the rest, prints the batches
go run ./cmd/chat --db demo.db           <- reopen: "loaded N earlier messages"
history
```

## Testing

**Rust** keeps each file's tests at the bottom of the same file, inside `#[cfg(test)] mod tests` (compiled only for `cargo test`).
**Go** keeps them in separate files ending in `_test.go`, next to the code they test, in the same package (so they can use private names such as `s.db`). `go build` ignores these files; `go test` compiles and runs them.

| Code | Go test file | What it checks |
|---|---|---|
| `users.go` | `users_test.go` | username rule, `NewUser` |
| `messages.go` | `messages_test.go` | body length in characters, no self-messages, no ID until saved |
| `store.go` | `store_test.go` | users added once, IDs assigned on save, CHECK rules, rollback, bad paths |
| `search.go` | `search_test.go` | by user, by keyword (case, newest first, `%` and `_`), both, pending list |
| `session.go`, `simulator.go` | `session_test.go` | two users at once lose nothing, save every 100, outsiders rejected, history and search, reopen without duplicates, **failed save keeps messages and retries**, keywords through the session, **every event kind gets a reply** |
| `keywords.go` | `keywords_test.go` | keywords trimmed and lowercased, 1-50 characters, saved once per user, forget, rerun |
| `commands.go` | `commands_test.go` | every command parses; mistakes give the right usage message |
| everything | `benchmark_test.go` | speed and memory: save 100, keyword search over 10,000, pending search, whole session with 1,000 and 10,000 messages |

Useful Go test commands (run inside `go-chat/`):

```bash
go test ./...                          # all tests
go test -v ./chat                      # list every test by name
go test -run TestSearch -v ./chat      # only tests whose name matches
go test -race ./...                    # with the race detector (finds unsafe sharing between goroutines)
go test -cover ./chat                  # how much of the code the tests run (about 87% now)
go test -count=20 -race ./chat         # repeat 20 times to catch timing-dependent bugs
go test -run XXX -bench . -benchmem ./chat   # benchmarks only (XXX matches no test)
```

The Rust equivalents (inside `rust-chat/`): `cargo test`, `cargo test search` (names matching "search"), `cargo test -- --nocapture` (show printed output).

## Run it

```bash
# Go
cd go-chat
go test ./...                 # unit tests
go run ./cmd/chat             # the command prompt (type help)
go run ./cmd/chat --demo      # the scripted demo
cd ..

# Rust
cd rust-chat
cargo test                    # unit tests
cargo run                     # the command prompt (type help)
cargo run -- --demo           # the scripted demo
cd ..

# Everything: both test suites, then the checks that Go and Rust behave the same
chmod +x scripts/*.sh
./scripts/test_all.sh

# Expected: Go `ok` (25 tests), Rust `test result: ok. 21 passed`, then
# `PASS: scripted demo: Go and Rust identical (82 lines).` and `PASS: command prompt: Go and Rust identical (55 lines).`

scripts/bench.sh # runs the performance test (10,000 messages, five runs per language).
```




