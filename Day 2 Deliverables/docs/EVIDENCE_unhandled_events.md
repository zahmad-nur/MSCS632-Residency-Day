# Evidence: adding a new event without handling it

Captured while building the saved-keywords extra. In both languages we first
added three new event types (save, forget and rerun a keyword) **without**
writing the code that handles them, then tried to build and run.

## Rust: the program does not compile

We added `SaveKeyword`, `ForgetKeyword` and `RunSavedKeywords` to the
`ChatEvent` enum and an `InvalidKeyword` case to the `StoreError` enum.
`cargo build` refused, naming every case that the session's `match` (and the
error message's `match`) did not handle:

```text
error[E0004]: non-exhaustive patterns: `ChatEvent::SaveKeyword { .. }`, `ChatEvent::ForgetKeyword { .. }` and `ChatEvent::RunSavedKeywords { .. }` not covered
  --> src/session.rs:64:19
   |
64 |             match event {
   |                   ^^^^^ patterns `ChatEvent::SaveKeyword { .. }`, `ChatEvent::ForgetKeyword { .. }` and `ChatEvent::RunSavedKeywords { .. }` not covered
   |
note: `ChatEvent` defined here
  --> src/event.rs:45:10
   |
45 | pub enum ChatEvent {
   |          ^^^^^^^^^
...
57 |     SaveKeyword {
   |     ----------- not covered
...
63 |     ForgetKeyword {
   |     ------------- not covered
...
69 |     RunSavedKeywords {
   |     ---------------- not covered
   = note: the matched value is of type `ChatEvent`
help: ensure that all possible cases are being handled by adding a match arm with a wildcard pattern, a match arm with multiple or-patterns as shown, or multiple match arms
   |
89 ~                 },
90 +                 ChatEvent::SaveKeyword { .. } | ChatEvent::ForgetKeyword { .. } | ChatEvent::RunSavedKeywords { .. } => todo!()
   |

error[E0004]: non-exhaustive patterns: `&StoreError::InvalidKeyword(_)` not covered
  --> src/store.rs:59:15
   |
59 |         match self {
   |               ^^^^ pattern `&StoreError::InvalidKeyword(_)` not covered
   |
note: `StoreError` defined here
  --> src/store.rs:48:10
   |
48 | pub enum StoreError {
   |          ^^^^^^^^^^
...
51 |     InvalidKeyword(usize),
   |     -------------- not covered
   = note: the matched value is of type `&StoreError`
help: ensure that all possible cases are being handled by adding a match arm with a wildcard pattern or an explicit pattern as shown
   |
63 ~             StoreError::BadTime(e) => write!(f, "bad timestamp in database: {e}"),
64 ~             &StoreError::InvalidKeyword(_) => todo!(),
   |

For more information about this error, try `rustc --explain E0004`.
error: could not compile `rust-chat` (bin "rust-chat") due to 2 previous errors
```

The mistake is found **before the program runs**, at the exact line to fix.

## Go: the program builds and runs, then waits forever

We added `EventSaveKeyword`, `EventForgetKeyword` and `EventRunSavedKeywords`
constants and fields to the `Event` struct. `go build` and `go vet`
reported nothing. The session's `switch` skipped the new kind without a
word, so a goroutine that sent `EventSaveKeyword` and waited for the reply
would have waited forever. Only a test with a time limit caught it:

```text
$ go build ./...     (no errors)
$ go vet ./...       (no warnings)
$ go test -run TestUnhandledEventKind ./chat
--- FAIL: TestUnhandledEventKind (2.00s)
    unhandled_test.go:18: no reply after 2 seconds: the session ignored the event, and the sender would wait forever
FAIL
FAIL	chatcompare/chat	2.005s
FAIL
```

The mistake is found **while the program runs**, and only if a test happens to
send that event. To make up for it, `session_test.go` now has
`TestEveryEventKindGetsAReply`, which sends every kind with a 2-second limit.

## For the report

* **Rust:** enums are *closed*: the compiler knows every case and checks every
  `match`. Adding a case turns every place that must change into a compile error.
* **Go:** an "enum" is a set of integer constants; a `switch` over them is not
  checked. Safety comes from tests and discipline (here: a timeout test).
* Trade-off: Rust's check costs nothing at run time but makes every change touch
  every `match`; Go lets you add a kind quickly but can fail silently.
