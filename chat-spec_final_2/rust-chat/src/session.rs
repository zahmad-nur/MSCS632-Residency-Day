//! A chat session between two users (Day 1 Report: Message Lifecycle).
//!
//! The session runs as ONE async task that owns all chat state:
//! * `history`: the earlier conversation, read from SQLite when the session
//!   opens. Read-only: never changed, never saved again.
//! * `pending`: new messages, only in memory. Every `save_every` messages, and
//!   once more at close, they are saved to SQLite in one transaction and the
//!   pending list is emptied.
//! * `store`: the SQLite connection.
//!
//! Other tasks never touch this state. They send `ChatEvent`s over a channel,
//! so no lock is needed: the compiler knows the session task is the only owner.

use crate::event::{ChatEvent, KeywordMatches, Listing, SessionReport};
use crate::messages::Message;
use crate::search::{search_pending, Search};
use crate::store::{Store, StoreError};
use crate::users::UserId;
use chrono::Utc;
use std::collections::HashMap;
use tokio::sync::mpsc;

pub struct Session {
    a: UserId,
    b: UserId,
    /// `Some` at all times, except while a database call is running on a
    /// blocking thread (see `with_store`).
    store: Option<Store>,
    history: Vec<Message>,
    pending: Vec<Message>,
    save_every: usize,
    report: SessionReport,
    /// When set, every accepted message and every save is printed, with
    /// usernames from this map (used by the command prompt).
    echo: Option<HashMap<UserId, String>>,
}

impl Session {
    /// Step 1 of the lifecycle, Open: read the earlier conversation between
    /// the two users into the read-only history list. The store is moved in:
    /// from now on only the session can use it.
    pub fn open(
        store: Store,
        a: UserId,
        b: UserId,
        save_every: usize,
    ) -> Result<Session, StoreError> {
        let history = store.conversation(a, b)?;
        let report = SessionReport {
            loaded: history.len(),
            ..SessionReport::default()
        };
        Ok(Session {
            a,
            b,
            store: Some(store),
            history,
            pending: Vec::new(),
            save_every,
            report,
            echo: None,
        })
    }

    /// Prints every accepted message and every save from now on.
    pub fn echo_to_screen(mut self, names: HashMap<UserId, String>) -> Session {
        self.echo = Some(names);
        self
    }

    /// Runs the session until it receives `Close` (or every sender is gone).
    /// Returns the store, so the caller owns it again afterwards.
    pub async fn run(mut self, mut events: mpsc::Receiver<ChatEvent>) -> Store {
        while let Some(event) = events.recv().await {
            // Every ChatEvent case must be handled here, or this will not compile.
            match event {
                ChatEvent::Send { message, ack } => {
                    let accepted = self.accept(message).await;
                    // Only answer if the sender asked: `if let` handles both
                    // cases of the Option (Some: reply, None: nothing to do).
                    if let Some(ack) = ack {
                        let _ = ack.send(accepted);
                    }
                }
                ChatEvent::History(reply) => {
                    let listing = self.history().await;
                    let _ = reply.send(listing); // the asker may have gone; that is fine
                }
                ChatEvent::Search {
                    viewer,
                    search,
                    reply,
                } => {
                    let query = search.clone(); // the database thread needs its own copy
                    let saved = self.with_store(move |s| s.search(viewer, &query)).await;
                    // Results borrowed from `pending` cannot leave this task (they
                    // would outlive the borrow), so they are cloned into owned values.
                    let unsaved = search_pending(&self.pending, viewer, &search)
                        .into_iter()
                        .cloned()
                        .collect();
                    let _ = reply.send(saved.map(|saved| Listing { saved, unsaved }));
                }
                ChatEvent::SaveKeyword {
                    user,
                    keyword,
                    reply,
                } => {
                    let outcome = self
                        .with_store(move |s| s.save_keyword(user, &keyword, Utc::now()))
                        .await;
                    let _ = reply.send(outcome);
                }
                ChatEvent::ForgetKeyword {
                    user,
                    keyword,
                    reply,
                } => {
                    let removed = self
                        .with_store(move |s| s.forget_keyword(user, &keyword))
                        .await;
                    let _ = reply.send(removed);
                }
                ChatEvent::RunSavedKeywords { viewer, reply } => {
                    let found = self.with_store(move |s| s.run_saved_keywords(viewer)).await;
                    // Add each keyword's matches from the pending list (owned copies,
                    // since they leave this task through the reply channel).
                    let result = found.map(|list| {
                        list.into_iter()
                            .map(|(keyword, saved)| {
                                let search = Search::Keyword(keyword.keyword.clone());
                                let unsaved = search_pending(&self.pending, viewer, &search)
                                    .into_iter()
                                    .cloned()
                                    .collect();
                                KeywordMatches {
                                    keyword,
                                    saved,
                                    unsaved,
                                }
                            })
                            .collect()
                    });
                    let _ = reply.send(result);
                }
                ChatEvent::Close(reply) => {
                    self.save().await; // step 4, Close: save what is left
                    let _ = reply.send(self.report.clone());
                    break;
                }
            }
        }
        // If every sender was dropped without Close, still save what is left.
        self.save().await;
        // `history` and `pending` are freed here, when `self` goes out of scope.
        self.store.take().expect("store is always put back")
    }

