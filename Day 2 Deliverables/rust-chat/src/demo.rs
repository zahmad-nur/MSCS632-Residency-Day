//! The scripted demo (run with `--demo`). It shows every part of the app in a
//! fixed order with fixed times, so it prints the same text on every run, and
//! the Go demo prints exactly the same text (scripts/compare.sh checks this).
//! Timings go to stderr, so they are not part of that check.
//!
//! Step 1: users and messages, with the spec's rules.
//! Step 2: saving to SQLite, and filtering and search by user or keyword.
//! Step 3: two simulated users chatting at the same time (async tasks).
//! Extra: saved keywords (Day 1 Report, Appendix).

use crate::event::ChatEvent;
use crate::keywords::SaveOutcome;
use crate::messages::{Message, TIME_FORMAT};
use crate::search::{search_pending, Search};
use crate::session::Session;
use crate::simulator::run_user;
use crate::store::Store;
use crate::users::{User, UserId};
use chrono::{DateTime, TimeZone, Utc};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

fn at(sec: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 3, 9, 0, sec).unwrap()
}

/// One message as a line, with usernames instead of IDs.
pub(crate) fn line(m: &Message, names: &HashMap<UserId, String>) -> String {
    let name = |id: UserId| names.get(&id).map_or("?", String::as_str);
    let status = match m.id {
        Some(id) => format!("id {id}"),
        None => "unsaved".to_string(),
    };
    format!(
        "[{}] {} -> {}: {} ({status})",
        m.timestamp.format(TIME_FORMAT),
        name(m.from),
        name(m.to),
        m.body
    )
}

fn print_results(title: &str, results: &[&Message], names: &HashMap<UserId, String>) {
    println!("{title}");
    for m in results {
        println!("  {}", line(m, names));
    }
    println!("  ({} found)", results.len());
}

