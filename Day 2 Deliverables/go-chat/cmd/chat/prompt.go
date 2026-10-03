// The interactive command prompt: the live demo.
//
// The prompt is one more goroutine talking to the session over the channel,
// just like the simulated users. It reads lines from the keyboard (or a piped
// file), turns each into a Command, and sends the matching Event. `simulate`
// starts the two simulated users in the background, so messages keep arriving
// while you type.
package main

import (
	"bufio"
	"fmt"
	"os"
	"strings"
	"sync"
	"time"

	"chatcompare/chat"
)

// promptOptions holds the settings for the prompt.
type promptOptions struct {
	db           string
	userA, userB string
	pauseMS      int  // pause between one simulated user's messages
	quiet        bool // do not print messages as they arrive
}

// prompt is what the prompt needs while it runs.
type prompt struct {
	events   chan chat.Event
	a, b     chat.User
	names    map[chat.UserID]string
	pause    time.Duration
	stop     chan struct{}  // closed on quit: tells simulated users to stop
	running  sync.WaitGroup // simulated users still sending
	nextLine int            // where the next simulation starts in chat.Lines
}

// runPrompt runs the prompt until `quit` or the end of the input.
func runPrompt(o promptOptions) error {
	if o.userA == o.userB {
		return fmt.Errorf("the two chat users must be different")
	}
	store, err := chat.OpenStore(o.db)
	if err != nil {
		return err
	}
	defer store.Close()
	a, err := store.EnsureUser(o.userA)
	if err != nil {
		return err
	}
	b, err := store.EnsureUser(o.userB)
	if err != nil {
		return err
	}
	names, err := store.Names()
	if err != nil {
		return err
	}

	session, err := chat.OpenSession(store, a.ID, b.ID, saveEvery)
	if err != nil {
		return err
	}
	if !o.quiet {
		session.Echo = names
	}
	fmt.Printf("chat between %s and %s (database %s, saving every %d messages)\n", a.Username, b.Username, o.db, saveEvery)
	fmt.Printf("loaded %d earlier messages\n", len(session.Loaded()))
	fmt.Println("type 'help' for commands")

	p := &prompt{
		events: make(chan chat.Event, 256),
		a:      a, b: b, names: names,
		pause: time.Duration(o.pauseMS) * time.Millisecond,
		stop:  make(chan struct{}),
	}
	go session.Run(p.events)

	// Show "> " only when a person is typing, not when input is piped in.
	info, _ := os.Stdin.Stat()
	interactive := info != nil && info.Mode()&os.ModeCharDevice != 0
	input := bufio.NewScanner(os.Stdin)
	for {
		if interactive {
			fmt.Print("> ")
		}
		// Scan blocks this goroutine only; the session and the simulated
		// users keep running in theirs.
		if !input.Scan() {
			break // end of input works like quit
		}
		cmd, err := chat.ParseCommand(input.Text())
		if err != nil {
			fmt.Println(err)
			continue
		}
		if !p.run(cmd) {
			break
		}
	}

	// Stop simulated users that are still sending. A goroutine cannot be
	// stopped from outside, so we close `stop` and each one checks it.
	close(p.stop)
	p.running.Wait()
	done := make(chan chat.SessionReport, 1)
	p.events <- chat.Event{Kind: chat.EventClose, Done: done}
	report := <-done
	fmt.Printf("closed: saved %d messages in batches %s (rejected %d, save errors %d)\n",
		report.Saved, batchText(report.Batches), report.Rejected, report.SaveErrors)
	n, err := store.CountMessages()
	if err != nil {
		return err
	}
	fmt.Printf("database %s now holds %d messages\n", o.db, n)
	return nil
}

// run runs one command and returns false for quit.
func (p *prompt) run(cmd chat.Command) bool {
	switch cmd.Kind {
	case chat.CmdNone:
	case chat.CmdSend:
		p.send(cmd.From, cmd.To, cmd.Text)
	case chat.CmdHistory:
		reply := make(chan chat.Listing, 1)
		p.events <- chat.Event{Kind: chat.EventHistory, Listing: reply}
		p.printListing(<-reply, "messages", false)
	case chat.CmdSearch:
		p.search(cmd.User, cmd.Keyword)
	case chat.CmdKeywordSave:
		user, ok := p.participant(cmd.User)
		if !ok {
			break
		}
		reply := make(chan chat.KeywordResult, 1)
		p.events <- chat.Event{Kind: chat.EventSaveKeyword, User: user.ID, Keyword: cmd.Keyword, KeywordReply: reply}
		switch r := <-reply; {
		case r.Err != nil:
			fmt.Println("error:", r.Err)
		case r.Added:
			fmt.Printf("saved keyword %q for %s\n", r.Saved.Keyword, user.Username)
		default:
			fmt.Printf("%s already saved %q\n", user.Username, r.Saved.Keyword)
		}
	case chat.CmdKeywordForget:
		user, ok := p.participant(cmd.User)
		if !ok {
			break
		}
		reply := make(chan chat.KeywordResult, 1)
		p.events <- chat.Event{Kind: chat.EventForgetKeyword, User: user.ID, Keyword: cmd.Keyword, KeywordReply: reply}
		word := strings.ToLower(strings.TrimSpace(cmd.Keyword))
		switch r := <-reply; {
		case r.Err != nil:
			fmt.Println("error:", r.Err)
		case r.Removed:
			fmt.Printf("removed keyword %q for %s\n", word, user.Username)
		default:
			fmt.Printf("%s has not saved %q\n", user.Username, word)
		}
	case chat.CmdKeywords:
		p.keywords(cmd.User)
	case chat.CmdSimulate:
		p.simulate(cmd.Count)
	case chat.CmdHelp:
		fmt.Println(chat.Help)
	case chat.CmdQuit:
		return false
	default:
		// The compiler does not check that every kind is handled, so say so
		// out loud instead of doing nothing.
		fmt.Printf("error: command kind %d is not handled\n", cmd.Kind)
	}
	return true
}

