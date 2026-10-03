// The scripted demo (run with --demo). It shows every part of the app in a
// fixed order with fixed times, so it prints the same text on every run, and
// the Rust demo prints exactly the same text (scripts/compare.sh checks this).
// Timings go to stderr, so they are not part of that check.
//
// Step 1: users and messages, with the spec's rules.
// Step 2: saving to SQLite, and filtering and search by user or keyword.
// Step 3: two simulated users chatting at the same time (goroutines).
// Extra: saved keywords (Day 1 Report, Appendix).
package main

import (
	"fmt"
	"os"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"chatcompare/chat"
)

func at(sec int) time.Time { return time.Date(2026, 10, 3, 9, 0, sec, 0, time.UTC) }

// line prints one message with usernames instead of IDs.
func line(m chat.Message, names map[chat.UserID]string) string {
	name := func(id chat.UserID) string {
		if n, ok := names[id]; ok {
			return n
		}
		return "?"
	}
	status := "unsaved"
	if m.IsSaved() {
		status = fmt.Sprintf("id %d", m.ID)
	}
	return fmt.Sprintf("[%s] %s -> %s: %s (%s)", m.Timestamp.UTC().Format(chat.TimeLayout), name(m.From), name(m.To), m.Body, status)
}

func printResults(title string, results []chat.Message, names map[chat.UserID]string) {
	fmt.Println(title)
	for _, m := range results {
		fmt.Println("  " + line(m, names))
	}
	fmt.Printf("  (%d found)\n", len(results))
}

// saveEvery: save the pending list to SQLite every this many messages (spec).
const saveEvery = 100

// options holds the settings for the scripted demo.
type options struct {
	messages, pauseMS int    // messages and pause between them in step 3
	db                string // database for step 3 (":memory:" for a temporary one)
}

// batchText prints batch sizes: all of them if there are a few, else a summary.
func batchText(batches []int) string {
	list := func(b []int) string {
		parts := make([]string, len(b))
		for i, n := range b {
			parts[i] = fmt.Sprint(n)
		}
		return strings.Join(parts, ", ")
	}
	if len(batches) <= 6 {
		return "[" + list(batches) + "]"
	}
	return fmt.Sprintf("[%s, ... %d] (%d batches)", list(batches[:3]), batches[len(batches)-1], len(batches))
}

// runDemo runs the whole scripted demo.
func runDemo(o options) error {
	step1UsersAndMessages()
	if err := step2StoreAndSearch(); err != nil {
		return err
	}
	store, err := step3SimulatedUsers(o)
	if err != nil {
		return err
	}
	defer store.Close()
	return extraSavedKeywords(store)
}

func step1UsersAndMessages() {
	fmt.Println("== Step 1: usernames ==")
	for _, name := range []string{"alice", "bob", "user_42", "al", "Alice", "a-b-c", strings.Repeat("x", 21)} {
		if err := chat.ValidateUsername(name); err != nil {
			fmt.Printf("%s: error: %v\n", name, err)
		} else {
			fmt.Printf("%s: ok\n", name)
		}
	}

	fmt.Println("== Step 1: users ==")
	alice, _ := chat.NewUser(1, "alice", at(0))
	bob, _ := chat.NewUser(2, "bob", at(0))
	fmt.Println(alice)
	fmt.Println(bob)

	fmt.Println("== Step 1: messages ==")
	attempts := []struct {
		label    string
		from, to chat.UserID
		body     string
	}{
		{"hey", alice.ID, bob.ID, "hey bob, are you around?"},
		{"reply", bob.ID, alice.ID, "yep, what's up"},
		{"to self", alice.ID, alice.ID, "talking to myself"},
		{"empty", alice.ID, bob.ID, ""},
		{"501 chars", alice.ID, bob.ID, strings.Repeat("x", 501)},
		{"500 accented", alice.ID, bob.ID, strings.Repeat("é", 500)},
	}
	for i, a := range attempts {
		m, err := chat.NewMessageAt(a.from, a.to, a.body, at(5+i))
		switch {
		case err != nil:
			fmt.Printf("%s: error: %v\n", a.label, err)
		case len(m.Body) <= 40:
			fmt.Printf("%s: ok %v\n", a.label, m)
		default:
			fmt.Printf("%s: ok (%d characters, %d bytes)\n", a.label, utf8.RuneCountInString(m.Body), len(m.Body))
		}
	}
}

