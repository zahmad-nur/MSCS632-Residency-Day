//! Users (Day 1 Report, Table 1): id, username, created_at.
//!
//! Rules from the spec:
//! * IDs are signed 64-bit (`i64`), because SQLite stores integers that way.
//! * Usernames are unique, 3-20 characters of a-z, 0-9 or _.
//! * Timestamps are UTC.

use crate::messages::TIME_FORMAT;
use chrono::{DateTime, Utc};
use std::fmt;

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

/// What can be wrong with a user. An enum, so `match` must handle every case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserError {
    InvalidUsername(String),
}

impl fmt::Display for UserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UserError::InvalidUsername(name) => {
                write!(
                    f,
                    "invalid username '{name}': use 3-20 characters of a-z, 0-9 or _"
                )
            }
        }
    }
}

impl std::error::Error for UserError {}

#[derive(Debug, Clone, PartialEq)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub created_at: DateTime<Utc>,
}

impl User {
    /// Builds a user, checking the username rule first.
    pub fn new(id: UserId, username: &str, created_at: DateTime<Utc>) -> Result<User, UserError> {
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

/// Usernames: 3-20 characters of a-z, 0-9 or _.
pub fn validate_username(name: &str) -> Result<(), UserError> {
    let len = name.chars().count();
    let allowed = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if (3..=20).contains(&len) && allowed {
        Ok(())
    } else {
        Err(UserError::InvalidUsername(name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

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
                Err(UserError::InvalidUsername(bad.to_string()))
            );
        }
    }

    #[test]
    fn user_new_checks_the_name() {
        let at = Utc.with_ymd_and_hms(2026, 10, 3, 9, 0, 0).unwrap();
        let u = User::new(UserId(1), "alice", at).unwrap();
        assert_eq!(u.to_string(), "user 1 alice (created 2026-10-03T09:00:00Z)");
        assert!(User::new(UserId(2), "Bob", at).is_err());
    }
}