/// Batch sizes for printing: all of them if there are a few, else a summary.
pub(crate) fn batch_text(batches: &[usize]) -> String {
    let list = |b: &[usize]| {
        b.iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    if batches.len() <= 6 {
        format!("[{}]", list(batches))
    } else {
        let last = batches[batches.len() - 1];
        format!(
            "[{}, ... {last}] ({} batches)",
            list(&batches[..3]),
            batches.len()
        )
    }
}

/// Save the pending list to SQLite every this many messages (Day 1 Report).
pub(crate) const SAVE_EVERY: usize = 100;

/// Settings for the scripted demo.
pub struct Options {
    /// Messages the two simulated users send in step 3.
    pub messages: usize,
    /// Pause between one user's messages in step 3.
    pub pause_ms: u64,
    /// Database for step 3 (":memory:" for a temporary one).
    pub db: String,
}

/// Runs the whole scripted demo.
pub async fn run(options: &Options) -> Result<(), Box<dyn std::error::Error>> {
    step1_users_and_messages();
    step2_store_and_search()?;
    let store = step3_simulated_users(options).await?;
    extra_saved_keywords(store).await?;
    Ok(())
}

fn step1_users_and_messages() {
    println!("== Step 1: usernames ==");
    let long = "x".repeat(21);
    for name in [
        "alice",
        "bob",
        "user_42",
        "al",
        "Alice",
        "a-b-c",
        long.as_str(),
    ] {
        match crate::users::validate_username(name) {
            Ok(()) => println!("{name}: ok"),
            Err(e) => println!("{name}: error: {e}"),
        }
    }

    println!("== Step 1: users ==");
    let alice = User::new(UserId(1), "alice", at(0)).expect("valid user");
    let bob = User::new(UserId(2), "bob", at(0)).expect("valid user");
    println!("{alice}");
    println!("{bob}");

    println!("== Step 1: messages ==");
    let attempts: [(&str, UserId, UserId, String); 6] = [
        (
            "hey",
            alice.id,
            bob.id,
            "hey bob, are you around?".to_string(),
        ),
        ("reply", bob.id, alice.id, "yep, what's up".to_string()),
        (
            "to self",
            alice.id,
            alice.id,
            "talking to myself".to_string(),
        ),
        ("empty", alice.id, bob.id, String::new()),
        ("501 chars", alice.id, bob.id, "x".repeat(501)),
        ("500 accented", alice.id, bob.id, "é".repeat(500)),
    ];
    for (i, (label, from, to, body)) in attempts.into_iter().enumerate() {
        match Message::new_at(from, to, &body, at(5 + i as u32)) {
            Ok(m) if m.body.len() <= 40 => println!("{label}: ok {m}"),
            Ok(m) => println!(
                "{label}: ok ({} characters, {} bytes)",
                m.body.chars().count(),
                m.body.len()
            ),
            Err(e) => println!("{label}: error: {e}"),
        }
    }
}

fn step2_store_and_search() -> Result<(), Box<dyn std::error::Error>> {
    println!("== Step 2: store ==");
    let mut store = Store::open(":memory:")?;
    let alice = store.ensure_user_at("alice", at(0))?.id;
    let bob = store.ensure_user_at("bob", at(0))?.id;
    let carol = store.ensure_user_at("carol", at(0))?.id;
    let names = store.names()?;
    println!("users: alice={alice} bob={bob} carol={carol}");

    let batch = vec![
        Message::new_at(alice, bob, "hey bob, are you around?", at(10))?,
        Message::new_at(bob, alice, "yep, what's up", at(11))?,
        Message::new_at(alice, bob, "did you finish the Report?", at(12))?,
        Message::new_at(bob, alice, "report is done, 100% tested", at(13))?,
        Message::new_at(carol, bob, "can you review my report?", at(14))?,
    ];
    // `batch` is moved into save_batch; `saved` is what comes back, now with IDs.
    let saved = store.save_batch(batch).map_err(|f| f.error)?;
    println!(
        "saved {} messages in one transaction, all have IDs: {}, ids {:?}",
        saved.len(),
        saved.iter().all(Message::is_saved),
        saved.iter().map(|m| m.id.unwrap_or(0)).collect::<Vec<_>>()
    );
    drop(saved); // the in-memory copies are freed here; the rows stay in SQLite

    let convo = store.conversation(alice, bob)?;
    print_results(
        "conversation alice <-> bob (oldest first):",
        &convo.iter().collect::<Vec<_>>(),
        &names,
    );

    println!("== Step 2: search saved messages (SQL) ==");
    let searches = [
        ("bob views: messages from alice", bob, Search::User(alice)),
        (
            "alice views: keyword \"REPORT\" (newest first)",
            alice,
            Search::Keyword("REPORT".into()),
        ),
        (
            "bob views: from alice AND keyword \"report\"",
            bob,
            Search::UserAndKeyword(alice, "report".into()),
        ),
        (
            "alice views: keyword \"100%\" (% matched literally)",
            alice,
            Search::Keyword("100%".into()),
        ),
        (
            "carol views: keyword \"report\" (only carol's chats)",
            carol,
            Search::Keyword("report".into()),
        ),
    ];
    for (title, viewer, search) in &searches {
        let found = store.search(*viewer, search)?;
        print_results(title, &found.iter().collect::<Vec<_>>(), &names);
    }

    println!("== Step 2: search pending messages (in memory) ==");
    let pending = vec![
        Message::new_at(alice, bob, "great, sending the report now", at(20))?,
        Message::new_at(bob, alice, "thanks", at(21))?,
    ];
    // `found` borrows from `pending`: no copies are made.
    let found = search_pending(&pending, alice, &Search::Keyword("report".into()));
    print_results("alice views: keyword \"report\" in pending", &found, &names);

    println!("== Step 2: the database enforces the rules ==");
    // Built by hand to skip the Message checks, so only the database can stop it.
    let to_self = Message {
        id: None,
        from: alice,
        to: alice,
        body: "me".into(),
        timestamp: at(30),
    };
    match store.save_batch(vec![to_self]) {
        Ok(_) => println!("message to self: saved (should not happen)"),
        Err(f) => println!(
            "message to self: refused, {} message handed back unsaved",
            f.batch.len()
        ),
    }
    let good = Message::new_at(alice, bob, "this one is fine", at(31))?;
    let bad = Message {
        id: None,
        from: alice,
        to: UserId(99),
        body: "to nobody".into(),
        timestamp: at(32),
    };
    match store.save_batch(vec![good, bad]) {
        Ok(_) => println!("batch with a bad row: saved (should not happen)"),
        Err(f) => println!(
            "batch with a bad row: refused, {} messages handed back unsaved",
            f.batch.len()
        ),
    }
    println!(
        "rows in database: {} (nothing half-saved)",
        store.count_messages()?
    );
    match store.user("dave") {
        Ok(u) => println!("lookup dave: {u}"),
        Err(e) => println!("lookup dave: error: {e}"),
    }
    Ok(())
}

async fn step3_simulated_users(o: &Options) -> Result<Store, Box<dyn std::error::Error>> {
    println!("== Step 3: two simulated users chatting at the same time ==");
    let store = Store::open(&o.db)?;
    let alice = store.ensure_user_at("alice", at(0))?.id;
    let bob = store.ensure_user_at("bob", at(0))?.id;
    let carol = store.ensure_user_at("carol", at(0))?.id;

    // The store moves into the session; `store` cannot be used here any more.
    let session = Session::open(store, alice, bob, SAVE_EVERY)?;
    println!("chat between alice and bob, saving every {SAVE_EVERY} messages");
    println!("loaded {} earlier messages", session.loaded().len());

    // A channel with room for 256 events. `tx` is the sending end; every user
    // task gets its own clone. The session task owns the only receiving end.
    let (tx, rx) = mpsc::channel(256);
    let started = Instant::now();
    let session_task = tokio::spawn(session.run(rx));

    let pause = Duration::from_millis(o.pause_ms);
    let from_bob = o.messages / 2;
    let from_alice = o.messages - from_bob;
    println!("alice sends {from_alice}, bob sends {from_bob}, at the same time");
    let alice_task = tokio::spawn(run_user(alice, bob, from_alice, 0, pause, tx.clone()));
    let bob_task = tokio::spawn(run_user(bob, alice, from_bob, 6, pause, tx.clone()));

    tx.send(ChatEvent::message(Message::new(carol, bob, "can I join?")?))
        .await?;
    println!("carol tries to post into this chat (she is not part of it)");

    let sent = alice_task.await? + bob_task.await?; // wait for both users to finish
    println!("both users finished: {sent} messages sent");

    let (reply, answer) = oneshot::channel();
    tx.send(ChatEvent::History(reply)).await?;
    let history = answer.await??;
    println!(
        "before close: {} messages ({} saved, {} still pending)",
        history.saved.len() + history.unsaved.len(),
        history.saved.len(),
        history.unsaved.len()
    );

    for (title, viewer, search) in [
        (
            "alice views: keyword \"rust\"",
            alice,
            Search::Keyword("rust".into()),
        ),
        ("alice views: messages from bob", alice, Search::User(bob)),
    ] {
        let (reply, answer) = oneshot::channel();
        tx.send(ChatEvent::Search {
            viewer,
            search,
            reply,
        })
        .await?;
        let found = answer.await??;
        println!(
            "search {title}: {} found (saved and pending)",
            found.saved.len() + found.unsaved.len()
        );
    }

    let (reply, answer) = oneshot::channel();
    tx.send(ChatEvent::Close(reply)).await?;
    let report = answer.await?;
    let store = session_task.await?; // the session hands the store back when it ends
    let elapsed = started.elapsed();
    println!(
        "closed: saved {} messages in batches {}, rejected {}, save errors {}",
        report.saved,
        batch_text(&report.batches),
        report.rejected,
        report.save_errors
    );
    println!(
        "database: {} rows, alice sent {}, bob sent {}",
        store.count_messages()?,
        store.search(bob, &Search::User(alice))?.len(),
        store.search(alice, &Search::User(bob))?.len()
    );
    eprintln!(
        "time: {:.1} ms for {} messages ({:.0} messages/second)",
        elapsed.as_secs_f64() * 1000.0,
        report.saved,
        report.saved as f64 / elapsed.as_secs_f64()
    );

    println!("== Step 3: reopen the same chat ==");
    let session = Session::open(store, alice, bob, SAVE_EVERY)?;
    println!("loaded {} earlier messages", session.loaded().len());
    let (tx, rx) = mpsc::channel(256);
    let session_task = tokio::spawn(session.run(rx));
    run_user(bob, alice, 3, 0, Duration::ZERO, tx.clone()).await;
    let (reply, answer) = oneshot::channel();
    tx.send(ChatEvent::Close(reply)).await?;
    let report = answer.await?;
    let store = session_task.await?;
    println!(
        "bob sent 3 more; closed: saved {} messages in batches {}",
        report.saved,
        batch_text(&report.batches)
    );
    println!(
        "database: {} rows (earlier messages were not saved again)",
        store.count_messages()?
    );
    Ok(store) // hand the store on to the next part of the demo
}

/// Extra (Day 1 Report, Appendix): alice saves keywords and reruns them later.
async fn extra_saved_keywords(store: Store) -> Result<(), Box<dyn std::error::Error>> {
    println!("== Extra: saved keywords ==");
    let alice = store.user("alice")?.id;
    let bob = store.user("bob")?.id;
    let session = Session::open(store, alice, bob, SAVE_EVERY)?;
    let (tx, rx) = mpsc::channel(256);
    let session_task = tokio::spawn(session.run(rx));

    for word in ["rust", "Lunch", "RUST", "   "] {
        let (reply, answer) = oneshot::channel();
        tx.send(ChatEvent::SaveKeyword {
            user: alice,
            keyword: word.to_string(),
            reply,
        })
        .await?;
        // `match` on the SaveOutcome enum: both outcomes (and the error) must be handled.
        match answer.await? {
            Ok(SaveOutcome::Added(k)) => {
                println!("alice saves \"{word}\": saved as \"{}\"", k.keyword)
            }
            Ok(SaveOutcome::AlreadySaved(k)) => println!(
                "alice saves \"{word}\": \"{}\" was already saved",
                k.keyword
            ),
            Err(e) => println!("alice saves \"{word}\": error: {e}"),
        }
    }

    // Two new messages from bob, still in the pending list (not saved yet).
    tx.send(ChatEvent::message(Message::new(
        bob,
        alice,
        "lunch at noon?",
    )?))
    .await?;
    tx.send(ChatEvent::message(Message::new(
        bob,
        alice,
        "the rust tests pass now",
    )?))
    .await?;
    println!("bob sends 2 new messages (still pending)");

    let rerun = |label: &'static str| {
        let tx = tx.clone();
        async move {
            let (reply, answer) = oneshot::channel();
            tx.send(ChatEvent::RunSavedKeywords {
                viewer: alice,
                reply,
            })
            .await?;
            println!("{label}");
            for k in answer.await?? {
                println!(
                    "  \"{}\": {} found ({} saved, {} pending)",
                    k.keyword.keyword,
                    k.saved.len() + k.unsaved.len(),
                    k.saved.len(),
                    k.unsaved.len()
                );
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        }
    };
    rerun("alice reruns her saved keywords:").await?;

    let (reply, answer) = oneshot::channel();
    tx.send(ChatEvent::ForgetKeyword {
        user: alice,
        keyword: "lunch".into(),
        reply,
    })
    .await?;
    println!("alice forgets \"lunch\": removed {}", answer.await??);
    rerun("alice reruns her saved keywords again:").await?;

    let (reply, answer) = oneshot::channel();
    tx.send(ChatEvent::Close(reply)).await?;
    answer.await?;
    let store = session_task.await?;
    let kept: Vec<String> = store
        .keywords(alice)?
        .into_iter()
        .map(|k| k.keyword)
        .collect();
    println!(
        "after close, alice's keywords stay in the database: [{}]",
        kept.join(", ")
    );
    Ok(())
}
