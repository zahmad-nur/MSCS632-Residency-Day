package chat

import "testing"

func TestSearchSavedMessages(t *testing.T) {
	s, alice, bob, carol := fixture(t)
	if err := s.SaveBatch([]Message{
		msg(t, alice, bob, "100% done", 1),
		msg(t, bob, alice, "Report ready", 2),
		msg(t, alice, bob, "snake_case report", 3),
		msg(t, carol, bob, "carol's report", 4),
	}); err != nil {
		t.Fatal(err)
	}
	count := func(viewer UserID, q Search) int {
		t.Helper()
		found, err := s.Search(viewer, q)
		if err != nil {
			t.Fatal(err)
		}
		return len(found)
	}

	// Messages alice sent in bob's conversation with her.
	if n := count(bob, Search{Kind: SearchUser, User: alice}); n != 2 {
		t.Errorf("by user: got %d, want 2", n)
	}

	// Keyword: capitals ignored, newest first, only the viewer's conversations.
	found, err := s.Search(alice, Search{Kind: SearchKeyword, Keyword: "REPORT"})
	if err != nil {
		t.Fatal(err)
	}
	if len(found) != 2 || found[0].Body != "snake_case report" || found[1].Body != "Report ready" {
		t.Errorf("by keyword: got %v", found)
	}

	// % and _ match themselves, not "anything".
	if n := count(alice, Search{Kind: SearchKeyword, Keyword: "%"}); n != 1 {
		t.Errorf("percent: got %d, want 1", n)
	}
	if n := count(alice, Search{Kind: SearchKeyword, Keyword: "_"}); n != 1 {
		t.Errorf("underscore: got %d, want 1", n)
	}

	// Both filters at once.
	if n := count(bob, Search{Kind: SearchUserAndKeyword, User: alice, Keyword: "report"}); n != 1 {
		t.Errorf("both: got %d, want 1", n)
	}

	// An unknown kind is an error, not a silent empty result.
	if _, err := s.Search(alice, Search{Kind: 99}); err == nil {
		t.Error("unknown kind should be an error")
	}
}

func TestSearchPendingMessages(t *testing.T) {
	_, alice, bob, carol := fixture(t)
	pending := []Message{
		msg(t, alice, bob, "Hello Bob", 1),
		msg(t, bob, alice, "Did you finish the Report?", 2),
		msg(t, alice, bob, "report sent", 3),
		msg(t, carol, bob, "report from carol", 4),
	}
	cases := []struct {
		name   string
		viewer UserID
		q      Search
		want   int
	}{
		{"from alice to bob", bob, Search{Kind: SearchUser, User: alice}, 2},
		{"keyword REPORT", alice, Search{Kind: SearchKeyword, Keyword: "REPORT"}, 2},
		{"from bob AND report", alice, Search{Kind: SearchUserAndKeyword, User: bob, Keyword: "report"}, 1},
		{"no match", alice, Search{Kind: SearchKeyword, Keyword: "zebra"}, 0},
	}
	for _, c := range cases {
		if got := len(SearchPending(pending, c.viewer, c.q)); got != c.want {
			t.Errorf("%s: got %d, want %d", c.name, got, c.want)
		}
	}
}
