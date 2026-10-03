package chat

// This file: the messages that goroutines send to the chat session over its
// channel.
//
// Go has no enums, so one Event struct carries a Kind plus the fields any kind
// might need; each kind uses only some of them. (Rust uses an enum where each
// case holds exactly its own data: rust-chat/src/event.rs.)

// EventKind says what an Event asks the session to do.
type EventKind int

const (
	EventSend    EventKind = iota // a user posts Msg
	EventHistory                  // reply on Listing with the whole conversation
	EventSearch                   // reply on Listing with the results of Query for Viewer
	EventClose                    // save what is pending, reply on Done, then stop

	// Extra: saved keywords (Day 1 Report, Appendix).
	EventSaveKeyword      // save Keyword for User, reply on KeywordReply
	EventForgetKeyword    // remove Keyword for User, reply on KeywordReply
	EventRunSavedKeywords // rerun Viewer's saved keywords, reply on KeywordReply
)

// Listing is messages from the database plus messages still only in memory.
type Listing struct {
	Saved   []Message
	Unsaved []Message
	Err     error
}

// SessionReport is what the session reports when it closes.
type SessionReport struct {
	Loaded     int   // earlier messages read from SQLite when the session opened
	Accepted   int   // new messages accepted into the pending list
	Rejected   int   // messages refused: not between the two chat users
	Batches    []int // size of each batch written to SQLite, in order
	Saved      int   // total messages written to SQLite during this session
	SaveErrors int   // saves that failed (their messages stay pending and are retried)
}

// Event is everything another goroutine can send to the session.
// Requests that need an answer carry their own reply channel.
type Event struct {
	Kind EventKind

	Msg Message // EventSend
	// Ack is an optional reply for EventSend: true if the message was
	// accepted. nil when the sender does not want one (the simulated users).
	// The session must check for nil: sending on a nil channel blocks forever.
	Ack chan bool

	Viewer UserID // EventSearch
	Query  Search // EventSearch

	Listing chan Listing       // reply channel for EventHistory and EventSearch
	Done    chan SessionReport // reply channel for EventClose

	User         UserID             // EventSaveKeyword, EventForgetKeyword
	Keyword      string             // EventSaveKeyword, EventForgetKeyword
	KeywordReply chan KeywordResult // reply channel for the keyword events
}

// KeywordResult answers the keyword events.
type KeywordResult struct {
	Saved   SavedKeyword     // EventSaveKeyword: the keyword as stored
	Added   bool             // EventSaveKeyword: false if it was already saved
	Removed bool             // EventForgetKeyword: false if it was not saved
	Matches []KeywordMatches // EventRunSavedKeywords
	Err     error
}

// KeywordMatches is one saved keyword and the messages it finds.
type KeywordMatches struct {
	Keyword SavedKeyword
	Saved   []Message
	Unsaved []Message
}
