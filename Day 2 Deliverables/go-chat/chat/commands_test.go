package chat

import (
	"strings"
	"testing"
)

func TestParsesEveryCommand(t *testing.T) {
	cases := []struct {
		line string
		want Command
	}{
		{"send alice bob hello  there", Command{Kind: CmdSend, From: "alice", To: "bob", Text: "hello there"}},
		{"history", Command{Kind: CmdHistory}},
		{"search user bob", Command{Kind: CmdSearch, User: "bob"}},
		{"search keyword rust part", Command{Kind: CmdSearch, Keyword: "rust part"}},
		{"search user bob keyword rust", Command{Kind: CmdSearch, User: "bob", Keyword: "rust"}},
		{"keyword save alice Rust", Command{Kind: CmdKeywordSave, User: "alice", Keyword: "Rust"}},
		{"keyword forget alice rust", Command{Kind: CmdKeywordForget, User: "alice", Keyword: "rust"}},
		{"keywords alice", Command{Kind: CmdKeywords, User: "alice"}},
		{"simulate 20", Command{Kind: CmdSimulate, Count: 20}},
		{"  help ", Command{Kind: CmdHelp}},
		{"quit", Command{Kind: CmdQuit}},
		{"exit", Command{Kind: CmdQuit}},
		{"   ", Command{Kind: CmdNone}},
	}
	for _, c := range cases {
		got, err := ParseCommand(c.line)
		if err != nil || got != c.want {
			t.Errorf("%q: got %+v, %v; want %+v", c.line, got, err, c.want)
		}
	}
}

func TestExplainsMistakes(t *testing.T) {
	cases := []struct{ line, prefix string }{
		{"send alice bob", "usage: send <from> <to> <text>"},
		{"search", "usage: search"},
		{"search keyword", "usage: search"},
		{"keyword save alice", "usage: keyword"},
		{"simulate lots", "usage: simulate"},
		{"simulate 0", "usage: simulate"},
		{"bogus stuff", "unknown command: bogus (type help)"},
	}
	for _, c := range cases {
		_, err := ParseCommand(c.line)
		if err == nil || !strings.HasPrefix(err.Error(), c.prefix) {
			t.Errorf("%q: got %v, want an error starting %q", c.line, err, c.prefix)
		}
	}
}