func step2StoreAndSearch() error {
	fmt.Println("== Step 2: store ==")
	store, err := chat.OpenStore(":memory:")
	if err != nil {
		return err
	}
	defer store.Close()

	ids := make(map[string]chat.UserID)
	for _, name := range []string{"alice", "bob", "carol"} {
		u, err := store.EnsureUserAt(name, at(0))
		if err != nil {
			return err
		}
		ids[name] = u.ID
	}
	alice, bob, carol := ids["alice"], ids["bob"], ids["carol"]
	names, err := store.Names()
	if err != nil {
		return err
	}
	fmt.Printf("users: alice=%d bob=%d carol=%d\n", alice, bob, carol)

	var batch []chat.Message
	for i, b := range []struct {
		from, to chat.UserID
		body     string
	}{
		{alice, bob, "hey bob, are you around?"},
		{bob, alice, "yep, what's up"},
		{alice, bob, "did you finish the Report?"},
		{bob, alice, "report is done, 100% tested"},
		{carol, bob, "can you review my report?"},
	} {
		m, err := chat.NewMessageAt(b.from, b.to, b.body, at(10+i))
		if err != nil {
			return err
		}
		batch = append(batch, m)
	}
	// SaveBatch writes the new IDs straight into our slice (shared memory).
	if err := store.SaveBatch(batch); err != nil {
		return err
	}
	allSaved := true
	idText := make([]string, len(batch))
	for i, m := range batch {
		allSaved = allSaved && m.IsSaved()
		idText[i] = fmt.Sprint(m.ID)
	}
	fmt.Printf("saved %d messages in one transaction, all have IDs: %t, ids [%s]\n", len(batch), allSaved, strings.Join(idText, ", "))
	batch = nil // drop our reference; the garbage collector frees the copies later

	convo, err := store.Conversation(alice, bob)
	if err != nil {
		return err
	}
	printResults("conversation alice <-> bob (oldest first):", convo, names)

	fmt.Println("== Step 2: search saved messages (SQL) ==")
	searches := []struct {
		title  string
		viewer chat.UserID
		q      chat.Search
	}{
		{"bob views: messages from alice", bob, chat.Search{Kind: chat.SearchUser, User: alice}},
		{`alice views: keyword "REPORT" (newest first)`, alice, chat.Search{Kind: chat.SearchKeyword, Keyword: "REPORT"}},
		{`bob views: from alice AND keyword "report"`, bob, chat.Search{Kind: chat.SearchUserAndKeyword, User: alice, Keyword: "report"}},
		{`alice views: keyword "100%" (% matched literally)`, alice, chat.Search{Kind: chat.SearchKeyword, Keyword: "100%"}},
		{`carol views: keyword "report" (only carol's chats)`, carol, chat.Search{Kind: chat.SearchKeyword, Keyword: "report"}},
	}
	for _, s := range searches {
		found, err := store.Search(s.viewer, s.q)
		if err != nil {
			return err
		}
		printResults(s.title, found, names)
	}

	fmt.Println("== Step 2: search pending messages (in memory) ==")
	p1, _ := chat.NewMessageAt(alice, bob, "great, sending the report now", at(20))
	p2, _ := chat.NewMessageAt(bob, alice, "thanks", at(21))
	pending := []chat.Message{p1, p2}
	found := chat.SearchPending(pending, alice, chat.Search{Kind: chat.SearchKeyword, Keyword: "report"})
	printResults(`alice views: keyword "report" in pending`, found, names)

	fmt.Println("== Step 2: the database enforces the rules ==")
	// Built by hand to skip the Message checks, so only the database can stop it.
	toSelf := []chat.Message{{From: alice, To: alice, Body: "me", Timestamp: at(30)}}
	if err := store.SaveBatch(toSelf); err != nil {
		fmt.Printf("message to self: refused, %d message handed back unsaved\n", len(toSelf))
	} else {
		fmt.Println("message to self: saved (should not happen)")
	}
	good, _ := chat.NewMessageAt(alice, bob, "this one is fine", at(31))
	mixed := []chat.Message{good, {From: alice, To: 99, Body: "to nobody", Timestamp: at(32)}}
	if err := store.SaveBatch(mixed); err != nil {
		fmt.Printf("batch with a bad row: refused, %d messages handed back unsaved\n", len(mixed))
	} else {
		fmt.Println("batch with a bad row: saved (should not happen)")
	}
	n, err := store.CountMessages()
	if err != nil {
		return err
	}
	fmt.Printf("rows in database: %d (nothing half-saved)\n", n)
	if u, err := store.User("dave"); err != nil {
		fmt.Printf("lookup dave: error: %v\n", err)
	} else {
		fmt.Printf("lookup dave: %v\n", u)
	}
	return nil
}

