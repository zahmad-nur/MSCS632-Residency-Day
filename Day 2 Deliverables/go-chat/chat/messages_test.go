package chat

import (
	"errors"
	"strings"
	"testing"
	"time"
)

func at(sec int) time.Time { return time.Date(2026, 10, 3, 9, 0, sec, 0, time.UTC) }

func TestBodiesAre1To500Characters(t *testing.T) {
	if err := ValidateBody("x"); err != nil {
		t.Error(err)
	}
	if err := ValidateBody(strings.Repeat("é", 500)); err != nil { // 500 characters, 1000 bytes
		t.Error(err)
	}
	if err := ValidateBody(""); err == nil || err.Error() != "message must be 1-500 characters (got 0)" {
		t.Errorf("empty body: got %v", err)
	}
	if err := ValidateBody(strings.Repeat("x", 501)); !errors.Is(err, ErrInvalidBody) {
		t.Errorf("501 characters: got %v", err)
	}
}

func TestMessagesAreOneToOne(t *testing.T) {
	if _, err := NewMessage(1, 1, "me"); !errors.Is(err, ErrSelfMessage) {
		t.Errorf("got %v, want ErrSelfMessage", err)
	}
	m, err := NewMessage(1, 2, "hi")
	if err != nil {
		t.Fatal(err)
	}
	if m.Timestamp.Nanosecond() != 0 {
		t.Error("timestamp should be whole seconds")
	}
}

func TestNewMessagesHaveNoIDUntilSaved(t *testing.T) {
	m, err := NewMessageAt(1, 2, "hey bob", at(5))
	if err != nil {
		t.Fatal(err)
	}
	if m.IsSaved() {
		t.Error("new message should not be saved")
	}
	if got, want := m.String(), "[2026-10-03T09:00:05Z] 1 -> 2: hey bob (unsaved)"; got != want {
		t.Errorf("got %q, want %q", got, want)
	}
	m.ID = 7 // what the database will do when it saves the message
	if got, want := m.String(), "[2026-10-03T09:00:05Z] 1 -> 2: hey bob (id 7)"; got != want {
		t.Errorf("got %q, want %q", got, want)
	}
}
