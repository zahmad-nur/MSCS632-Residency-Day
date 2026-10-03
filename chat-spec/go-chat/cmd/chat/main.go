// Step 1 demo: builds users and messages with the spec's rules and prints the
// results. The Rust demo prints exactly the same text (scripts/compare.sh).
package main

import (
	"fmt"
	"strings"
	"time"
	"unicode/utf8"

	"chatcompare/chat"
)

func at(sec int) time.Time { return time.Date(2026, 10, 3, 9, 0, sec, 0, time.UTC) }

func main() {
	fmt.Println("== Usernames ==")
	for _, name := range []string{"alice", "bob", "user_42", "al", "Alice", "a-b-c", strings.Repeat("x", 21)} {
		if err := chat.ValidateUsername(name); err != nil {
			fmt.Printf("%s: error: %v\n", name, err)
		} else {
			fmt.Printf("%s: ok\n", name)
		}
	}

	fmt.Println("== Users ==")
	alice, err := chat.NewUser(1, "alice", at(0))
	if err != nil {
		panic(err)
	}
	bob, err := chat.NewUser(2, "bob", at(0))
	if err != nil {
		panic(err)
	}
	fmt.Println(alice)
	fmt.Println(bob)

	fmt.Println("== Messages ==")
	attempts := []struct {
		label    string
		from, to chat.UserID
		body     string
	}{
		{"hey", alice.ID, bob.ID, "hey bob, are you around?"},
		{"reply", bob.ID, alice.ID, "yep, what's up"},
		{"to self", alice.ID, alice.ID, "talking to myself"},
		{"empty", alice.ID, bob.ID, ""},
		{"501 chars", alice.ID, bob.ID, strings.Repeat("x", 501)},
		{"500 accented", alice.ID, bob.ID, strings.Repeat("é", 500)},
	}
	var pending []chat.Message
	for i, a := range attempts {
		m, err := chat.NewMessageAt(a.from, a.to, a.body, at(5+i))
		if err != nil {
			fmt.Printf("%s: error: %v\n", a.label, err)
			continue
		}
		fmt.Printf("%s: ok (%d characters, %d bytes)\n", a.label, utf8.RuneCountInString(m.Body), len(m.Body))
		pending = append(pending, m) // append copies the struct into the slice
	}

	fmt.Println("== Pending (in memory, no IDs yet) ==")
	for _, m := range pending[:2] {
		fmt.Println(m)
	}

	fmt.Println("== After saving (the database assigns IDs) ==")
	for i := range pending {
		pending[i].ID = int64(i + 1) // index into the slice: `for _, m := range` would change a copy
	}
	for _, m := range pending[:2] {
		fmt.Println(m)
	}
	allSaved := true
	for _, m := range pending {
		allSaved = allSaved && m.IsSaved()
	}
	fmt.Printf("(%d messages, all saved: %t)\n", len(pending), allSaved)
}