func step3SimulatedUsers(o options) (*chat.Store, error) {
	fmt.Println("== Step 3: two simulated users chatting at the same time ==")
	store, err := chat.OpenStore(o.db)
	if err != nil {
		return nil, err
	}
	ids := make(map[string]chat.UserID)
	for _, name := range []string{"alice", "bob", "carol"} {
		u, err := store.EnsureUserAt(name, at(0))
		if err != nil {
			return nil, err
		}
		ids[name] = u.ID
	}
	alice, bob, carol := ids["alice"], ids["bob"], ids["carol"]

	// The session gets a pointer to the same store. Go has no ownership
	// transfer: main could still use `store` while the session runs, and only
	// our own discipline (and the race detector in tests) keeps us from it.
	session, err := chat.OpenSession(store, alice, bob, saveEvery)
	if err != nil {
		return nil, err
	}
	fmt.Printf("chat between alice and bob, saving every %d messages\n", saveEvery)
	fmt.Printf("loaded %d earlier messages\n", len(session.Loaded()))

	// A buffered channel with room for 256 events. Every user goroutine sends
	// on it; only the session goroutine receives.
	events := make(chan chat.Event, 256)
	started := time.Now()
	go session.Run(events)

	pause := time.Duration(o.pauseMS) * time.Millisecond
	fromBob := o.messages / 2
	fromAlice := o.messages - fromBob
	fmt.Printf("alice sends %d, bob sends %d, at the same time\n", fromAlice, fromBob)
	var wg sync.WaitGroup
	var sentAlice, sentBob int
	wg.Add(2)
	go func() { defer wg.Done(); sentAlice = chat.RunUser(alice, bob, fromAlice, 0, pause, events) }()
	go func() { defer wg.Done(); sentBob = chat.RunUser(bob, alice, fromBob, 6, pause, events) }()

	intruder, err := chat.NewMessage(carol, bob, "can I join?")
	if err != nil {
		return nil, err
	}
	events <- chat.Event{Kind: chat.EventSend, Msg: intruder}
	fmt.Println("carol tries to post into this chat (she is not part of it)")

	wg.Wait() // wait for both users to finish
	fmt.Printf("both users finished: %d messages sent\n", sentAlice+sentBob)

	reply := make(chan chat.Listing, 1)
	events <- chat.Event{Kind: chat.EventHistory, Listing: reply}
	history := <-reply
	if history.Err != nil {
		return nil, history.Err
	}
	fmt.Printf("before close: %d messages (%d saved, %d still pending)\n",
		len(history.Saved)+len(history.Unsaved), len(history.Saved), len(history.Unsaved))

	for _, q := range []struct {
		title  string
		viewer chat.UserID
		search chat.Search
	}{
		{`alice views: keyword "rust"`, alice, chat.Search{Kind: chat.SearchKeyword, Keyword: "rust"}},
		{"alice views: messages from bob", alice, chat.Search{Kind: chat.SearchUser, User: bob}},
	} {
		events <- chat.Event{Kind: chat.EventSearch, Viewer: q.viewer, Query: q.search, Listing: reply}
		found := <-reply
		if found.Err != nil {
			return nil, found.Err
		}
		fmt.Printf("search %s: %d found (saved and pending)\n", q.title, len(found.Saved)+len(found.Unsaved))
	}

	done := make(chan chat.SessionReport, 1)
	events <- chat.Event{Kind: chat.EventClose, Done: done}
	report := <-done
	elapsed := time.Since(started)
	fmt.Printf("closed: saved %d messages in batches %s, rejected %d, save errors %d\n",
		report.Saved, batchText(report.Batches), report.Rejected, report.SaveErrors)
	rows, err := store.CountMessages()
	if err != nil {
		return nil, err
	}
	fromA, err := store.Search(bob, chat.Search{Kind: chat.SearchUser, User: alice})
	if err != nil {
		return nil, err
	}
	fromB, err := store.Search(alice, chat.Search{Kind: chat.SearchUser, User: bob})
	if err != nil {
		return nil, err
	}
	fmt.Printf("database: %d rows, alice sent %d, bob sent %d\n", rows, len(fromA), len(fromB))
	fmt.Fprintf(os.Stderr, "time: %.1f ms for %d messages (%.0f messages/second)\n",
		float64(elapsed.Microseconds())/1000, report.Saved, float64(report.Saved)/elapsed.Seconds())

	fmt.Println("== Step 3: reopen the same chat ==")
	session, err = chat.OpenSession(store, alice, bob, saveEvery)
	if err != nil {
		return nil, err
	}
	fmt.Printf("loaded %d earlier messages\n", len(session.Loaded()))
	events = make(chan chat.Event, 256)
	go session.Run(events)
	chat.RunUser(bob, alice, 3, 0, 0, events)
	events <- chat.Event{Kind: chat.EventClose, Done: done}
	report = <-done
	fmt.Printf("bob sent 3 more; closed: saved %d messages in batches %s\n", report.Saved, batchText(report.Batches))
	rows, err = store.CountMessages()
	if err != nil {
		return nil, err
	}
	fmt.Printf("database: %d rows (earlier messages were not saved again)\n", rows)
	return store, nil // the caller closes it after the next part of the demo
}

