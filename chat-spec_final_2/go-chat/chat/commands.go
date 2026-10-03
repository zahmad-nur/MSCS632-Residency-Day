package chat

// This file: the commands typed at the prompt, and the parser that reads them.
//
// Parsing is kept apart from running so it can be tested on its own. Go has no
// enums, so a Command is a struct with a Kind plus the fields any kind might
// need. (Rust: rust-chat/src/commands.rs uses an enum with one case per command.)

import (
	"errors"
	"fmt"
	"strconv"
	"strings"
)

// Help is the text printed by `help`. Identical in the Rust version.
const Help = `commands:
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
  quit                                save everything and exit`

// CommandKind says which command was typed.
type CommandKind int

const (
	CmdNone CommandKind = iota // a blank line
	CmdSend
	CmdHistory
	CmdSearch
	CmdKeywordSave
	CmdKeywordForget
	CmdKeywords
	CmdSimulate
	CmdHelp
	CmdQuit
)

// Command is one parsed line. Each kind uses only some of the fields.
type Command struct {
	Kind     CommandKind
	From, To string // CmdSend
	Text     string // CmdSend
	User     string // CmdSearch (may be empty), CmdKeywordSave, CmdKeywordForget, CmdKeywords
	Keyword  string // CmdSearch (may be empty), CmdKeywordSave, CmdKeywordForget
	Count    int    // CmdSimulate
}

const (
	searchUsage  = "usage: search user <name> | search keyword <word> | search user <name> keyword <word>"
	keywordUsage = "usage: keyword save <user> <word> | keyword forget <user> <word>"
)

// ParseCommand reads one typed line. A blank line gives Kind CmdNone; anything
// that is not a valid command gives an error holding a usage message.
func ParseCommand(line string) (Command, error) {
	w := strings.Fields(line)
	if len(w) == 0 {
		return Command{Kind: CmdNone}, nil
	}
	rest := func(from int) string { return strings.Join(w[from:], " ") }
	switch w[0] {
	case "send":
		if len(w) < 4 {
			return Command{}, errors.New("usage: send <from> <to> <text>")
		}
		return Command{Kind: CmdSend, From: w[1], To: w[2], Text: rest(3)}, nil
	case "history":
		if len(w) == 1 {
			return Command{Kind: CmdHistory}, nil
		}
	case "search":
		switch {
		case len(w) == 3 && w[1] == "user":
			return Command{Kind: CmdSearch, User: w[2]}, nil
		case len(w) >= 3 && w[1] == "keyword":
			return Command{Kind: CmdSearch, Keyword: rest(2)}, nil
		case len(w) >= 5 && w[1] == "user" && w[3] == "keyword":
			return Command{Kind: CmdSearch, User: w[2], Keyword: rest(4)}, nil
		}
		return Command{}, errors.New(searchUsage)
	case "keyword":
		if len(w) >= 4 && w[1] == "save" {
			return Command{Kind: CmdKeywordSave, User: w[2], Keyword: rest(3)}, nil
		}
		if len(w) >= 4 && w[1] == "forget" {
			return Command{Kind: CmdKeywordForget, User: w[2], Keyword: rest(3)}, nil
		}
		return Command{}, errors.New(keywordUsage)
	case "keywords":
		if len(w) == 2 {
			return Command{Kind: CmdKeywords, User: w[1]}, nil
		}
		return Command{}, errors.New("usage: keywords <user>")
	case "simulate":
		if len(w) == 2 {
			if n, err := strconv.Atoi(w[1]); err == nil && n > 0 {
				return Command{Kind: CmdSimulate, Count: n}, nil
			}
		}
		return Command{}, errors.New("usage: simulate <count> (a number above 0)")
	case "help":
		if len(w) == 1 {
			return Command{Kind: CmdHelp}, nil
		}
	case "quit", "exit":
		if len(w) == 1 {
			return Command{Kind: CmdQuit}, nil
		}
	}
	return Command{}, fmt.Errorf("unknown command: %s (type help)", w[0])
}
