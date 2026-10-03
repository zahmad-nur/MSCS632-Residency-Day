package chat

import (
	"sync"
	"testing"
	"time"
)

// start opens a session and runs it in its own goroutine.
func start(t *testing.T, s *Store, a, b UserID) chan Event {
	t.Helper()
	session, err := OpenSession(s, a, b, 100)
	if err != nil {
		t.Fatal(err)
	}
	events := make(chan Event, 256)
	go session.Run(events)
	return events
}

func closeSession(events chan Event) SessionReport {
	done := make(chan SessionReport, 1)
	events <- Event{Kind: EventClose, Done: done}
	return <-done
}

// Run with `go test -race`: the race detector checks that no two goroutines
// touch the same memory without synchronization.
func TestTwoUsersAtOnceLoseNothing(t *testing.T) {
	s, alice, bob, _ := fixture(t)
	events := start(t, s, alice, bob)

	var wg sync.WaitGroup
	var sentA, sentB int
	wg.Add(2)
	go func() { defer wg.Done(); sentA = RunUser(alice, bob, 125, 0, 0, events) }()
	go func() { defer wg.Done(); sentB = RunUser(bob, alice, 125, 6, 0, events) }()
	wg.Wait()
	if sentA+sentB != 250 {
		t.Fatalf("sent %d, want 250", sentA+sentB)
	}

	report := closeSession(events)
	if len(report.Batches) != 3 || report.Batches[0] != 100 || report.Batches[1] != 100 || report.Batches[2] != 50 {
		t.Errorf("batches %v, want [100 100 50]", report.Batches)
	}
	if report.Accepted != 250 || report.Saved != 250 || report.Rejected != 0 {
		t.Errorf("report %+v", report)
	}
	if n, _ := s.CountMessages(); n != 250 {
		t.Errorf("rows %d, want 250", n)
	}
}

func TestMessagesFromOutsideTheChatAreRejected(t *testing.T) {
	s, alice, bob, carol := fixture(t)
	events := start(t, s, alice, bob)
	intruder, _ := NewMessage(carol, bob, "can I join?")
	hi, _ := NewMessage(alice, bob, "hi")
	events <- Event{Kind: EventSend, Msg: intruder}
	events <- Event{Kind: EventSend, Msg: hi}
	report := closeSession(events)
	if report.Accepted != 1 || report.Rejected != 1 || report.Saved != 1 {
		t.Errorf("report %+v", report)
	}
}

func TestHistoryAndSearchCoverSavedAndPending(t *testing.T) {
	s, alice, bob, _ := fixture(t)
	events := start(t, s, alice, bob)
	RunUser(alice, bob, 120, 0, 0, events)

	reply := make(chan Listing, 1)
	events <- Event{Kind: EventHistory, Listing: reply}
	l := <-reply
	if l.Err != nil || len(l.Saved) != 100 || len(l.Unsaved) != 20 {
		t.Errorf("history: saved %d, unsaved %d, err %v; want 100, 20", len(l.Saved), len(l.Unsaved), l.Err)
	}

	// Lines[2] is "did you finish the rust part?": it is message 3, 15, 27, ...
	events <- Event{Kind: EventSearch, Viewer: bob, Query: Search{Kind: SearchKeyword, Keyword: "RUST"}, Listing: reply}
	found := <-reply
	if found.Err != nil || len(found.Saved)+len(found.Unsaved) != 10 {
		t.Errorf("search: got %d+%d, err %v; want 10 in total", len(found.Saved), len(found.Unsaved), found.Err)
	}
	for _, m := range found.Unsaved {
		if m.IsSaved() {
			t.Errorf("pending result has an ID: %v", m)
		}
	}
}

func TestReopeningLoadsHistoryWithoutSavingItTwice(t *testing.T) {
	s, alice, bob, _ := fixture(t)
	events := start(t, s, alice, bob)
	RunUser(alice, bob, 5, 0, 0, events)
	closeSession(events)

	events = start(t, s, alice, bob) // the same chat again
	RunUser(bob, alice, 3, 0, 0, events)
	report := closeSession(events)
	if report.Loaded != 5 || report.Saved != 3 {
		t.Errorf("report %+v, want loaded 5, saved 3", report)
	}
	if n, _ := s.CountMessages(); n != 8 {
		t.Errorf("rows %d, want 8", n)
	}
}

