//! Filtering and search by user or keyword (Day 1 Report: Retrieval and Search).
//!
//! * Saved messages are searched with SQL. A user only ever searches their own
//!   conversations, so every query is limited to messages the viewer sent or
//!   received.
//! * Messages still in memory (the pending list) are not in the database yet,
//!   so they are filtered in Rust (spec, Table 5, last row).
//!
//! The SQL text is identical to go-chat/chat/search.go.

use crate::message::Message;
use crate::store::{Result, Store};
use crate::user::UserId;
use rusqlite::params;

/// The three kinds of search. An enum: each case carries exactly the data it
/// needs, and every `match` below must handle all three or it will not compile.
#[derive(Debug, Clone, PartialEq)]
pub enum Search {
    /// Messages this user sent to the viewer.
    User(UserId),
    /// The viewer's messages containing this word (capital letters ignored).
    Keyword(String),
    /// Both filters at once ("messages from bob that mention rust").
    UserAndKeyword(UserId, String),
}

/// Spec: messages one person (?2) sent in the viewer's (?1) conversation with them.
const BY_USER: &str = "WHERE sender_id = ?2 AND recipient_id = ?1 ORDER BY timestamp, id";
/// Spec: the viewer's messages whose body contains the keyword, newest first.
const BY_KEYWORD: &str = "WHERE (sender_id = ?1 OR recipient_id = ?1) AND body LIKE '%' || ?2 || '%' ESCAPE '\\' ORDER BY timestamp DESC, id DESC";
/// Spec: the two filters combined with AND.
const BY_USER_AND_KEYWORD: &str = "WHERE sender_id = ?2 AND recipient_id = ?1 AND body LIKE '%' || ?3 || '%' ESCAPE '\\' ORDER BY timestamp DESC, id DESC";

/// In LIKE, % and _ are wildcards. Escape them so a search for "100%" or
/// "snake_case" matches those characters literally.
fn escape_like(word: &str) -> String {
    word.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

impl Store {
    /// Searches the saved messages in the viewer's conversations.
    pub fn search(&self, viewer: UserId, search: &Search) -> Result<Vec<Message>> {
        match search {
            Search::User(sender) => self.query_messages(BY_USER, params![viewer.0, sender.0]),
            Search::Keyword(word) => {
                self.query_messages(BY_KEYWORD, params![viewer.0, escape_like(word)])
            }
            Search::UserAndKeyword(sender, word) => self.query_messages(
                BY_USER_AND_KEYWORD,
                params![viewer.0, sender.0, escape_like(word)],
            ),
        }
    }
}

/// Searches the pending list (in memory, not saved yet) with the same rules.
///
/// Memory safety: the result holds *references* into `pending` (`&Message`),
/// not copies. The compiler will not let anyone change or free `pending`
/// while these results are still in use, so they can never point at
/// messages that have been moved away by a save.
pub fn search_pending<'a>(
    pending: &'a [Message],
    viewer: UserId,
    search: &Search,
) -> Vec<&'a Message> {
    let contains = |m: &Message, word: &str| m.body.to_lowercase().contains(&word.to_lowercase());
    pending
        .iter()
        .filter(|m| m.from == viewer || m.to == viewer)
        .filter(|m| match search {
            Search::User(sender) => m.from == *sender && m.to == viewer,
            Search::Keyword(word) => contains(m, word),
            Search::UserAndKeyword(sender, word) => {
                m.from == *sender && m.to == viewer && contains(m, word)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{fixture, msg};

    #[test]
    fn search_saved_messages() {
        let (mut store, alice, bob, carol) = fixture();
        store
            .save_batch(vec![
                msg(alice, bob, "100% done", 1),
                msg(bob, alice, "Report ready", 2),
                msg(alice, bob, "snake_case report", 3),
                msg(carol, bob, "carol's report", 4),
            ])
            .unwrap();

        // Messages alice sent in bob's conversation with her.
        assert_eq!(store.search(bob, &Search::User(alice)).unwrap().len(), 2);

        // Keyword: capitals ignored, newest first, only the viewer's conversations.
        let found = store
            .search(alice, &Search::Keyword("REPORT".into()))
            .unwrap();
        let bodies: Vec<&str> = found.iter().map(|m| m.body.as_str()).collect();
        assert_eq!(bodies, ["snake_case report", "Report ready"]); // not carol's

        // % and _ match themselves, not "anything".
        assert_eq!(
            store
                .search(alice, &Search::Keyword("%".into()))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .search(alice, &Search::Keyword("_".into()))
                .unwrap()
                .len(),
            1
        );

        // Both filters at once.
        let both = store
            .search(bob, &Search::UserAndKeyword(alice, "report".into()))
            .unwrap();
        assert_eq!(both.len(), 1);
        assert_eq!(both[0].body, "snake_case report");
    }

    #[test]
    fn search_pending_messages() {
        let (_, alice, bob, carol) = fixture();
        let pending = vec![
            msg(alice, bob, "Hello Bob", 1),
            msg(bob, alice, "Did you finish the Report?", 2),
            msg(alice, bob, "report sent", 3),
            msg(carol, bob, "report from carol", 4),
        ];
        assert_eq!(search_pending(&pending, bob, &Search::User(alice)).len(), 2);
        assert_eq!(
            search_pending(&pending, alice, &Search::Keyword("REPORT".into())).len(),
            2
        );
        assert_eq!(
            search_pending(
                &pending,
                alice,
                &Search::UserAndKeyword(bob, "report".into())
            )
            .len(),
            1
        );
        assert_eq!(
            search_pending(&pending, alice, &Search::Keyword("zebra".into())).len(),
            0
        );
    }
}
