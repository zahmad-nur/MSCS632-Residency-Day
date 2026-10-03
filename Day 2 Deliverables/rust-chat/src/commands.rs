//! The commands typed at the prompt, and the parser that reads them.
//!
//! Parsing is kept apart from running so it can be tested on its own. Every
//! command is one case of the `Command` enum: the prompt's `match` must handle
//! all of them, so a new command cannot be forgotten.
//! (Go: go-chat/chat/commands.go uses a struct with a Kind field.)

/// The text printed by `help`. Identical in the Go version.
pub const HELP: &str = "\
commands:
  send <from> <to> <text>             send a message
  history                             the whole conversation (saved and pending)
  search user <name>                  messages <name> sent in this chat
  search keyword <word>               messages containing <word> (capitals ignored)
  search user <name> keyword <word>   both filters at once
  keyword save <user> <word>          save a keyword for <user>
  keyword forget <user> <word>        remove one of <user>'s saved keywords
  keywords <user>                     rerun <user>'s saved keywords
  simulate <count>                    the two users send <count> messages in the background
  help                                show this list
  quit                                save everything and exit";

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Send {
        from: String,
        to: String,
        text: String,
    },
    History,
    Search {
        user: Option<String>,
        keyword: Option<String>,
    },
    KeywordSave {
        user: String,
        keyword: String,
    },
    KeywordForget {
        user: String,
        keyword: String,
    },
    Keywords {
        user: String,
    },
    Simulate {
        count: usize,
    },
    Help,
    Quit,
}

const SEARCH_USAGE: &str =
    "usage: search user <name> | search keyword <word> | search user <name> keyword <word>";
const KEYWORD_USAGE: &str = "usage: keyword save <user> <word> | keyword forget <user> <word>";

/// Reads one typed line. `Ok(None)` for a blank line, `Err` with a usage
/// message for anything that is not a valid command.
pub fn parse(line: &str) -> Result<Option<Command>, String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    // Slice patterns: match on the shape of the list of words.
    let command = match words.as_slice() {
        [] => return Ok(None),
        ["send", from, to, text @ ..] if !text.is_empty() => Command::Send {
            from: from.to_string(),
            to: to.to_string(),
            text: text.join(" "),
        },
        ["send", ..] => return Err("usage: send <from> <to> <text>".to_string()),
        ["history"] => Command::History,
        ["search", "user", name] => Command::Search {
            user: Some(name.to_string()),
            keyword: None,
        },
        ["search", "keyword", word @ ..] if !word.is_empty() => Command::Search {
            user: None,
            keyword: Some(word.join(" ")),
        },
        ["search", "user", name, "keyword", word @ ..] if !word.is_empty() => Command::Search {
            user: Some(name.to_string()),
            keyword: Some(word.join(" ")),
        },
        ["search", ..] => return Err(SEARCH_USAGE.to_string()),
        ["keyword", "save", user, word @ ..] if !word.is_empty() => Command::KeywordSave {
            user: user.to_string(),
            keyword: word.join(" "),
        },
        ["keyword", "forget", user, word @ ..] if !word.is_empty() => Command::KeywordForget {
            user: user.to_string(),
            keyword: word.join(" "),
        },
        ["keyword", ..] => return Err(KEYWORD_USAGE.to_string()),
        ["keywords", user] => Command::Keywords {
            user: user.to_string(),
        },
        ["keywords", ..] => return Err("usage: keywords <user>".to_string()),
        ["simulate", count] => match count.parse::<usize>() {
            Ok(n) if n > 0 => Command::Simulate { count: n },
            _ => return Err("usage: simulate <count> (a number above 0)".to_string()),
        },
        ["simulate", ..] => return Err("usage: simulate <count> (a number above 0)".to_string()),
        ["help"] => Command::Help,
        ["quit"] | ["exit"] => Command::Quit,
        [other, ..] => return Err(format!("unknown command: {other} (type help)")),
    };
    Ok(Some(command))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(line: &str) -> Command {
        parse(line).unwrap().unwrap()
    }

    #[test]
    fn parses_every_command() {
        assert_eq!(
            ok("send alice bob hello  there"),
            Command::Send {
                from: "alice".into(),
                to: "bob".into(),
                text: "hello there".into()
            }
        );
        assert_eq!(ok("history"), Command::History);
        assert_eq!(
            ok("search user bob"),
            Command::Search {
                user: Some("bob".into()),
                keyword: None
            }
        );
        assert_eq!(
            ok("search keyword rust part"),
            Command::Search {
                user: None,
                keyword: Some("rust part".into())
            }
        );
        assert_eq!(
            ok("search user bob keyword rust"),
            Command::Search {
                user: Some("bob".into()),
                keyword: Some("rust".into())
            }
        );
        assert_eq!(
            ok("keyword save alice Rust"),
            Command::KeywordSave {
                user: "alice".into(),
                keyword: "Rust".into()
            }
        );
        assert_eq!(
            ok("keyword forget alice rust"),
            Command::KeywordForget {
                user: "alice".into(),
                keyword: "rust".into()
            }
        );
        assert_eq!(
            ok("keywords alice"),
            Command::Keywords {
                user: "alice".into()
            }
        );
        assert_eq!(ok("simulate 20"), Command::Simulate { count: 20 });
        assert_eq!(ok("  help "), Command::Help);
        assert_eq!(ok("quit"), Command::Quit);
        assert_eq!(ok("exit"), Command::Quit);
        assert_eq!(parse("   ").unwrap(), None);
    }

    #[test]
    fn explains_mistakes() {
        assert_eq!(
            parse("send alice bob").unwrap_err(),
            "usage: send <from> <to> <text>"
        );
        assert!(parse("search").unwrap_err().starts_with("usage: search"));
        assert!(parse("search keyword")
            .unwrap_err()
            .starts_with("usage: search"));
        assert!(parse("keyword save alice")
            .unwrap_err()
            .starts_with("usage: keyword"));
        assert!(parse("simulate lots")
            .unwrap_err()
            .starts_with("usage: simulate"));
        assert!(parse("simulate 0")
            .unwrap_err()
            .starts_with("usage: simulate"));
        assert_eq!(
            parse("bogus stuff").unwrap_err(),
            "unknown command: bogus (type help)"
        );
    }
}
