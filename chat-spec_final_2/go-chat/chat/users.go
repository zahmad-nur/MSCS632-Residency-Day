// Package chat holds the chat application.
//
// This file: users (Day 1 Report, Table 1): ID, Username, CreatedAt.
// Rules from the spec:
//   - IDs are signed 64-bit (int64), because SQLite stores integers that way.
//   - Usernames are unique, 3-20 characters of a-z, 0-9 or _.
//   - Timestamps are UTC.
package chat

import (
	"errors"
	"fmt"
	"time"
	"unicode/utf8"
)

// UserID is a user's database ID. A named type: Go will not mix it with a
// plain int64 or another named type without an explicit conversion.
type UserID int64

// ErrInvalidUsername is returned for a bad username. Go has no enums, so each
// kind of error is a value, and callers test for it with errors.Is.
var ErrInvalidUsername = errors.New("invalid username")

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
