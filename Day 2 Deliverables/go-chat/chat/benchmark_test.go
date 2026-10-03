package chat

// Benchmarks: Go's testing package measures speed and memory out of the box.
// Run them with:   go test -bench . -benchmem ./chat
// Each benchmark runs its loop b.N times; Go picks b.N so the timing is stable.

import (
	"fmt"
	"testing"
)

// benchStore is an in-memory database with alice and bob.
func benchStore(b *testing.B) (*Store, UserID, UserID) {
	b.Helper()
	s, err := OpenStore(":memory:")
	if err != nil {
		b.Fatal(err)
	}
	b.Cleanup(func() { s.Close() })
	alice, _ := s.EnsureUserAt("alice", at(0))
	bob, _ := s.EnsureUserAt("bob", at(0))
	return s, alice.ID, bob.ID
}

func batchOf(n int, from, to UserID) []Message {
	out := make([]Message, n)
	for i := range out {
		out[i], _ = NewMessageAt(from, to, Lines[i%len(Lines)], at(i%60))
	}
	return out
}

// One save of 100 messages in one transaction (the spec's batch size).
func BenchmarkSaveBatch100(b *testing.B) {
	s, alice, bob := benchStore(b)
	b.ReportAllocs()
	for i := 0; i < b.N; i++ {
		b.StopTimer()
		batch := batchOf(100, alice, bob) // build outside the timed part
		b.StartTimer()
		if err := s.SaveBatch(batch); err != nil {
			b.Fatal(err)
		}
	}
}

// Keyword search over 10,000 saved messages (a full scan, as the spec notes).
func BenchmarkSearchKeyword10000(b *testing.B) {
	s, alice, bob := benchStore(b)
	for i := 0; i < 100; i++ {
		if err := s.SaveBatch(batchOf(100, alice, bob)); err != nil {
			b.Fatal(err)
		}
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		found, err := s.Search(alice, Search{Kind: SearchKeyword, Keyword: "rust"})
		if err != nil || len(found) == 0 {
			b.Fatal(err)
		}
	}
}

// In-memory search of a full pending list (100 messages).
func BenchmarkSearchPending100(b *testing.B) {
	pending := batchOf(100, 1, 2)
	b.ReportAllocs()
	for i := 0; i < b.N; i++ {
		SearchPending(pending, 2, Search{Kind: SearchKeyword, Keyword: "rust"})
	}
}

// The whole pipeline: a session goroutine plus two user goroutines sending
// over a channel, saving every 100. Reports messages per second.
func BenchmarkSessionTwoUsers(b *testing.B) {
	for _, total := range []int{1000, 10000} {
		b.Run(fmt.Sprintf("messages=%d", total), func(b *testing.B) {
			for i := 0; i < b.N; i++ {
				b.StopTimer()
				s, alice, bob := benchStore(b)
				session, err := OpenSession(s, alice, bob, 100)
				if err != nil {
					b.Fatal(err)
				}
				events := make(chan Event, 256)
				b.StartTimer()

				go session.Run(events)
				done := make(chan int, 2)
				go func() { done <- RunUser(alice, bob, total/2, 0, 0, events) }()
				go func() { done <- RunUser(bob, alice, total-total/2, 6, 0, events) }()
				<-done
				<-done
				report := make(chan SessionReport, 1)
				events <- Event{Kind: EventClose, Done: report}
				if r := <-report; r.Saved != total {
					b.Fatalf("saved %d, want %d", r.Saved, total)
				}
			}
			b.ReportMetric(float64(total*b.N)/b.Elapsed().Seconds(), "msgs/sec")
		})
	}
}
