package chat

import (
	"errors"
	"testing"
)

func TestUsernamesFollowTheRule(t *testing.T) {
	for _, ok := range []string{"alice", "bob", "user_42", "abc", "a2345678901234567890"} {
		if err := ValidateUsername(ok); err != nil {
			t.Errorf("%q should be allowed, got %v", ok, err)
		}
	}
	for _, bad := range []string{"al", "Alice", "a-b-c", "has space", "", "a23456789012345678901"} {
		if err := ValidateUsername(bad); !errors.Is(err, ErrInvalidUsername) {
			t.Errorf("%q should be rejected, got %v", bad, err)
		}
	}
}

func TestNewUserChecksTheName(t *testing.T) {
	u, err := NewUser(1, "alice", at(0))
	if err != nil {
		t.Fatal(err)
	}
	if got, want := u.String(), "user 1 alice (created 2026-10-03T09:00:00Z)"; got != want {
		t.Errorf("got %q, want %q", got, want)
	}
	if _, err := NewUser(2, "Bob", at(0)); err == nil {
		t.Error("Bob should be rejected")
	}
}
