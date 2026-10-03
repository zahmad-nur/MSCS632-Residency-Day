// Package chat holds the chat application. This file is the data model from
// the spec (Table 1): User and Message.
//
// Rules from the spec:
//   - IDs are signed 64-bit (int64), because SQLite stores integers that way.
//   - Usernames are unique, 3-20 characters of a-z, 0-9 or _.
//   - A message goes from one user to exactly one other user (one-to-one only).
//   - A message body is 1-500 characters.
//   - Timestamps are UTC.
//   - A new message has no database ID until it is saved.
package chat

import (
	"errors"
	"fmt"
	"time"
	"unicode/utf8"
)

// TimeLayout is how timestamps are printed and stored (UTC, whole seconds).
const TimeLayout = "2006-01-02T15:04:05Z"

// UserID is a user's database ID. A named type: Go will not mix it with a
// plain int64 or another named type without an explicit conversion.
type UserID int64

// Errors. Go has no enums, so each kind of error is a value, and callers test
// for it with errors.Is. The wrapped messages match the Rust version exactly.
var (
	ErrInvalidUsername = errors.New("invalid username")
	ErrInvalidBody     = errors.New("message must be 1-500 characters")
	ErrSelfMessage     = errors.New("cannot send a message to yourself")
)

// User is one person in the chat.
type User struct {
	ID        UserID
	Username  string
	CreatedAt time.Time
}

// NewUser builds a user, checking the username rule first.
func NewUser(id UserID, username string, createdAt time.Time) (User, error) {
	if err := ValidateUsername(username); err != nil {
		return User{}, err
	}
	return User{ID: id, Username: username, CreatedAt: createdAt}, nil
}

// String is how a user is printed.
func (u User) String() string {
	return fmt.Sprintf("user %d %s (created %s)", u.ID, u.Username, u.CreatedAt.UTC().Format(TimeLayout))
}

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

// ValidateUsername checks the rule: 3-20 characters of a-z, 0-9 or _.
func ValidateUsername(name string) error {
	n := utf8.RuneCountInString(name)
	ok := n >= 3 && n <= 20
	for _, c := range name {
		if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_') {
			ok = false
		}
	}
	if !ok {
		return fmt.Errorf("%w '%s': use 3-20 characters of a-z, 0-9 or _", ErrInvalidUsername, name)
	}
	return nil
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
