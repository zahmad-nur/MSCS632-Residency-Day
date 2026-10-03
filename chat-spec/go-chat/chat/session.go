package chat

// This file: a chat session between two users (spec: Message Lifecycle).
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

// Session is one open chat between users A and B.
type Session struct {
	A, B      UserID
	SaveEvery int

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
			s.accept(ev.Msg)
		case EventHistory:
			saved, err := s.store.Conversation(s.A, s.B)
			// slices.Clone-style copy: the reply must not share pending's array,
			// or the asker would see it change as new messages arrive.
			unsaved := append([]Message(nil), s.pending...)
			ev.Listing <- Listing{Saved: saved, Unsaved: unsaved, Err: err}
		case EventSearch:
			saved, err := s.store.Search(ev.Viewer, ev.Query)
			ev.Listing <- Listing{Saved: saved, Unsaved: SearchPending(s.pending, ev.Viewer, ev.Query), Err: err}
		case EventClose:
			s.save() // step 4, Close: save what is left
			ev.Done <- s.report
			s.release()
			return
		}
		// No default: an unknown kind is silently ignored. The compiler does
		// not warn when a new EventKind is added but not handled here.
	}
	s.save() // the channel was closed without EventClose: still save what is left
	s.release()
}

// accept is step 2, Chat: add a new message to the pending list; save at 100.
func (s *Session) accept(m Message) {
	inThisChat := (m.From == s.A && m.To == s.B) || (m.From == s.B && m.To == s.A)
	if !inThisChat {
		s.report.Rejected++
		return
	}
	s.pending = append(s.pending, m) // append copies the struct into the slice
	s.report.Accepted++
	if s.SaveEvery > 0 && len(s.pending) >= s.SaveEvery {
		s.save()
	}
}

// save is steps 3 and 4: save every pending message in one transaction.
func (s *Session) save() {
	if len(s.pending) == 0 {
		return
	}
	if err := s.store.SaveBatch(s.pending); err != nil {
		// Nothing was saved and pending is untouched, so the next save
		// retries the same messages (spec: a failed save loses nothing).
		s.report.SaveErrors++
		return
	}
	s.report.Batches = append(s.report.Batches, len(s.pending))
	s.report.Saved += len(s.pending)

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