    /// Step 2, Chat: add a new message to the pending list; save at 100.
    /// Returns false if the message was refused (not between the two users).
    async fn accept(&mut self, m: Message) -> bool {
        let in_this_chat =
            (m.from == self.a && m.to == self.b) || (m.from == self.b && m.to == self.a);
        if !in_this_chat {
            self.report.rejected += 1;
            return false;
        }
        if let Some(names) = &self.echo {
            let name = |id: UserId| names.get(&id).map_or("?", String::as_str);
            println!(
                "[{}] {} -> {}: {}",
                m.timestamp.format("%H:%M:%S"),
                name(m.from),
                name(m.to),
                m.body
            );
        }
        self.pending.push(m); // `m` moves into the list
        self.report.accepted += 1;
        if self.save_every > 0 && self.pending.len() >= self.save_every {
            self.save().await;
        }
        true
    }

    /// Steps 3 and 4: save every pending message in one transaction.
    async fn save(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        // drain(..) moves every message out of `pending` into `batch`. `pending`
        // is left empty but keeps its capacity for the next 100 messages.
        let batch: Vec<Message> = self.pending.drain(..).collect();
        match self.with_store(move |s| s.save_batch(batch)).await {
            Ok(saved) => {
                self.report.batches.push(saved.len());
                self.report.saved += saved.len();
                if self.echo.is_some() {
                    println!("(saved {} messages to the database)", saved.len());
                }
                // `saved` goes out of scope here: those messages are freed now,
                // at a known point, not "whenever a garbage collector runs".
            }
            Err(failed) => {
                // Nothing was saved. Put the messages back at the front so the
                // next save retries them (Day 1 Report: a failed save loses nothing).
                self.report.save_errors += 1;
                self.pending.splice(0..0, failed.batch);
            }
        }
    }

    /// The conversation: saved rows from SQLite, then pending messages.
    async fn history(&mut self) -> Result<Listing, StoreError> {
        let (a, b) = (self.a, self.b);
        let saved = self.with_store(move |s| s.conversation(a, b)).await?;
        Ok(Listing {
            saved,
            unsaved: self.pending.clone(),
        })
    }

