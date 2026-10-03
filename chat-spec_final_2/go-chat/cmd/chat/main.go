// Command chat is the chat application, Go version.
//
// By default it starts the interactive command prompt (type `help`):
//
//	go run ./cmd/chat [--db chat.db] [--users alice,bob] [--pause-ms 300] [--quiet]
//
// With --demo it runs the scripted demo instead, which prints the same text on
// every run (the Rust version prints exactly the same text):
//
//	go run ./cmd/chat --demo [--messages 250] [--pause-ms 0] [--db FILE]
package main

import (
	"flag"
	"fmt"
	"os"
	"strings"
)

func main() {
	demo := flag.Bool("demo", false, "run the scripted demo instead of the prompt")
	quiet := flag.Bool("quiet", false, "prompt: do not print messages as they arrive")
	messages := flag.Int("messages", 250, "demo: messages the two users send in step 3")
	pauseMS := flag.Int("pause-ms", -1, "pause between one simulated user's messages (demo 0, prompt 300)")
	db := flag.String("db", "", "database file (demo: in memory, prompt: chat.db)")
	users := flag.String("users", "alice,bob", "prompt: the two chat users")
	flag.Parse()

	var err error
	if *demo {
		o := options{messages: *messages, pauseMS: orDefault(*pauseMS, 0), db: *db}
		if o.db == "" {
			o.db = ":memory:"
		}
		err = runDemo(o)
	} else {
		a, b, ok := strings.Cut(*users, ",")
		if !ok {
			err = fmt.Errorf("--users needs two names, like alice,bob")
		} else {
			o := promptOptions{db: *db, userA: strings.TrimSpace(a), userB: strings.TrimSpace(b),
				pauseMS: orDefault(*pauseMS, 300), quiet: *quiet}
			if o.db == "" {
				o.db = "chat.db"
			}
			err = runPrompt(o)
		}
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		os.Exit(1)
	}
}

// orDefault returns def when a number flag was left at its "not given" value (-1).
func orDefault(n, def int) int {
	if n < 0 {
		return def
	}
	return n
}
