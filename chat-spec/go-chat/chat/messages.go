package chat

// This file: messages (spec, Table 1): ID, From, To, Body, Timestamp.
// Rules from the spec:
//   - A message goes from one user to exactly one other user (one-to-one only).
//   - A message body is 1-500 characters.
//   - Timestamps are UTC.
//   - A new message has no database ID until it is saved.

import (
	"errors"
	"fmt"
	"time"
	"unicode/utf8"
)

// TimeLayout is how timestamps are printed and stored (UTC, whole seconds).
const TimeLayout = "2006-01-02T15:04:05Z"

// Errors for messages, tested with errors.Is. The text matches the Rust version.
var (
	ErrInvalidBody = errors.New("message must be 1-500 characters")
	ErrSelfMessage = errors.New("cannot send a message to yourself")
)

// Message is one chat message.
type Message struct {
	// ID is 0 while the message exists only in memory; SQLite assigns the real
	// ID when the message is saved. Go has no Option type, so "no ID yet" is a
	// convention (SQLite IDs start at 1), checked with IsSaved.
	ID        int64
	From      UserID
	To        UserID
	Body      string
	Timestamp time.Time
}

// NewMessage creates a message stamped with the current UTC time (whole seconds).
func NewMessage(from, to UserID, body string) (Message, error) {
	return NewMessageAt(from, to, body, time.Now().UTC().Truncate(time.Second))
}

// NewMessageAt creates a message with a given time (used by tests and the
// demo, so their output is the same on every run).
func NewMessageAt(from, to UserID, body string, timestamp time.Time) (Message, error) {
	if from == to {
		return Message{}, ErrSelfMessage
	}
	if err := ValidateBody(body); err != nil {
		return Message{}, err
	}
	return Message{From: from, To: to, Body: body, Timestamp: timestamp}, nil
}

// IsSaved reports whether the database has given the message an ID.
func (m Message) IsSaved() bool { return m.ID != 0 }

// String is how a message is printed.
func (m Message) String() string {
	s := fmt.Sprintf("[%s] %d -> %d: %s", m.Timestamp.UTC().Format(TimeLayout), m.From, m.To, m.Body)
	if m.IsSaved() {
		return s + fmt.Sprintf(" (id %d)", m.ID)
	}
	return s + " (unsaved)"
}

// ValidateBody checks the rule: 1-500 characters. Characters (runes), not
// bytes: len("é") is 2 in Go, so we count with utf8.RuneCountInString.
func ValidateBody(body string) error {
	n := utf8.RuneCountInString(body)
	if n < 1 || n > 500 {
		return fmt.Errorf("%w (got %d)", ErrInvalidBody, n)
	}
	return nil
}