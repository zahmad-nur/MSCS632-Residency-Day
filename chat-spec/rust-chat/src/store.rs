//! The SQLite store (Day 1 Report: Storage Strategy and Table 5), using `rusqlite`.
//!
//! * One row per message, in a `messages` table that points at a `users` table.
//! * The database itself enforces the spec's rules with CHECK constraints, so a
//!   bad row is refused even if a bug lets it past the Rust code.
//! * A batch of messages is saved in ONE transaction: all rows or none.
//!
//! The SQL text is identical to go-chat/chat/store.go.

use crate::message::{Message, TIME_FORMAT};
use crate::user::{validate_username, User, UserError, UserId};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Params};
use std::collections::HashMap;
use std::fmt;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS users (
    id         INTEGER PRIMARY KEY,
    username   TEXT NOT NULL UNIQUE CHECK (length(username) BETWEEN 3 AND 20),
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
    id           INTEGER PRIMARY KEY,
    sender_id    INTEGER NOT NULL REFERENCES users(id),
    recipient_id INTEGER NOT NULL REFERENCES users(id),
    body         TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 500),
    timestamp    TEXT NOT NULL,
    CHECK (sender_id <> recipient_id)
);
CREATE INDEX IF NOT EXISTS idx_messages_pair_time ON messages(sender_id, recipient_id, timestamp);
";

/// The columns every message query reads, in this order.
pub(crate) const SELECT_MESSAGES: &str =
    "SELECT id, sender_id, recipient_id, body, timestamp FROM messages ";

/// Everything that can go wrong in the store. Library errors are wrapped so
/// callers deal with one type; `From` impls let `?` do the wrapping.
#[derive(Debug)]
pub enum StoreError {
    UnknownUser(String),
    InvalidUser(UserError),
    Db(rusqlite::Error),
    BadTime(chrono::ParseError),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::UnknownUser(name) => write!(f, "unknown user: {name}"),
            StoreError::InvalidUser(e) => write!(f, "{e}"),
            StoreError::Db(e) => write!(f, "database error: {e}"),
            StoreError::BadTime(e) => write!(f, "bad timestamp in database: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Db(e)
    }
}

impl From<chrono::ParseError> for StoreError {
    fn from(e: chrono::ParseError) -> Self {
        StoreError::BadTime(e)
    }
}

impl From<UserError> for StoreError {
    fn from(e: UserError) -> Self {
        StoreError::InvalidUser(e)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// A failed save: the error, plus the messages handed back unsaved.
#[derive(Debug)]
pub struct SaveFailed {
    pub error: StoreError,
    pub batch: Vec<Message>,
}

pub struct Store {
    conn: Connection,
}

fn parse_time(ts: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(ts)?.with_timezone(&Utc))
}

impl Store {
    /// Opens (or creates) the database file and makes sure the tables exist.
    /// Use ":memory:" for a temporary database (tests and the demo).
    pub fn open(path: &str) -> Result<Store> {
        let conn = Connection::open(path)?;
        // WAL lets reads continue while a write is in progress (spec, Table 5).
        // This pragma answers with one row (the new mode), so read it.
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    /// Returns the user with this name, adding them first if they are new.
    #[allow(dead_code)] // used by the chat session in the next step
    pub fn ensure_user(&self, name: &str) -> Result<User> {
        self.ensure_user_at(name, Utc::now())
    }

    /// Same as `ensure_user`, with a given creation time (tests and the demo).
    pub fn ensure_user_at(&self, name: &str, created_at: DateTime<Utc>) -> Result<User> {
        validate_username(name)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO users (username, created_at) VALUES (?1, ?2)",
            params![name, created_at.format(TIME_FORMAT).to_string()],
        )?;
        self.user(name)
    }

