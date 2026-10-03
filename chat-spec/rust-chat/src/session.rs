//! A chat session between two users (spec: Message Lifecycle).
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

use crate::event::{ChatEvent, Listing, SessionReport};
use crate::message::Message;
use crate::search::search_pending;
use crate::store::{Store, StoreError};
use crate::user::UserId;
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
        })
    }

    /// Runs the session until it receives `Close` (or every sender is gone).
    /// Returns the store, so the caller owns it again afterwards.
    pub async fn run(mut self, mut events: mpsc::Receiver<ChatEvent>) -> Store {
        while let Some(event) = events.recv().await {
            // Every ChatEvent case must be handled here, or this will not compile.
            match event {
                ChatEvent::Send(m) => self.accept(m).await,
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
    async fn accept(&mut self, m: Message) {
        let in_this_chat =
            (m.from == self.a && m.to == self.b) || (m.from == self.b && m.to == self.a);
        if !in_this_chat {
            self.report.rejected += 1;
            return;
        }
        self.pending.push(m); // `m` moves into the list
        self.report.accepted += 1;
        if self.save_every > 0 && self.pending.len() >= self.save_every {
            self.save().await;
        }
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
                // `saved` goes out of scope here: those messages are freed now,
                // at a known point, not "whenever a garbage collector runs".
            }
            Err(failed) => {
                // Nothing was saved. Put the messages back at the front so the
                // next save retries them (spec: a failed save loses nothing).
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
        tx.send(ChatEvent::Send(
            Message::new(carol, bob, "can I join?").unwrap(),
        ))
        .await
        .unwrap();
        tx.send(ChatEvent::Send(Message::new(alice, bob, "hi").unwrap()))
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
}