    /// Runs a database call on a thread meant for blocking work, so the async
    /// workers keep handling other tasks meanwhile.
    ///
    /// Ownership round trip: the store is moved to that thread and moved back
    /// with the result. While it is away, `self.store` is `None`, and the
    /// compiler guarantees nothing else can be using it.
    async fn with_store<T, F>(&mut self, f: F) -> T
    where
        F: FnOnce(&mut Store) -> T + Send + 'static,
        T: Send + 'static,
    {
        let mut store = self.store.take().expect("store is always put back");
        let (store, out) = tokio::task::spawn_blocking(move || {
            let out = f(&mut store);
            (store, out)
        })
        .await
        .expect("database task panicked");
        self.store = Some(store);
        out
    }

    /// The earlier conversation loaded at open (read-only).
    pub fn loaded(&self) -> &[Message] {
        &self.history
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Search;
    use crate::simulator::run_user;
    use crate::store::tests::fixture;
    use std::time::Duration;
    use tokio::sync::oneshot;

    /// Starts a session task and returns its channel and join handle.
    fn start(
        store: Store,
        a: UserId,
        b: UserId,
    ) -> (mpsc::Sender<ChatEvent>, tokio::task::JoinHandle<Store>) {
        let session = Session::open(store, a, b, 100).unwrap();
        let (tx, rx) = mpsc::channel(256);
        (tx, tokio::spawn(session.run(rx)))
    }

    async fn close(tx: &mpsc::Sender<ChatEvent>) -> SessionReport {
        let (reply, answer) = oneshot::channel();
        tx.send(ChatEvent::Close(reply)).await.unwrap();
        answer.await.unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_users_at_once_lose_nothing() {
        let (store, alice, bob, _) = fixture();
        let (tx, handle) = start(store, alice, bob);
        let u1 = tokio::spawn(run_user(alice, bob, 125, 0, Duration::ZERO, tx.clone()));
        let u2 = tokio::spawn(run_user(bob, alice, 125, 6, Duration::ZERO, tx.clone()));
        assert_eq!(u1.await.unwrap() + u2.await.unwrap(), 250);

        let report = close(&tx).await;
        assert_eq!(report.batches, vec![100, 100, 50]); // every 100, then the rest at close
        assert_eq!(
            (report.accepted, report.saved, report.rejected),
            (250, 250, 0)
        );

        let store = handle.await.unwrap(); // the session hands the store back
        assert_eq!(store.count_messages().unwrap(), 250);
        let ids: Vec<i64> = store
            .conversation(alice, bob)
            .unwrap()
            .iter()
            .filter_map(|m| m.id)
            .collect();
        assert_eq!(ids.len(), 250);
    }

    #[tokio::test]
    async fn messages_from_outside_the_chat_are_rejected() {
        let (store, alice, bob, carol) = fixture();
        let (tx, handle) = start(store, alice, bob);
        tx.send(ChatEvent::message(
            Message::new(carol, bob, "can I join?").unwrap(),
        ))
        .await
        .unwrap();
        tx.send(ChatEvent::message(Message::new(alice, bob, "hi").unwrap()))
            .await
            .unwrap();
        let report = close(&tx).await;
        assert_eq!((report.accepted, report.rejected, report.saved), (1, 1, 1));
        assert_eq!(handle.await.unwrap().count_messages().unwrap(), 1);
    }

    #[tokio::test]
    async fn history_and_search_cover_saved_and_pending() {
        let (store, alice, bob, _) = fixture();
        let (tx, _handle) = start(store, alice, bob);
        run_user(alice, bob, 120, 0, Duration::ZERO, tx.clone()).await;

        let (reply, answer) = oneshot::channel();
        tx.send(ChatEvent::History(reply)).await.unwrap();
        let listing = answer.await.unwrap().unwrap();
        assert_eq!((listing.saved.len(), listing.unsaved.len()), (100, 20));

        // LINES[2] is "did you finish the rust part?": it is message 3, 15, 27, ...
        let (reply, answer) = oneshot::channel();
        tx.send(ChatEvent::Search {
            viewer: bob,
            search: Search::Keyword("RUST".into()),
            reply,
        })
        .await
        .unwrap();
        let found = answer.await.unwrap().unwrap();
        assert_eq!(found.saved.len() + found.unsaved.len(), 10); // 120 messages / 12 lines
        assert!(found.unsaved.iter().all(|m| m.id.is_none()));
    }

    #[tokio::test]
    async fn reopening_loads_history_without_saving_it_twice() {
        let (store, alice, bob, _) = fixture();
        let (tx, handle) = start(store, alice, bob);
        run_user(alice, bob, 5, 0, Duration::ZERO, tx.clone()).await;
        close(&tx).await;
        let store = handle.await.unwrap();

        let (tx, handle) = start(store, alice, bob); // the same chat again
        run_user(bob, alice, 3, 0, Duration::ZERO, tx.clone()).await;
        let report = close(&tx).await;
        assert_eq!((report.loaded, report.saved), (5, 3));
        assert_eq!(handle.await.unwrap().count_messages().unwrap(), 8);
    }

    /// Day 1 Report, Table 5: a failed save loses nothing. The messages stay pending
    /// and the next save writes them.
    #[tokio::test]
    async fn failed_save_keeps_messages_and_retries() {
        let path = std::env::temp_dir().join(format!("rust-chat-retry-{}.db", std::process::id()));
        let path = path.to_str().unwrap().to_string();
        let store = Store::open(&path).unwrap();
        let alice = store.ensure_user("alice").unwrap().id;
        let bob = store.ensure_user("bob").unwrap().id;
        // The session takes the store; a second connection to the same file
        // lets the test break and repair the table while the session runs.
        let other = Store::open(&path).unwrap();
        let session = Session::open(store, alice, bob, 2).unwrap(); // save every 2
        let (tx, rx) = mpsc::channel(16);
        let handle = tokio::spawn(session.run(rx));

        other.execute_for_test("ALTER TABLE messages RENAME TO messages_away");
        run_user(alice, bob, 2, 0, Duration::ZERO, tx.clone()).await; // this save fails

        // A History request is answered only after the two sends were handled.
        let (reply, answer) = oneshot::channel();
        tx.send(ChatEvent::History(reply)).await.unwrap();
        assert!(answer.await.unwrap().is_err()); // the table is still missing

        other.execute_for_test("ALTER TABLE messages_away RENAME TO messages");
        let report = close(&tx).await; // retries the same two messages
        assert_eq!((report.save_errors, report.saved), (1, 2));
        assert_eq!(report.batches, vec![2]);
        assert_eq!(handle.await.unwrap().count_messages().unwrap(), 2);
        drop(other);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{path}{suffix}"));
        }
    }

    #[tokio::test]
    async fn saved_keywords_through_the_session() {
        let (store, alice, bob, _) = fixture();
        let (tx, handle) = start(store, alice, bob);
        let (reply, answer) = oneshot::channel();
        tx.send(ChatEvent::SaveKeyword {
            user: alice,
            keyword: "Rust".into(),
            reply,
        })
        .await
        .unwrap();
        assert!(matches!(
            answer.await.unwrap().unwrap(),
            crate::keywords::SaveOutcome::Added(_)
        ));

        run_user(bob, alice, 120, 0, Duration::ZERO, tx.clone()).await; // 100 saved, 20 pending
        let (reply, answer) = oneshot::channel();
        tx.send(ChatEvent::RunSavedKeywords {
            viewer: alice,
            reply,
        })
        .await
        .unwrap();
        let results = answer.await.unwrap().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].keyword.keyword, "rust");
        assert_eq!(results[0].saved.len() + results[0].unsaved.len(), 10); // 120 / 12 lines

        let (reply, answer) = oneshot::channel();
        tx.send(ChatEvent::ForgetKeyword {
            user: alice,
            keyword: "rust".into(),
            reply,
        })
        .await
        .unwrap();
        assert!(answer.await.unwrap().unwrap());
        close(&tx).await;
        assert!(handle.await.unwrap().keywords(alice).unwrap().is_empty());
    }
}