// Day 1 Report, Table 5: a failed save loses nothing. The messages stay pending and
// the next save writes them.
func TestFailedSaveKeepsMessagesAndRetries(t *testing.T) {
	s, alice, bob, _ := fixture(t)
	session, err := OpenSession(s, alice, bob, 2) // save every 2 messages
	if err != nil {
		t.Fatal(err)
	}
	events := make(chan Event, 16)
	go session.Run(events)

	// Move the messages table away so the next save fails.
	if _, err := s.db.Exec(`ALTER TABLE messages RENAME TO messages_away`); err != nil {
		t.Fatal(err)
	}
	RunUser(alice, bob, 2, 0, 0, events) // pending reaches 2: this save fails

	// A History request is answered only after the two sends were handled,
	// so it tells us the failed save has happened.
	reply := make(chan Listing, 1)
	events <- Event{Kind: EventHistory, Listing: reply}
	if l := <-reply; len(l.Unsaved) != 2 {
		t.Fatalf("after the failed save: %d pending, want 2", len(l.Unsaved))
	}

	// Put the table back; closing retries the same two messages.
	if _, err := s.db.Exec(`ALTER TABLE messages_away RENAME TO messages`); err != nil {
		t.Fatal(err)
	}
	report := closeSession(events)
	if report.SaveErrors != 1 || report.Saved != 2 || len(report.Batches) != 1 || report.Batches[0] != 2 {
		t.Errorf("report %+v, want 1 save error, then 2 saved in one batch", report)
	}
	if n, _ := s.CountMessages(); n != 2 {
		t.Errorf("rows %d, want 2", n)
	}
}

func TestOpenSessionLoadsEarlierMessages(t *testing.T) {
	s, alice, bob, carol := fixture(t)
	if err := s.SaveBatch([]Message{msg(t, alice, bob, "one", 1), msg(t, bob, alice, "two", 2), msg(t, carol, bob, "other", 3)}); err != nil {
		t.Fatal(err)
	}
	session, err := OpenSession(s, bob, alice, 100) // order of the two users does not matter
	if err != nil {
		t.Fatal(err)
	}
	if got := session.Loaded(); len(got) != 2 || got[0].Body != "one" || got[1].Body != "two" {
		t.Errorf("loaded %v, want [one two]", got)
	}
}

func TestSavedKeywordsThroughTheSession(t *testing.T) {
	s, alice, bob, _ := fixture(t)
	events := start(t, s, alice, bob)
	reply := make(chan KeywordResult, 1)
	events <- Event{Kind: EventSaveKeyword, User: alice, Keyword: "Rust", KeywordReply: reply}
	if r := <-reply; r.Err != nil || !r.Added {
		t.Fatalf("save: %+v", r)
	}

	RunUser(bob, alice, 120, 0, 0, events) // 100 saved, 20 pending
	events <- Event{Kind: EventRunSavedKeywords, Viewer: alice, KeywordReply: reply}
	r := <-reply
	if r.Err != nil || len(r.Matches) != 1 || r.Matches[0].Keyword.Keyword != "rust" ||
		len(r.Matches[0].Saved)+len(r.Matches[0].Unsaved) != 10 { // 120 / 12 lines
		t.Errorf("rerun: %+v", r)
	}

	events <- Event{Kind: EventForgetKeyword, User: alice, Keyword: "rust", KeywordReply: reply}
	if r := <-reply; r.Err != nil || !r.Removed {
		t.Errorf("forget: %+v", r)
	}
	closeSession(events)
	if list, _ := s.Keywords(alice); len(list) != 0 {
		t.Errorf("keywords after forget: %v", list)
	}
}

// Go's safety net for what Rust's compiler checks: every event kind must be
// answered. An unhandled kind would leave the sender waiting forever, so each
// request here has a time limit.
func TestEveryEventKindGetsAReply(t *testing.T) {
	s, alice, bob, _ := fixture(t)
	events := start(t, s, alice, bob)
	wait := func(name string, got <-chan struct{}) {
		t.Helper()
		select {
		case <-got:
		case <-time.After(2 * time.Second):
			t.Fatalf("%s: no reply after 2 seconds (is this kind handled in Session.Run?)", name)
		}
	}
	answered := func(f func()) <-chan struct{} {
		c := make(chan struct{})
		go func() { f(); close(c) }()
		return c
	}
	listing := make(chan Listing, 1)
	keyword := make(chan KeywordResult, 1)

	events <- Event{Kind: EventHistory, Listing: listing}
	wait("EventHistory", answered(func() { <-listing }))
	events <- Event{Kind: EventSearch, Viewer: alice, Query: Search{Kind: SearchKeyword, Keyword: "x"}, Listing: listing}
	wait("EventSearch", answered(func() { <-listing }))
	events <- Event{Kind: EventSaveKeyword, User: alice, Keyword: "x", KeywordReply: keyword}
	wait("EventSaveKeyword", answered(func() { <-keyword }))
	events <- Event{Kind: EventForgetKeyword, User: alice, Keyword: "x", KeywordReply: keyword}
	wait("EventForgetKeyword", answered(func() { <-keyword }))
	events <- Event{Kind: EventRunSavedKeywords, Viewer: alice, KeywordReply: keyword}
	wait("EventRunSavedKeywords", answered(func() { <-keyword }))
	done := make(chan SessionReport, 1)
	events <- Event{Kind: EventClose, Done: done}
	wait("EventClose", answered(func() { <-done }))
	// EventSend has no reply; TestTwoUsersAtOnceLoseNothing checks it.
}
