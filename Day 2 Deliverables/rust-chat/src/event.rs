//! The messages that tasks send to the chat session over its channel.
//!
//! Message handling is a set of events, and an enum is the natural Rust type
//! for "one of these": each case carries exactly the data it needs, and the
//! session's `match` must handle every case or the program will not compile.
//! (Go has no enums; go-chat/chat/event.go uses a struct with a Kind field.)

use crate::keywords::{SaveOutcome, SavedKeyword};
use crate::messages::Message;
use crate::search::Search;
use crate::store::StoreError;
use crate::users::UserId;
use tokio::sync::oneshot;

/// Messages from the database plus messages still only in memory.
#[derive(Debug, Default)]
pub struct Listing {
    pub saved: Vec<Message>,
    pub unsaved: Vec<Message>,
}

/// What the session reports when it closes.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SessionReport {
    /// Earlier messages read from SQLite when the session opened.
    pub loaded: usize,
    /// New messages accepted into the pending list.
    pub accepted: usize,
    /// Messages refused because they were not between the two chat users.
    pub rejected: usize,
    /// Size of each batch written to SQLite, in order.
    pub batches: Vec<usize>,
    /// Total messages written to SQLite during this session.
    pub saved: usize,
    /// Saves that failed (their messages stay pending and are retried).
    pub save_errors: usize,
}

/// Everything another task can ask the session to do.
///
/// Requests that need an answer carry a `oneshot::Sender`: a one-use reply
/// channel. Ownership of the sender moves into the event, so only the session
/// can answer, and it can answer only once.
#[derive(Debug)]
pub enum ChatEvent {
    /// A user posts a message. The message is moved into the event. `ack` is
    /// an optional reply: `Some` when the sender wants to know whether the
    /// message was accepted (the command prompt), `None` when it does not care
    /// (the simulated users). `Option` makes "no reply wanted" an explicit case.
    Send {
        message: Message,
        ack: Option<oneshot::Sender<bool>>,
    },
    /// The whole conversation so far: saved rows plus pending messages.
    History(oneshot::Sender<Result<Listing, StoreError>>),
    /// Search saved messages (SQL) and pending ones (in memory).
    Search {
        viewer: UserId,
        search: Search,
        reply: oneshot::Sender<Result<Listing, StoreError>>,
    },
    /// Save a keyword for a user (extra: saved keywords).
    SaveKeyword {
        user: UserId,
        keyword: String,
        reply: oneshot::Sender<Result<SaveOutcome, StoreError>>,
    },
    /// Remove one of a user's saved keywords. Replies true if it existed.
    ForgetKeyword {
        user: UserId,
        keyword: String,
        reply: oneshot::Sender<Result<bool, StoreError>>,
    },
    /// Rerun all of a user's saved keywords over saved and pending messages.
    RunSavedKeywords {
        viewer: UserId,
        reply: oneshot::Sender<Result<Vec<KeywordMatches>, StoreError>>,
    },
    /// Save whatever is still pending, then stop.
    Close(oneshot::Sender<SessionReport>),
}

impl ChatEvent {
    /// A `Send` event that wants no reply.
    pub fn message(message: Message) -> ChatEvent {
        ChatEvent::Send { message, ack: None }
    }
}

/// One saved keyword and the messages it finds.
#[derive(Debug)]
pub struct KeywordMatches {
    pub keyword: SavedKeyword,
    pub saved: Vec<Message>,
    pub unsaved: Vec<Message>,
}
