package chat

import (
	"errors"
	"strings"
	"testing"
)

func TestKeywordsAreNormalizedAndChecked(t *testing.T) {
	if w, err := NormalizeKeyword("  Rust "); err != nil || w != "rust" {
		t.Errorf("got %q, %v; want \"rust\"", w, err)
	}
	if _, err := NormalizeKeyword("   "); !errors.Is(err, ErrInvalidKeyword) {
		t.Errorf("blank keyword: got %v", err)
	}
	if _, err := NormalizeKeyword(strings.Repeat("x", 51)); err == nil || err.Error() != "keyword must be 1-50 characters (got 51)" {
		t.Errorf("long keyword: got %v", err)
	}
}

func TestSaveListAndForgetKeywords(t *testing.T) {
	s, alice, bob, _ := fixture(t)
	for _, w := range []string{"rust", "Lunch"} {
		if _, added, err := s.SaveKeyword(alice, w, at(1)); err != nil || !added {
			t.Fatalf("save %q: added %t, err %v", w, added, err)
		}
	}
	// The same word again (any capitals) is not a second keyword.
	k, added, err := s.SaveKeyword(alice, "RUST", at(3))
	if err != nil || added || !k.CreatedAt.Equal(at(1)) {
		t.Errorf("save RUST again: %+v, added %t, err %v; want the original rust", k, added, err)
	}
	if _, _, err := s.SaveKeyword(bob, "rust", at(4)); err != nil { // keywords are per user
		t.Fatal(err)
	}
	list, err := s.Keywords(alice)
	if err != nil || len(list) != 2 || list[0].Keyword != "lunch" || list[1].Keyword != "rust" {
		t.Errorf("alice's keywords: %v, %v; want [lunch rust]", list, err)
	}

	if removed, err := s.ForgetKeyword(alice, "LUNCH"); err != nil || !removed {
		t.Errorf("forget lunch: removed %t, err %v", removed, err)
	}
	if removed, _ := s.ForgetKeyword(alice, "lunch"); removed {
		t.Error("lunch was already forgotten")
	}
	if a, _ := s.Keywords(alice); len(a) != 1 {
		t.Errorf("alice has %d keywords, want 1", len(a))
	}
	if b, _ := s.Keywords(bob); len(b) != 1 {
		t.Errorf("bob has %d keywords, want 1", len(b))
	}
}

func TestRerunSavedKeywords(t *testing.T) {
	s, alice, bob, carol := fixture(t)
	if err := s.SaveBatch([]Message{
		msg(t, alice, bob, "did you finish the rust part?", 1),
		msg(t, bob, alice, "Rust is done, lunch?", 2),
		msg(t, carol, bob, "rust rust rust", 3), // not alice's conversation
	}); err != nil {
		t.Fatal(err)
	}
	s.SaveKeyword(alice, "rust", at(1))
	s.SaveKeyword(alice, "zebra", at(1))
	results, err := s.RunSavedKeywords(alice)
	if err != nil {
		t.Fatal(err)
	}
	if len(results) != 2 || results[0].Keyword.Keyword != "rust" || len(results[0].Saved) != 2 ||
		results[1].Keyword.Keyword != "zebra" || len(results[1].Saved) != 0 {
		t.Errorf("got %+v; want rust: 2, zebra: 0", results)
	}
}