    /// Looks up an existing user by name.
    pub fn user(&self, name: &str) -> Result<User> {
        let row = self.conn.query_row(
            "SELECT id, username, created_at FROM users WHERE username = ?1",
            params![name],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        );
        match row {
            Ok((id, username, created)) => Ok(User {
                id: UserId(id),
                username,
                created_at: parse_time(&created)?,
            }),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Err(StoreError::UnknownUser(name.to_string()))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// User ID -> username, used to print messages with names.
    pub fn names(&self) -> Result<HashMap<UserId, String>> {
        let mut stmt = self.conn.prepare("SELECT id, username FROM users")?;
        let rows = stmt.query_map([], |r| Ok((UserId(r.get(0)?), r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
    }

    /// Saves a batch of messages as one row each, inside ONE transaction.
    ///
    /// Ownership: the batch is moved in (`Vec<Message>`, not `&[Message]`), so
    /// after the call the caller cannot touch those messages through the old
    /// variable. They come back either way:
    /// * success: with their new database IDs filled in;
    /// * failure: unchanged, inside `SaveFailed`, so a later save can retry
    ///   them (spec: nothing is lost because of a failed save).
    pub fn save_batch(
        &mut self,
        mut batch: Vec<Message>,
    ) -> std::result::Result<Vec<Message>, SaveFailed> {
        match self.insert_all(&batch) {
            Ok(ids) => {
                // IDs are filled in only after the commit succeeded.
                for (m, id) in batch.iter_mut().zip(ids) {
                    m.id = Some(id);
                }
                Ok(batch)
            }
            Err(error) => Err(SaveFailed { error, batch }),
        }
    }

    /// Inserts every message in one transaction and returns the new row IDs.
    fn insert_all(&mut self, batch: &[Message]) -> Result<Vec<i64>> {
        // If we leave early with `?`, `tx` is dropped and rolls back by itself.
        let tx = self.conn.transaction()?;
        let mut ids = Vec::with_capacity(batch.len());
        {
            let mut stmt = tx.prepare(
                "INSERT INTO messages (sender_id, recipient_id, body, timestamp) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for m in batch {
                stmt.execute(params![
                    m.from.0,
                    m.to.0,
                    m.body,
                    m.timestamp.format(TIME_FORMAT).to_string()
                ])?;
                ids.push(tx.last_insert_rowid());
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    /// The whole conversation between two users, oldest first.
    pub fn conversation(&self, a: UserId, b: UserId) -> Result<Vec<Message>> {
        self.query_messages(
            "WHERE (sender_id = ?1 AND recipient_id = ?2) OR (sender_id = ?2 AND recipient_id = ?1) ORDER BY timestamp, id",
            params![a.0, b.0],
        )
    }

    pub fn count_messages(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?)
    }

    /// Runs `SELECT_MESSAGES` + a WHERE clause and turns each row into a
    /// `Message`. Each column is read through a typed `get` inside the closure.
    pub(crate) fn query_messages<P: Params>(
        &self,
        where_clause: &str,
        args: P,
    ) -> Result<Vec<Message>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{SELECT_MESSAGES}{where_clause}"))?;
        let rows = stmt.query_map(args, |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, from, to, body, ts) = row?;
            out.push(Message {
                id: Some(id),
                from: UserId(from),
                to: UserId(to),
                body,
                timestamp: parse_time(&ts)?,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chrono::TimeZone;

    pub(crate) fn at(sec: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 3, 9, 0, sec).unwrap()
    }

    /// A fresh in-memory database with alice (1), bob (2) and carol (3).
    pub(crate) fn fixture() -> (Store, UserId, UserId, UserId) {
        let store = Store::open(":memory:").unwrap();
        let a = store.ensure_user_at("alice", at(0)).unwrap().id;
        let b = store.ensure_user_at("bob", at(0)).unwrap().id;
        let c = store.ensure_user_at("carol", at(0)).unwrap().id;
        (store, a, b, c)
    }

    pub(crate) fn msg(from: UserId, to: UserId, body: &str, sec: u32) -> Message {
        Message::new_at(from, to, body, at(sec)).unwrap()
    }

    #[test]
    fn users_are_added_once() {
        let (store, alice, _, _) = fixture();
        let again = store.ensure_user_at("alice", at(9)).unwrap();
        assert_eq!(again.id, alice); // same row, not a second alice
        assert_eq!(again.created_at, at(0)); // the original creation time
        assert!(matches!(
            store.ensure_user_at("Bad Name", at(0)),
            Err(StoreError::InvalidUser(_))
        ));
        assert!(matches!(
            store.user("nobody"),
            Err(StoreError::UnknownUser(_))
        ));
    }

    #[test]
    fn save_assigns_ids_and_loads_back() {
        let (mut store, alice, bob, carol) = fixture();
        let saved = store
            .save_batch(vec![
                msg(alice, bob, "hello", 1),
                msg(bob, alice, "hi there", 2),
                msg(carol, bob, "other chat", 3),
            ])
            .unwrap();
        assert_eq!(
            saved.iter().map(|m| m.id).collect::<Vec<_>>(),
            [Some(1), Some(2), Some(3)]
        );
        let convo = store.conversation(bob, alice).unwrap();
        assert_eq!(convo.len(), 2); // carol's message is a different conversation
        assert_eq!(convo[0], saved[0]); // same id, sender, recipient, body and time
        assert_eq!(convo[1], saved[1]);
    }

    #[test]
    fn database_refuses_bad_rows_and_rolls_back() {
        let (mut store, alice, bob, _) = fixture();
        // Built by hand to get past the Message checks, so the database must catch it.
        let to_self = Message {
            id: None,
            from: alice,
            to: alice,
            body: "me".into(),
            timestamp: at(1),
        };
        let empty = Message {
            id: None,
            from: alice,
            to: bob,
            body: String::new(),
            timestamp: at(1),
        };
        assert!(store.save_batch(vec![to_self]).is_err());
        assert!(store.save_batch(vec![empty]).is_err());
        // A good message followed by a bad one: the whole batch is rolled back.
        let bad = Message {
            id: None,
            from: alice,
            to: UserId(99),
            body: "who?".into(),
            timestamp: at(2),
        };
        let failed = store
            .save_batch(vec![msg(alice, bob, "fine", 1), bad])
            .unwrap_err();
        assert_eq!(store.count_messages().unwrap(), 0);
        // The messages come back unsaved (no IDs), ready to retry.
        assert_eq!(failed.batch.len(), 2);
        assert!(failed.batch.iter().all(|m| m.id.is_none()));
    }
}