// extraSavedKeywords: alice saves keywords and reruns them later (Day 1 Report, Appendix).
func extraSavedKeywords(store *chat.Store) error {
	fmt.Println("== Extra: saved keywords ==")
	a, err := store.User("alice")
	if err != nil {
		return err
	}
	b, err := store.User("bob")
	if err != nil {
		return err
	}
	alice, bob := a.ID, b.ID
	session, err := chat.OpenSession(store, alice, bob, saveEvery)
	if err != nil {
		return err
	}
	events := make(chan chat.Event, 256)
	go session.Run(events)

	reply := make(chan chat.KeywordResult, 1)
	for _, word := range []string{"rust", "Lunch", "RUST", "   "} {
		events <- chat.Event{Kind: chat.EventSaveKeyword, User: alice, Keyword: word, KeywordReply: reply}
		r := <-reply
		switch {
		case r.Err != nil:
			fmt.Printf("alice saves %q: error: %v\n", word, r.Err)
		case r.Added:
			fmt.Printf("alice saves %q: saved as %q\n", word, r.Saved.Keyword)
		default:
			fmt.Printf("alice saves %q: %q was already saved\n", word, r.Saved.Keyword)
		}
	}

	// Two new messages from bob, still in the pending list (not saved yet).
	for _, body := range []string{"lunch at noon?", "the rust tests pass now"} {
		m, err := chat.NewMessage(bob, alice, body)
		if err != nil {
			return err
		}
		events <- chat.Event{Kind: chat.EventSend, Msg: m}
	}
	fmt.Println("bob sends 2 new messages (still pending)")

	rerun := func(label string) error {
		events <- chat.Event{Kind: chat.EventRunSavedKeywords, Viewer: alice, KeywordReply: reply}
		r := <-reply
		if r.Err != nil {
			return r.Err
		}
		fmt.Println(label)
		for _, k := range r.Matches {
			fmt.Printf("  %q: %d found (%d saved, %d pending)\n",
				k.Keyword.Keyword, len(k.Saved)+len(k.Unsaved), len(k.Saved), len(k.Unsaved))
		}
		return nil
	}
	if err := rerun("alice reruns her saved keywords:"); err != nil {
		return err
	}

	events <- chat.Event{Kind: chat.EventForgetKeyword, User: alice, Keyword: "lunch", KeywordReply: reply}
	r := <-reply
	if r.Err != nil {
		return r.Err
	}
	fmt.Printf("alice forgets \"lunch\": removed %t\n", r.Removed)
	if err := rerun("alice reruns her saved keywords again:"); err != nil {
		return err
	}

	done := make(chan chat.SessionReport, 1)
	events <- chat.Event{Kind: chat.EventClose, Done: done}
	<-done
	kept, err := store.Keywords(alice)
	if err != nil {
		return err
	}
	words := make([]string, len(kept))
	for i, k := range kept {
		words[i] = k.Keyword
	}
	fmt.Printf("after close, alice's keywords stay in the database: [%s]\n", strings.Join(words, ", "))
	return nil
}
