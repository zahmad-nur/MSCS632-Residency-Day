//! The simulator: simulated users. Each user is an async task (started with `tokio::spawn`)
//! that sends messages to the session over its channel.

use crate::event::ChatEvent;
use crate::messages::Message;
use crate::users::UserId;
use std::time::Duration;
use tokio::sync::mpsc;

/// The things simulated users say. A user starts at `first_line` and goes
/// round the list, so every run sends the same texts (handy for tests).
pub const LINES: [&str; 12] = [
    "hey, are you around?",
    "yep, what's up",
    "did you finish the rust part?",
    "almost done, just testing now",
    "lunch anyone?",
    "sounds good, bye",
    "can you review my code?",
    "the build is green again",
    "who broke the tests?",
    "pushing it tonight",
    "great work today",
    "let's sync tomorrow morning",
];

/// One simulated user: sends `count` messages from `me` to `other`, waiting
/// `pause` between them. Returns how many messages were sent.
///
/// The channel sender (`events`) is moved into the task. Each user task owns
/// its own clone; the session's receiver sees all of their messages in the
/// order they arrive.
pub async fn run_user(
    me: UserId,
    other: UserId,
    count: usize,
    first_line: usize,
    pause: Duration,
    events: mpsc::Sender<ChatEvent>,
) -> usize {
    let mut sent = 0;
    for i in 0..count {
        let body = LINES[(first_line + i) % LINES.len()];
        let m = match Message::new(me, other, body) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("warning: {e}");
                continue;
            }
        };
        // `m` is moved into the event, and the event into the channel. Using
        // `m` after this line would be a compile error: the message now
        // belongs to the session.
        if events.send(ChatEvent::message(m)).await.is_err() {
            break; // the session has closed
        }
        sent += 1;
        if !pause.is_zero() {
            tokio::time::sleep(pause).await; // gives up the thread while waiting
        }
    }
    sent
}
