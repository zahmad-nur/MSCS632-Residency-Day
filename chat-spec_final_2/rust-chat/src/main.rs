//! Chat application, Rust version.
//!
//! By default it starts the interactive command prompt (type `help`):
//!
//!   cargo run -- [--db chat.db] [--users alice,bob] [--pause-ms 300] [--quiet]
//!
//! With `--demo` it runs the scripted demo instead, which prints the same text
//! on every run (the Go version prints exactly the same text):
//!
//!   cargo run -- --demo [--messages 250] [--pause-ms 0] [--db FILE]

mod commands;
mod demo;
mod event;
mod keywords;
mod messages;
mod prompt;
mod search;
mod session;
mod simulator;
mod store;
mod users;

/// Which program to run, with its settings. An enum: exactly one of the two.
enum Mode {
    Prompt(prompt::Options),
    Demo(demo::Options),
}

fn parse_args() -> Result<Mode, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut demo, mut quiet) = (false, false);
    let (mut messages, mut pause_ms, mut db, mut users) = (None, None, None, None);
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        // Flags that take a value read the next word.
        let mut value = || {
            i += 1;
            args.get(i).cloned().ok_or(format!("{flag} needs a value"))
        };
        match flag {
            "--demo" => demo = true,
            "--quiet" => quiet = true,
            "--messages" => {
                messages = Some(value()?.parse::<usize>().map_err(|_| "bad --messages")?)
            }
            "--pause-ms" => pause_ms = Some(value()?.parse::<u64>().map_err(|_| "bad --pause-ms")?),
            "--db" => db = Some(value()?),
            "--users" => users = Some(value()?),
            other => return Err(format!("unknown option {other}")),
        }
        i += 1;
    }

    if demo {
        return Ok(Mode::Demo(demo::Options {
            messages: messages.unwrap_or(250),
            pause_ms: pause_ms.unwrap_or(0),
            db: db.unwrap_or_else(|| ":memory:".to_string()),
        }));
    }
    let users = users.unwrap_or_else(|| "alice,bob".to_string());
    let Some((a, b)) = users.split_once(',') else {
        return Err("--users needs two names, like alice,bob".to_string());
    };
    Ok(Mode::Prompt(prompt::Options {
        db: db.unwrap_or_else(|| "chat.db".to_string()),
        user_a: a.trim().to_string(),
        user_b: b.trim().to_string(),
        pause_ms: pause_ms.unwrap_or(300),
        quiet,
    }))
}

/// `#[tokio::main]` starts the async runtime (a pool of worker threads) and
/// runs `main` on it. Go needs nothing like this: goroutines are built in.
#[tokio::main]
async fn main() {
    let result = match parse_args() {
        Ok(Mode::Prompt(options)) => prompt::run(options).await,
        Ok(Mode::Demo(options)) => demo::run(&options).await,
        Err(e) => Err(e.into()),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
