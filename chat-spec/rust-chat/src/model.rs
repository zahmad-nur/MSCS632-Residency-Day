//! The data model from the spec (Table 1): `User` and `Message`.
//!
//! Rules from the spec:
//! * IDs are signed 64-bit (`i64`), because SQLite stores integers that way.
//! * Usernames are unique, 3-20 characters of a-z, 0-9 or _.
//! * A message goes from one user to exactly one other user (one-to-one only).
//! * A message body is 1-500 characters.
//! * Timestamps are UTC.
//! * A new message has no database ID until it is saved.

use chrono::{DateTime, SubsecRound, Utc};
use std::fmt;

/// How timestamps are printed and stored (UTC, whole seconds).
pub const TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// A user's database ID. A "newtype" around i64: the compiler treats it as a
/// different type from a plain number, so a message ID can never be passed
/// where a user ID is expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserId(pub i64);

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Everything that can be wrong with a user or a message. An enum: each case
/// carries exactly the data it needs, and `match` must handle every case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    InvalidUsername(String),
    InvalidBody(usize), // the length that was rejected
    SelfMessage,
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::InvalidUsername(name) => {
                write!(
                    f,
                    "invalid username '{name}': use 3-20 characters of a-z, 0-9 or _"
                )
            }
            ModelError::InvalidBody(len) => {
                write!(f, "message must be 1-500 characters (got {len})")
            }
            ModelError::SelfMessage => write!(f, "cannot send a message to yourself"),
        }
    }
}

impl std::error::Error for ModelError {}

#[derive(Debug, Clone, PartialEq)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub created_at: DateTime<Utc>,
}

impl User {
    /// Builds a user, checking the username rule first.
    pub fn new(id: UserId, username: &str, created_at: DateTime<Utc>) -> Result<User, ModelError> {
        validate_username(username)?;
        Ok(User {
            id,
            username: username.to_string(),
            created_at,
        })
    }
}

impl fmt::Display for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "user {} {} (created {})",
            self.id,
            self.username,
            self.created_at.format(TIME_FORMAT)
        )
    }
}

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
    pub fn new(from: UserId, to: UserId, body: &str) -> Result<Message, ModelError> {
        Message::new_at(from, to, body, Utc::now().trunc_subsecs(0))
    }

    /// A new message with a given time (used by tests and the demo, so their
    /// output is the same on every run).
    pub fn new_at(
        from: UserId,
        to: UserId,
        body: &str,
        timestamp: DateTime<Utc>,
    ) -> Result<Message, ModelError> {
        if from == to {
            return Err(ModelError::SelfMessage);
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

/// Usernames: 3-20 characters of a-z, 0-9 or _.
pub fn validate_username(name: &str) -> Result<(), ModelError> {
    let len = name.chars().count();
    let allowed = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if (3..=20).contains(&len) && allowed {
        Ok(())
    } else {
        Err(ModelError::InvalidUsername(name.to_string()))
    }
}

/// Message bodies: 1-500 characters. Characters, not bytes: "é" is one
/// character but two bytes.
pub fn validate_body(body: &str) -> Result<(), ModelError> {
    let len = body.chars().count();
    if (1..=500).contains(&len) {
        Ok(())
    } else {
        Err(ModelError::InvalidBody(len))
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
    fn usernames_follow_the_rule() {
        for ok in ["alice", "bob", "user_42", "abc", "a2345678901234567890"] {
            assert!(validate_username(ok).is_ok(), "{ok} should be allowed");
        }
        for bad in [
            "al",
            "Alice",
            "a-b-c",
            "has space",
            "",
            "a23456789012345678901",
        ] {
            assert_eq!(
                validate_username(bad),
                Err(ModelError::InvalidUsername(bad.to_string()))
            );
        }
    }

    #[test]
    fn user_new_checks_the_name() {
        let u = User::new(UserId(1), "alice", at(0)).unwrap();
        assert_eq!(u.to_string(), "user 1 alice (created 2026-10-03T09:00:00Z)");
        assert!(User::new(UserId(2), "Bob", at(0)).is_err());
    }

    #[test]
    fn bodies_are_1_to_500_characters() {
        assert!(validate_body("x").is_ok());
        assert!(validate_body(&"é".repeat(500)).is_ok()); // 500 characters, 1000 bytes
        assert_eq!(validate_body(""), Err(ModelError::InvalidBody(0)));
        assert_eq!(
            validate_body(&"x".repeat(501)),
            Err(ModelError::InvalidBody(501))
        );
    }

    #[test]
    fn messages_are_one_to_one() {
        assert_eq!(
            Message::new(UserId(1), UserId(1), "me"),
            Err(ModelError::SelfMessage)
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
