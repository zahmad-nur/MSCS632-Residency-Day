//! The interactive command prompt: the live demo.
//!
//! The prompt is one more task talking to the session over the channel, just
//! like the simulated users. It reads lines from the keyboard (or a piped
//! file), turns each into a `Command`, and sends the matching `ChatEvent`.
//! `simulate` starts the two simulated users in the background, so messages
//! keep arriving while you type.

use crate::commands::{self, Command, HELP};
use crate::demo::{batch_text, line, SAVE_EVERY};
use crate::event::{ChatEvent, Listing};
use crate::keywords::SaveOutcome;
use crate::messages::Message;
use crate::search::Search;
use crate::session::Session;
use crate::simulator::run_user;
use crate::store::{Store, StoreError};
use crate::users::{User, UserId};
use std::collections::HashMap;
use std::error::Error;
use std::io::{IsTerminal, Write};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// Settings for the prompt.
pub struct Options {
    pub db: String,
    pub user_a: String,
    pub user_b: String,
    /// Pause between one simulated user's messages.
    pub pause_ms: u64,
    /// Do not print messages as they arrive.
    pub quiet: bool,
}

/// What the prompt needs while it runs.
struct Prompt {
    events: mpsc::Sender<ChatEvent>,
    a: User,
    b: User,
    names: HashMap<UserId, String>,
    pause: Duration,
    /// Running simulated users, so `quit` can stop them.
    simulators: Vec<JoinHandle<usize>>,
    /// Where the next simulation starts in the list of lines users say.
    next_line: usize,
}

type Res<T> = Result<T, Box<dyn Error>>;

/// Runs the prompt until `quit` or the end of the input.
pub async fn run(o: Options) -> Res<()> {
    if o.user_a == o.user_b {
        return Err("the two chat users must be different".into());
    }
    let store = Store::open(&o.db)?;
    let a = store.ensure_user(&o.user_a)?;
    let b = store.ensure_user(&o.user_b)?;
    let names = store.names()?;

    let mut session = Session::open(store, a.id, b.id, SAVE_EVERY)?;
    let loaded = session.loaded().len();
    if !o.quiet {
        session = session.echo_to_screen(names.clone());
    }
    println!(
        "chat between {} and {} (database {}, saving every {SAVE_EVERY} messages)",
        a.username, b.username, o.db
    );
    println!("loaded {loaded} earlier messages");
    println!("type 'help' for commands");

    let (events, rx) = mpsc::channel(256);
    let session_task = tokio::spawn(session.run(rx));
    let mut prompt = Prompt {
        events,
        a,
        b,
        names,
        pause: Duration::from_millis(o.pause_ms),
        simulators: Vec::new(),
        next_line: 0,
    };

    // Show "> " only when a person is typing, not when input is piped in.
    let interactive = std::io::stdin().is_terminal();
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    loop {
        if interactive {
            print!("> ");
            std::io::stdout().flush()?;
        }
        // `await` gives up the thread while waiting for a line, so the session
        // and the simulated users keep running.
        let Some(text) = input.next_line().await? else {
            break; // end of input works like quit
        };
        match commands::parse(&text) {
            Ok(None) => {}
            Err(usage) => println!("{usage}"),
            Ok(Some(command)) => {
                if !prompt.run(command).await? {
                    break;
                }
            }
        }
    }

    // Stop simulated users that are still sending. `abort` cancels a task
    // from outside; Go has no such thing and must ask a goroutine to stop.
    for task in prompt.simulators.drain(..) {
        task.abort();
    }
    let (reply, answer) = oneshot::channel();
    prompt.events.send(ChatEvent::Close(reply)).await?;
    let report = answer.await?;
    let store = session_task.await?;
    println!(
        "closed: saved {} messages in batches {} (rejected {}, save errors {})",
        report.saved,
        batch_text(&report.batches),
        report.rejected,
        report.save_errors
    );
    println!(
        "database {} now holds {} messages",
        o.db,
        store.count_messages()?
    );
    Ok(())
}

impl Prompt {
    /// Runs one command. Returns false for `quit`.
    async fn run(&mut self, command: Command) -> Res<bool> {
        // Every Command case must be handled here, or this will not compile.
        match command {
            Command::Send { from, to, text } => self.send(&from, &to, &text).await?,
            Command::History => {
                let (reply, answer) = oneshot::channel();
                self.events.send(ChatEvent::History(reply)).await?;
                self.print_listing(answer.await?, "messages", false);
            }
            Command::Search { user, keyword } => self.search(user, keyword).await?,
            Command::KeywordSave { user, keyword } => {
                let Some(user) = self.participant(&user) else {
                    return Ok(true);
                };
                let (reply, answer) = oneshot::channel();
                self.events
                    .send(ChatEvent::SaveKeyword {
                        user: user.id,
                        keyword,
                        reply,
                    })
                    .await?;
                match answer.await? {
                    Ok(SaveOutcome::Added(k)) => {
                        println!("saved keyword \"{}\" for {}", k.keyword, user.username)
                    }
                    Ok(SaveOutcome::AlreadySaved(k)) => {
                        println!("{} already saved \"{}\"", user.username, k.keyword)
                    }
                    Err(e) => println!("error: {e}"),
                }
            }
            Command::KeywordForget { user, keyword } => {
                let Some(user) = self.participant(&user) else {
                    return Ok(true);
                };
                let (reply, answer) = oneshot::channel();
                self.events
                    .send(ChatEvent::ForgetKeyword {
                        user: user.id,
                        keyword: keyword.clone(),
                        reply,
                    })
                    .await?;
                match answer.await? {
                    Ok(true) => println!(
                        "removed keyword \"{}\" for {}",
                        keyword.trim().to_lowercase(),
                        user.username
                    ),
                    Ok(false) => println!(
                        "{} has not saved \"{}\"",
                        user.username,
                        keyword.trim().to_lowercase()
                    ),
                    Err(e) => println!("error: {e}"),
                }
            }
            Command::Keywords { user } => self.keywords(&user).await?,
            Command::Simulate { count } => self.simulate(count),
            Command::Help => println!("{HELP}"),
            Command::Quit => return Ok(false),
        }
        Ok(true)
    }

