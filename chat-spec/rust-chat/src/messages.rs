//! Messages (Day 1 Report, Table 1): id, from, to, body, timestamp.
//!
//! Rules from the spec:
//! * A message goes from one user to exactly one other user (one-to-one only).
//! * A message body is 1-500 characters.
//! * Timestamps are UTC.
//! * A new message has no database ID until it is saved.

use crate::user::UserId;
use chrono::{DateTime, SubsecRound, Utc};
use std::fmt;

/// How timestamps are printed and stored (UTC, whole seconds).
pub const TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// What can be wrong with a message. An enum: each case carries exactly the
/// data it needs, and `match` must handle every case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageError {
    InvalidBody(usize), // the length that was rejected
    SelfMessage,
}

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageError::InvalidBody(len) => {
                write!(f, "message must be 1-500 characters (got {len})")
            }
            MessageError::SelfMessage => write!(f, "cannot send a message to yourself"),
        }
    }
}

impl std::error::Error for MessageError {}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// `None` while the message exists only in memory. SQLite assigns the ID
    /// when the message is saved. Rust has `Option` for "maybe a value", so
    /// "no ID yet" cannot be confused with a real ID.
    pub id: Option<i64>,
    pub from: UserId,
    pub to: UserId,
    pub body: String,
    pub timestamp: DateTime<Utc>,
}

impl Message {
    /// A new message stamped with the current UTC time (whole seconds).
    // Not called by the step 1 demo (its output must be the same every run);
    // the live chat in the next step uses it.
    #[allow(dead_code)]
    pub fn new(from: UserId, to: UserId, body: &str) -> Result<Message, MessageError> {
        Message::new_at(from, to, body, Utc::now().trunc_subsecs(0))
    }

    /// A new message with a given time (used by tests and the demo, so their
    /// output is the same on every run).
    pub fn new_at(
        from: UserId,
        to: UserId,
        body: &str,
        timestamp: DateTime<Utc>,
    ) -> Result<Message, MessageError> {
        if from == to {
            return Err(MessageError::SelfMessage);
        }
        validate_body(body)?;
        Ok(Message {
            id: None,
            from,
            to,
            body: body.to_string(),
            timestamp,
        })
    }

    pub fn is_saved(&self) -> bool {
        self.id.is_some()
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} -> {}: {}",
            self.timestamp.format(TIME_FORMAT),
            self.from,
            self.to,
            self.body
        )?;
        // `match` on the Option: both cases must be handled.
        match self.id {
            Some(id) => write!(f, " (id {id})"),
            None => write!(f, " (unsaved)"),
        }
    }
}

/// Message bodies: 1-500 characters. Characters, not bytes: "é" is one
/// character but two bytes.
pub fn validate_body(body: &str) -> Result<(), MessageError> {
    let len = body.chars().count();
    if (1..=500).contains(&len) {
        Ok(())
    } else {
        Err(MessageError::InvalidBody(len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(sec: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 3, 9, 0, sec).unwrap()
    }

    #[test]
    fn bodies_are_1_to_500_characters() {
        assert!(validate_body("x").is_ok());
        assert!(validate_body(&"é".repeat(500)).is_ok()); // 500 characters, 1000 bytes
        assert_eq!(validate_body(""), Err(MessageError::InvalidBody(0)));
        assert_eq!(
            validate_body(&"x".repeat(501)),
            Err(MessageError::InvalidBody(501))
        );
    }

    #[test]
    fn messages_are_one_to_one() {
        assert_eq!(
            Message::new(UserId(1), UserId(1), "me"),
            Err(MessageError::SelfMessage)
        );
        let m = Message::new(UserId(1), UserId(2), "hi").unwrap();
        assert_eq!(m.timestamp.timestamp_subsec_nanos(), 0); // whole seconds
    }

    #[test]
    fn new_messages_have_no_id_until_saved() {
        let mut m = Message::new_at(UserId(1), UserId(2), "hey bob", at(5)).unwrap();
        assert!(!m.is_saved());
        assert_eq!(
            m.to_string(),
            "[2026-10-03T09:00:05Z] 1 -> 2: hey bob (unsaved)"
        );
        m.id = Some(7); // what the database will do when it saves the message
        assert!(m.is_saved());
        assert_eq!(
            m.to_string(),
            "[2026-10-03T09:00:05Z] 1 -> 2: hey bob (id 7)"
        );
    }
}