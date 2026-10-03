package chat

// This file: a chat session between two users (Day 1 Report: Message Lifecycle).
//
// The session runs as ONE goroutine that owns all chat state:
//   - history: the earlier conversation, read from SQLite when the session
//     opens. Read-only: never changed, never saved again.
//   - pending: new messages, only in memory. Every SaveEvery messages, and once
//     more at close, they are saved to SQLite in one transaction and pending is
//     emptied.
//   - store: the SQLite connection.
//
// Other goroutines never touch this state. They send Events over a channel:
// "Do not communicate by sharing memory; instead, share memory by
// communicating." No lock is needed, but only because every goroutine follows
// this rule. The compiler does not check it; `go test -race` does, at run time.

import (
	"fmt"
	"time"
)

// Session is one open chat between users A and B.
type Session struct {
	A, B      UserID
	SaveEvery int

	// Echo, when not nil, makes the session print every accepted message and
	// every save, with usernames from this map (used by the command prompt).
	Echo map[UserID]string

	store   *Store
	history []Message
	pending []Message
	report  SessionReport
}

// OpenSession is step 1 of the lifecycle, Open: it reads the earlier
// conversation between the two users into the read-only history list.
func OpenSession(store *Store, a, b UserID, saveEvery int) (*Session, error) {
	history, err := store.Conversation(a, b)
	if err != nil {
		return nil, err
	}
	return &Session{
		A: a, B: b, SaveEvery: saveEvery,
		store:   store,
		history: history,
		report:  SessionReport{Loaded: len(history)},
	}, nil
}

// Loaded returns the earlier conversation read at open.
func (s *Session) Loaded() []Message { return s.history }

// Run handles events until EventClose arrives or the channel is closed.
// Start it with `go session.Run(events)`.
func (s *Session) Run(events <-chan Event) {
	for ev := range events {
		switch ev.Kind {
		case EventSend:
			accepted := s.accept(ev.Msg)
			if ev.Ack != nil { // only answer if the sender asked
				ev.Ack <- accepted
			}
		case EventHistory:
			saved, err := s.store.Conversation(s.A, s.B)
			// slices.Clone-style copy: the reply must not share pending's array,
			// or the asker would see it change as new messages arrive.
			unsaved := append([]Message(nil), s.pending...)
			ev.Listing <- Listing{Saved: saved, Unsaved: unsaved, Err: err}
		case EventSearch:
			saved, err := s.store.Search(ev.Viewer, ev.Query)
			ev.Listing <- Listing{Saved: saved, Unsaved: SearchPending(s.pending, ev.Viewer, ev.Query), Err: err}
		case EventSaveKeyword:
			k, added, err := s.store.SaveKeyword(ev.User, ev.Keyword, time.Now().UTC())
			ev.KeywordReply <- KeywordResult{Saved: k, Added: added, Err: err}
		case EventForgetKeyword:
			removed, err := s.store.ForgetKeyword(ev.User, ev.Keyword)
			ev.KeywordReply <- KeywordResult{Removed: removed, Err: err}
		case EventRunSavedKeywords:
			matches, err := s.store.RunSavedKeywords(ev.Viewer)
			for i := range matches { // index, so we change the slice element, not a copy
				q := Search{Kind: SearchKeyword, Keyword: matches[i].Keyword.Keyword}
				matches[i].Unsaved = SearchPending(s.pending, ev.Viewer, q)
			}
			ev.KeywordReply <- KeywordResult{Matches: matches, Err: err}
		case EventClose:
			s.save() // step 4, Close: save what is left
			ev.Done <- s.report
			s.release()
			return
		}
		// No default: an unknown kind is silently ignored. The compiler does
		// not warn when a new EventKind is added but not handled here; the
		// sender would then wait forever for a reply. session_test.go checks
		// that every kind gets an answer (TestEveryEventKindGetsAReply).
	}
	s.save() // the channel was closed without EventClose: still save what is left
	s.release()
}

// accept is step 2, Chat: add a new message to the pending list; save at 100.
// It returns false if the message was refused (not between the two users).
func (s *Session) accept(m Message) bool {
	inThisChat := (m.From == s.A && m.To == s.B) || (m.From == s.B && m.To == s.A)
	if !inThisChat {
		s.report.Rejected++
		return false
	}
	if s.Echo != nil {
		fmt.Printf("[%s] %s -> %s: %s\n", m.Timestamp.UTC().Format("15:04:05"), nameOf(s.Echo, m.From), nameOf(s.Echo, m.To), m.Body)
	}
	s.pending = append(s.pending, m) // append copies the struct into the slice
	s.report.Accepted++
	if s.SaveEvery > 0 && len(s.pending) >= s.SaveEvery {
		s.save()
	}
	return true
}

// nameOf returns a user's name from the map, or "?".
func nameOf(names map[UserID]string, id UserID) string {
	if n, ok := names[id]; ok {
		return n
	}
	return "?"
}

// save is steps 3 and 4: save every pending message in one transaction.
func (s *Session) save() {
	if len(s.pending) == 0 {
		return
	}
	if err := s.store.SaveBatch(s.pending); err != nil {
		// Nothing was saved and pending is untouched, so the next save
		// retries the same messages (Day 1 Report: a failed save loses nothing).
		s.report.SaveErrors++
		return
	}
	s.report.Batches = append(s.report.Batches, len(s.pending))
	s.report.Saved += len(s.pending)
	if s.Echo != nil {
		fmt.Printf("(saved %d messages to the database)\n", len(s.pending))
	}

	// Empty the list but keep its array for the next 100 messages.
	// clear() first zeroes the old entries, so their text is no longer
	// referenced and the garbage collector can free it on its next run.
	// Without clear(), pending[:0] alone would keep the old bodies alive until
	// they are overwritten. Any other slice still pointing at this array would
	// now see zeroed (and later overwritten) messages: Go does not stop that.
	clear(s.pending)
	s.pending = s.pending[:0]
}

// release drops the lists so the garbage collector can free them.
func (s *Session) release() {
	s.history, s.pending = nil, nil
}