    /// One of the two chat users by name, or prints why not.
    fn participant(&self, name: &str) -> Option<User> {
        let found = [&self.a, &self.b]
            .into_iter()
            .find(|u| u.username == name)
            .cloned();
        if found.is_none() {
            println!(
                "error: {name} is not in this chat (the users are {} and {})",
                self.a.username, self.b.username
            );
        }
        found
    }

    /// The other person in the chat.
    fn other(&self, id: UserId) -> UserId {
        if id == self.a.id {
            self.b.id
        } else {
            self.a.id
        }
    }

    async fn send(&mut self, from: &str, to: &str, text: &str) -> Res<()> {
        let Some(from) = self.participant(from) else {
            return Ok(());
        };
        let Some(to) = self.participant(to) else {
            return Ok(());
        };
        let message = match Message::new(from.id, to.id, text) {
            Ok(m) => m,
            Err(e) => {
                println!("error: {e}");
                return Ok(());
            }
        };
        // Ask for a reply, so the session has handled (and printed) the
        // message before the prompt reads the next command.
        let (ack, accepted) = oneshot::channel();
        self.events
            .send(ChatEvent::Send {
                message,
                ack: Some(ack),
            })
            .await?;
        if !accepted.await? {
            println!("error: message refused");
        }
        Ok(())
    }

    async fn search(&mut self, user: Option<String>, keyword: Option<String>) -> Res<()> {
        // The viewer: with a user filter, the other person in the chat (the
        // messages that user sent to them); otherwise the first chat user.
        let (viewer, search) = match (user, keyword) {
            (Some(name), keyword) => {
                let Some(u) = self.participant(&name) else {
                    return Ok(());
                };
                let search = match keyword {
                    Some(word) => Search::UserAndKeyword(u.id, word),
                    None => Search::User(u.id),
                };
                (self.other(u.id), search)
            }
            (None, Some(word)) => (self.a.id, Search::Keyword(word)),
            (None, None) => {
                println!("usage: search user <name> | search keyword <word>");
                return Ok(());
            }
        };
        let newest_first = search.newest_first();
        let (reply, answer) = oneshot::channel();
        self.events
            .send(ChatEvent::Search {
                viewer,
                search,
                reply,
            })
            .await?;
        self.print_listing(answer.await?, "found", newest_first);
        Ok(())
    }

    async fn keywords(&mut self, user: &str) -> Res<()> {
        let Some(user) = self.participant(user) else {
            return Ok(());
        };
        let (reply, answer) = oneshot::channel();
        self.events
            .send(ChatEvent::RunSavedKeywords {
                viewer: user.id,
                reply,
            })
            .await?;
        match answer.await? {
            Err(e) => println!("error: {e}"),
            Ok(list) if list.is_empty() => println!("{} has no saved keywords", user.username),
            Ok(list) => {
                for k in list {
                    println!(
                        "\"{}\": {} found ({} saved, {} pending)",
                        k.keyword.keyword,
                        k.saved.len() + k.unsaved.len(),
                        k.saved.len(),
                        k.unsaved.len()
                    );
                    // A keyword search: newest first.
                    for m in in_order(&k.saved, &k.unsaved, true) {
                        println!("  {}", line(m, &self.names));
                    }
                }
            }
        }
        Ok(())
    }

    /// Starts the two simulated users as background tasks and returns at once.
    fn simulate(&mut self, count: usize) {
        let from_b = count / 2;
        let from_a = count - from_b;
        println!(
            "simulating: {} sends {from_a} and {} sends {from_b} messages in the background",
            self.a.username, self.b.username
        );
        let start = self.next_line;
        self.next_line += count;
        // Each task gets its own clone of the channel's sending end.
        let a = tokio::spawn(run_user(
            self.a.id,
            self.b.id,
            from_a,
            start,
            self.pause,
            self.events.clone(),
        ));
        let b = tokio::spawn(run_user(
            self.b.id,
            self.a.id,
            from_b,
            start + 6,
            self.pause,
            self.events.clone(),
        ));
        self.simulators.retain(|t| !t.is_finished()); // forget finished ones
        self.simulators.extend([a, b]);
    }

    fn print_listing(&self, result: Result<Listing, StoreError>, noun: &str, newest_first: bool) {
        match result {
            Err(e) => println!("error: {e}"),
            Ok(l) => {
                for m in in_order(&l.saved, &l.unsaved, newest_first) {
                    println!("  {}", line(m, &self.names));
                }
                println!(
                    "({} {noun}: {} saved, {} pending)",
                    l.saved.len() + l.unsaved.len(),
                    l.saved.len(),
                    l.unsaved.len()
                );
            }
        }
    }
}

/// Saved and pending results as one list. Pending messages are always newer
/// than saved ones, so oldest first puts them last and newest first puts them
/// first. Both lists already come in the requested order.
fn in_order<'a>(
    saved: &'a [Message],
    unsaved: &'a [Message],
    newest_first: bool,
) -> Vec<&'a Message> {
    let (saved, unsaved) = (saved.iter(), unsaved.iter());
    if newest_first {
        unsaved.chain(saved).collect()
    } else {
        saved.chain(unsaved).collect()
    }
}