// participant finds one of the two chat users by name, or prints why not.
func (p *prompt) participant(name string) (chat.User, bool) {
	for _, u := range []chat.User{p.a, p.b} {
		if u.Username == name {
			return u, true
		}
	}
	fmt.Printf("error: %s is not in this chat (the users are %s and %s)\n", name, p.a.Username, p.b.Username)
	return chat.User{}, false
}

// other is the other person in the chat.
func (p *prompt) other(id chat.UserID) chat.UserID {
	if id == p.a.ID {
		return p.b.ID
	}
	return p.a.ID
}

func (p *prompt) send(fromName, toName, text string) {
	from, ok := p.participant(fromName)
	if !ok {
		return
	}
	to, ok := p.participant(toName)
	if !ok {
		return
	}
	m, err := chat.NewMessage(from.ID, to.ID, text)
	if err != nil {
		fmt.Println("error:", err)
		return
	}
	// Ask for a reply, so the session has handled (and printed) the message
	// before the prompt reads the next command.
	ack := make(chan bool, 1)
	p.events <- chat.Event{Kind: chat.EventSend, Msg: m, Ack: ack}
	if !<-ack {
		fmt.Println("error: message refused")
	}
}

func (p *prompt) search(userName, keyword string) {
	// The viewer: with a user filter, the other person in the chat (the
	// messages that user sent to them); otherwise the first chat user.
	viewer := p.a.ID
	q := chat.Search{Kind: chat.SearchKeyword, Keyword: keyword}
	if userName != "" {
		u, ok := p.participant(userName)
		if !ok {
			return
		}
		viewer = p.other(u.ID)
		q = chat.Search{Kind: chat.SearchUser, User: u.ID}
		if keyword != "" {
			q = chat.Search{Kind: chat.SearchUserAndKeyword, User: u.ID, Keyword: keyword}
		}
	}
	reply := make(chan chat.Listing, 1)
	p.events <- chat.Event{Kind: chat.EventSearch, Viewer: viewer, Query: q, Listing: reply}
	p.printListing(<-reply, "found", q.NewestFirst())
}

func (p *prompt) keywords(userName string) {
	user, ok := p.participant(userName)
	if !ok {
		return
	}
	reply := make(chan chat.KeywordResult, 1)
	p.events <- chat.Event{Kind: chat.EventRunSavedKeywords, Viewer: user.ID, KeywordReply: reply}
	r := <-reply
	switch {
	case r.Err != nil:
		fmt.Println("error:", r.Err)
	case len(r.Matches) == 0:
		fmt.Printf("%s has no saved keywords\n", user.Username)
	default:
		for _, k := range r.Matches {
			fmt.Printf("%q: %d found (%d saved, %d pending)\n",
				k.Keyword.Keyword, len(k.Saved)+len(k.Unsaved), len(k.Saved), len(k.Unsaved))
			for _, m := range inOrder(k.Saved, k.Unsaved, true) { // a keyword search: newest first
				fmt.Println("  " + line(m, p.names))
			}
		}
	}
}

// simulate starts the two simulated users as goroutines and returns at once.
func (p *prompt) simulate(count int) {
	fromB := count / 2
	fromA := count - fromB
	fmt.Printf("simulating: %s sends %d and %s sends %d messages in the background\n",
		p.a.Username, fromA, p.b.Username, fromB)
	start := p.nextLine
	p.nextLine += count
	p.running.Add(2)
	go func() {
		defer p.running.Done()
		chat.RunUserUntil(p.stop, p.a.ID, p.b.ID, fromA, start, p.pause, p.events)
	}()
	go func() {
		defer p.running.Done()
		chat.RunUserUntil(p.stop, p.b.ID, p.a.ID, fromB, start+6, p.pause, p.events)
	}()
}

func (p *prompt) printListing(l chat.Listing, noun string, newestFirst bool) {
	if l.Err != nil {
		fmt.Println("error:", l.Err)
		return
	}
	for _, m := range inOrder(l.Saved, l.Unsaved, newestFirst) {
		fmt.Println("  " + line(m, p.names))
	}
	fmt.Printf("(%d %s: %d saved, %d pending)\n", len(l.Saved)+len(l.Unsaved), noun, len(l.Saved), len(l.Unsaved))
}

// inOrder joins saved and pending results into one list. Pending messages are
// always newer than saved ones, so oldest first puts them last and newest
// first puts them first. Both lists already come in the requested order.
func inOrder(saved, unsaved []chat.Message, newestFirst bool) []chat.Message {
	out := make([]chat.Message, 0, len(saved)+len(unsaved))
	if newestFirst {
		return append(append(out, unsaved...), saved...)
	}
	return append(append(out, saved...), unsaved...)
}
