package chat

import (
	"errors"
	"testing"
	"time"
)

// fixture is a fresh in-memory database with alice (1), bob (2) and carol (3).
func fixture(t *testing.T) (s *Store, alice, bob, carol UserID) {
	t.Helper()
	s, err := OpenStore(":memory:")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { s.Close() })
	ids := make([]UserID, 3)
	for i, name := range []string{"alice", "bob", "carol"} {
		u, err := s.EnsureUserAt(name, at(0))
		if err != nil {
			t.Fatal(err)
		}
		ids[i] = u.ID
	}
	return s, ids[0], ids[1], ids[2]
}

func msg(t *testing.T, from, to UserID, body string, sec int) Message {
	t.Helper()
	m, err := NewMessageAt(from, to, body, at(sec))
	if err != nil {
		t.Fatal(err)
	}
	return m
}

func TestUsersAreAddedOnce(t *testing.T) {
	s, alice, _, _ := fixture(t)
	again, err := s.EnsureUserAt("alice", at(9))
	if err != nil {
		t.Fatal(err)
	}
	if again.ID != alice || !again.CreatedAt.Equal(at(0)) {
		t.Errorf("got %v, want the original alice", again)
	}
	if _, err := s.EnsureUserAt("Bad Name", at(0)); !errors.Is(err, ErrInvalidUsername) {
		t.Errorf("bad name: got %v", err)
	}
	if _, err := s.User("nobody"); !errors.Is(err, ErrUnknownUser) {
		t.Errorf("unknown user: got %v", err)
	}
}

func TestSaveAssignsIDsAndLoadsBack(t *testing.T) {
	s, alice, bob, carol := fixture(t)
	batch := []Message{msg(t, alice, bob, "hello", 1), msg(t, bob, alice, "hi there", 2), msg(t, carol, bob, "other chat", 3)}
	if err := s.SaveBatch(batch); err != nil {
		t.Fatal(err)
	}
	for i, m := range batch {
		if m.ID != int64(i+1) {
			t.Errorf("message %d: got id %d", i, m.ID)
		}
	}
	convo, err := s.Conversation(bob, alice)
	if err != nil {
		t.Fatal(err)
	}
	if len(convo) != 2 { // carol's message is a different conversation
		t.Fatalf("got %d messages, want 2", len(convo))
	}
	for i, got := range convo { // same id, sender, recipient, body and time
		want := batch[i]
		if got.ID != want.ID || got.From != want.From || got.To != want.To ||
			got.Body != want.Body || !got.Timestamp.Equal(want.Timestamp) {
			t.Errorf("message %d: got %v, want %v", i, got, want)
		}
	}
}

func TestDatabaseRefusesBadRowsAndRollsBack(t *testing.T) {
	s, alice, bob, _ := fixture(t)
	// Built by hand to get past the Message checks, so the database must catch it.
	if err := s.SaveBatch([]Message{{From: alice, To: alice, Body: "me", Timestamp: at(1)}}); err == nil {
		t.Error("message to self should be refused")
	}
	if err := s.SaveBatch([]Message{{From: alice, To: bob, Body: "", Timestamp: at(1)}}); err == nil {
		t.Error("empty body should be refused")
	}
	// A good message followed by a bad one: the whole batch is rolled back.
	batch := []Message{msg(t, alice, bob, "fine", 1), {From: alice, To: 99, Body: "who?", Timestamp: at(2)}}
	if err := s.SaveBatch(batch); err == nil {
		t.Error("batch with a bad row should be refused")
	}
	if n, _ := s.CountMessages(); n != 0 {
		t.Errorf("got %d rows, want 0", n)
	}
	for _, m := range batch { // left unsaved, ready to retry
		if m.IsSaved() {
			t.Errorf("message should still be unsaved: %v", m)
		}
	}
}

func TestEnsureUserStampsNowAndNamesListsEveryone(t *testing.T) {
	s, alice, bob, carol := fixture(t)
	before := time.Now().UTC().Add(-time.Second)
	dave, err := s.EnsureUser("dave")
	if err != nil {
		t.Fatal(err)
	}
	if dave.CreatedAt.Before(before) {
		t.Errorf("created_at %v should be about now", dave.CreatedAt)
	}
	names, err := s.Names()
	if err != nil {
		t.Fatal(err)
	}
	want := map[UserID]string{alice: "alice", bob: "bob", carol: "carol", dave.ID: "dave"}
	if len(names) != len(want) {
		t.Fatalf("names %v, want %v", names, want)
	}
	for id, name := range want {
		if names[id] != name {
			t.Errorf("names[%d] = %q, want %q", id, names[id], name)
		}
	}
}

func TestOpenStoreReportsBadPaths(t *testing.T) {
	if _, err := OpenStore("/no/such/folder/chat.db"); err == nil {
		t.Error("opening a database in a missing folder should fail")
	}
}
