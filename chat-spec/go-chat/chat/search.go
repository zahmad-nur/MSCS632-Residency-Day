package chat

// This file: filtering and search by user or keyword (Day 1 Report: Retrieval and Search).
//
//   - Saved messages are searched with SQL. A user only ever searches their own
//     conversations, so every query is limited to messages the viewer sent or
//     received.
//   - Messages still in memory (the pending list) are not in the database yet,
//     so they are filtered in Go (spec, Table 5, last row).
//
// The SQL text is identical to rust-chat/src/search.rs.

import (
	"fmt"
	"strings"
)

// SearchKind says which of the three searches to run. Go has no enums, so we
// use typed constants; a Search value carries the fields its kind needs.
type SearchKind int

const (
	SearchUser           SearchKind = iota // messages User sent to the viewer
	SearchKeyword                          // the viewer's messages containing Keyword
	SearchUserAndKeyword                   // both filters at once
)

// Search describes one search. Unlike a Rust enum, every field always exists,
// so a SearchKeyword value can still carry a User; only convention says to ignore it.
type Search struct {
	Kind    SearchKind
	User    UserID
	Keyword string
}

const (
	// Spec: messages one person (?2) sent in the viewer's (?1) conversation with them.
	byUser = `WHERE sender_id = ?2 AND recipient_id = ?1 ORDER BY timestamp, id`
	// Spec: the viewer's messages whose body contains the keyword, newest first.
	byKeyword = `WHERE (sender_id = ?1 OR recipient_id = ?1) AND body LIKE '%' || ?2 || '%' ESCAPE '\' ORDER BY timestamp DESC, id DESC`
	// Spec: the two filters combined with AND.
	byUserAndKeyword = `WHERE sender_id = ?2 AND recipient_id = ?1 AND body LIKE '%' || ?3 || '%' ESCAPE '\' ORDER BY timestamp DESC, id DESC`
)

// escapeLike makes % and _ (wildcards in LIKE) match themselves, so a search
// for "100%" or "snake_case" finds those characters literally.
var escapeLike = strings.NewReplacer(`\`, `\\`, `%`, `\%`, `_`, `\_`).Replace

// Search searches the saved messages in the viewer's conversations.
func (s *Store) Search(viewer UserID, q Search) ([]Message, error) {
	switch q.Kind {
	case SearchUser:
		return s.queryMessages(byUser, viewer, q.User)
	case SearchKeyword:
		return s.queryMessages(byKeyword, viewer, escapeLike(q.Keyword))
	case SearchUserAndKeyword:
		return s.queryMessages(byUserAndKeyword, viewer, q.User, escapeLike(q.Keyword))
	default:
		// Needed because the compiler does not check that every kind is handled.
		return nil, fmt.Errorf("unknown search kind %d", q.Kind)
	}
}

// SearchPending searches the pending list (in memory, not saved yet) with the
// same rules. The result holds copies of the matching messages: append copies
// each struct, so later changes to pending do not affect the result (and the
// garbage collector keeps both alive as long as they are used).
func SearchPending(pending []Message, viewer UserID, q Search) []Message {
	contains := func(m Message) bool {
		return strings.Contains(strings.ToLower(m.Body), strings.ToLower(q.Keyword))
	}
	var out []Message
	for _, m := range pending {
		if m.From != viewer && m.To != viewer {
			continue
		}
		var match bool
		switch q.Kind {
		case SearchUser:
			match = m.From == q.User && m.To == viewer
		case SearchKeyword:
			match = contains(m)
		case SearchUserAndKeyword:
			match = m.From == q.User && m.To == viewer && contains(m)
		}
		if match {
			out = append(out, m)
		}
	}
	return out
}
