//! Extra: saved keywords (Day 1 Report, Appendix).
//!
//! A user saves words or phrases they care about and can rerun those searches
//! later. Keywords live in the `saved_keywords` table (user, keyword, created
//! time), so they survive restarts, and each rerun is the spec's keyword search.
//!
//! Keywords are stored trimmed and in lowercase: the search ignores capital
//! letters anyway, so "Rust" and "rust" are the same saved keyword.
//!
//! The SQL text is identical to go-chat/chat/keywords.go.

use crate::messages::{Message, TIME_FORMAT};
use crate::search::Search;
use crate::store::{Result, Store, StoreError};
use crate::users::UserId;
use chrono::{DateTime, Utc};
use rusqlite::params;

/// Longest keyword allowed, in characters.
pub const MAX_KEYWORD_CHARS: usize = 50;

#[derive(Debug, Clone, PartialEq)]
pub struct SavedKeyword {
    pub id: i64,
    pub user: UserId,
    pub keyword: String,
    pub created_at: DateTime<Utc>,
}

/// What saving a keyword did. An enum instead of a (keyword, bool) pair: the
/// caller must `match` and so cannot forget to check which case happened.
#[derive(Debug, Clone, PartialEq)]
pub enum SaveOutcome {
    /// A new keyword was stored.
    Added(SavedKeyword),
    /// The user had already saved it; the original row is returned.
    AlreadySaved(SavedKeyword),
}

/// Trims and lowercases a keyword, and checks its length (1-50 characters).
pub fn normalize_keyword(word: &str) -> Result<String> {
    let w = word.trim().to_lowercase();
    let len = w.chars().count();
    if (1..=MAX_KEYWORD_CHARS).contains(&len) {
        Ok(w)
    } else {
        Err(StoreError::InvalidKeyword(len))
    }
}

impl Store {
    /// Saves a keyword for a user, or reports that it was already saved.
    pub fn save_keyword(&self, user: UserId, word: &str, at: DateTime<Utc>) -> Result<SaveOutcome> {
        let keyword = normalize_keyword(word)?;
        // ON CONFLICT ... DO NOTHING: a second save of the same keyword changes
        // no rows instead of failing; `execute` returns how many rows changed.
        let changed = self.conn.execute(
            "INSERT INTO saved_keywords (user_id, keyword, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (user_id, keyword) DO NOTHING",
            params![user.0, keyword, at.format(TIME_FORMAT).to_string()],
        )?;
        let row = self.conn.query_row(
            "SELECT id, created_at FROM saved_keywords WHERE user_id = ?1 AND keyword = ?2",
            params![user.0, keyword],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )?;
        let created_at = DateTime::parse_from_rfc3339(&row.1)?.with_timezone(&Utc);
        let saved = SavedKeyword {
            id: row.0,
            user,
            keyword,
            created_at,
        };
        Ok(if changed == 1 {
            SaveOutcome::Added(saved)
        } else {
            SaveOutcome::AlreadySaved(saved)
        })
    }

    /// A user's saved keywords, in alphabetical order.
    pub fn keywords(&self, user: UserId) -> Result<Vec<SavedKeyword>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, keyword, created_at FROM saved_keywords WHERE user_id = ?1 ORDER BY keyword",
        )?;
        let rows = stmt.query_map(params![user.0], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, keyword, created) = row?;
            let created_at = DateTime::parse_from_rfc3339(&created)?.with_timezone(&Utc);
            out.push(SavedKeyword {
                id,
                user,
                keyword,
                created_at,
            });
        }
        Ok(out)
    }

    /// Removes one of a user's saved keywords. Returns true if it existed.
    pub fn forget_keyword(&self, user: UserId, word: &str) -> Result<bool> {
        let keyword = normalize_keyword(word)?;
        let changed = self.conn.execute(
            "DELETE FROM saved_keywords WHERE user_id = ?1 AND keyword = ?2",
            params![user.0, keyword],
        )?;
        Ok(changed == 1)
    }

    /// Reruns every saved keyword of `viewer` as a keyword search over the
    /// viewer's saved messages (newest first).
    pub fn run_saved_keywords(&self, viewer: UserId) -> Result<Vec<(SavedKeyword, Vec<Message>)>> {
        self.keywords(viewer)?
            .into_iter() // takes ownership of each keyword, no copies
            .map(|k| {
                let found = self.search(viewer, &Search::Keyword(k.keyword.clone()))?;
                Ok((k, found))
            })
            .collect() // stops at the first error, if any
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{at, fixture, msg};

    #[test]
    fn keywords_are_normalized_and_checked() {
        assert_eq!(normalize_keyword("  Rust ").unwrap(), "rust");
        assert!(matches!(
            normalize_keyword("   "),
            Err(StoreError::InvalidKeyword(0))
        ));
        assert!(matches!(
            normalize_keyword(&"x".repeat(51)),
            Err(StoreError::InvalidKeyword(51))
        ));
    }

    #[test]
    fn save_list_and_forget() {
        let (store, alice, bob, _) = fixture();
        assert!(matches!(
            store.save_keyword(alice, "rust", at(1)).unwrap(),
            SaveOutcome::Added(_)
        ));
        assert!(matches!(
            store.save_keyword(alice, "Lunch", at(2)).unwrap(),
            SaveOutcome::Added(_)
        ));
        // The same word again (any capitals) is not a second keyword.
        match store.save_keyword(alice, "RUST", at(3)).unwrap() {
            SaveOutcome::AlreadySaved(k) => assert_eq!(k.created_at, at(1)),
            SaveOutcome::Added(_) => panic!("rust was already saved"),
        }
        store.save_keyword(bob, "rust", at(4)).unwrap(); // keywords are per user
        let words: Vec<String> = store
            .keywords(alice)
            .unwrap()
            .into_iter()
            .map(|k| k.keyword)
            .collect();
        assert_eq!(words, ["lunch", "rust"]);

        assert!(store.forget_keyword(alice, "LUNCH").unwrap());
        assert!(!store.forget_keyword(alice, "lunch").unwrap()); // already gone
        assert_eq!(store.keywords(alice).unwrap().len(), 1);
        assert_eq!(store.keywords(bob).unwrap().len(), 1);
    }

    #[test]
    fn rerun_saved_keywords() {
        let (mut store, alice, bob, carol) = fixture();
        store
            .save_batch(vec![
                msg(alice, bob, "did you finish the rust part?", 1),
                msg(bob, alice, "Rust is done, lunch?", 2),
                msg(carol, bob, "rust rust rust", 3), // not alice's conversation
            ])
            .unwrap();
        store.save_keyword(alice, "rust", at(1)).unwrap();
        store.save_keyword(alice, "zebra", at(1)).unwrap();
        let results = store.run_saved_keywords(alice).unwrap();
        let counts: Vec<(&str, usize)> = results
            .iter()
            .map(|(k, m)| (k.keyword.as_str(), m.len()))
            .collect();
        assert_eq!(counts, [("rust", 2), ("zebra", 0)]);
    }
}
