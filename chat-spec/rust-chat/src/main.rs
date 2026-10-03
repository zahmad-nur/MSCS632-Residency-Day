//! Step 1 demo: builds users and messages with the spec's rules and prints
//! the results. The Go demo prints exactly the same text (scripts/compare.sh).

mod model;

use chrono::{DateTime, TimeZone, Utc};
use model::{Message, User, UserId};

fn at(sec: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 3, 9, 0, sec).unwrap()
}

fn main() {
    println!("== Usernames ==");
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
        match model::validate_username(name) {
            Ok(()) => println!("{name}: ok"),
            Err(e) => println!("{name}: error: {e}"),
        }
    }

    println!("== Users ==");
    let alice = User::new(UserId(1), "alice", at(0)).expect("valid user");
    let bob = User::new(UserId(2), "bob", at(0)).expect("valid user");
    println!("{alice}");
    println!("{bob}");

    println!("== Messages ==");
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
    let mut pending: Vec<Message> = Vec::new();
    for (i, (label, from, to, body)) in attempts.into_iter().enumerate() {
        match Message::new_at(from, to, &body, at(5 + i as u32)) {
            Ok(m) => {
                println!(
                    "{label}: ok ({} characters, {} bytes)",
                    m.body.chars().count(),
                    m.body.len()
                );
                pending.push(m); // `m` moves into the list
            }
            Err(e) => println!("{label}: error: {e}"),
        }
    }

    println!("== Pending (in memory, no IDs yet) ==");
    for m in pending.iter().take(2) {
        println!("{m}");
    }

    println!("== After saving (the database assigns IDs) ==");
    for (i, m) in pending.iter_mut().enumerate() {
        m.id = Some(i as i64 + 1);
    }
    for m in pending.iter().take(2) {
        println!("{m}");
    }
    println!(
        "({} messages, all saved: {})",
        pending.len(),
        pending.iter().all(Message::is_saved)
    );
}
