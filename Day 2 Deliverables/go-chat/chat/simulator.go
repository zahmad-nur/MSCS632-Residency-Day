package chat

// This file: the simulator (simulated users). Each user is a goroutine (started with the `go`
// keyword) that sends messages to the session over its channel.

import (
	"fmt"
	"os"
	"time"
)

// Lines are the things simulated users say. A user starts at firstLine and
// goes round the list, so every run sends the same texts (handy for tests).
var Lines = []string{
	"hey, are you around?",
	"yep, what's up",
	"did you finish the rust part?",
	"almost done, just testing now",
	"lunch anyone?",
	"sounds good, bye",
	"can you review my code?",
	"the build is green again",
	"who broke the tests?",
	"pushing it tonight",
	"great work today",
	"let's sync tomorrow morning",
}

// RunUser is one simulated user: it sends count messages from me to other,
// waiting pause between them, and returns how many it sent.
//
// events is a send-only channel (chan<-): the compiler stops this function
// from receiving on it. Sending blocks while the channel's buffer is full,
// which slows a fast sender down to the speed of the session.
func RunUser(me, other UserID, count, firstLine int, pause time.Duration, events chan<- Event) int {
	return RunUserUntil(nil, me, other, count, firstLine, pause, events)
}

// RunUserUntil is RunUser that also stops early when stop is closed.
//
// A goroutine cannot be stopped from outside in Go (Rust can abort a task);
// it has to check a signal itself. Closing a channel is the usual signal: a
// closed channel is always ready to receive, so every `case <-stop` fires.
// A nil stop channel is never ready, so RunUser (stop == nil) never stops early.
func RunUserUntil(stop <-chan struct{}, me, other UserID, count, firstLine int, pause time.Duration, events chan<- Event) int {
	sent := 0
	for i := 0; i < count; i++ {
		m, err := NewMessage(me, other, Lines[(firstLine+i)%len(Lines)])
		if err != nil {
			fmt.Fprintln(os.Stderr, "warning:", err)
			continue
		}
		// The channel receives a copy of the Event (and the Message inside it).
		// Unlike Rust, `m` is still usable here afterwards.
		select {
		case events <- Event{Kind: EventSend, Msg: m}:
			sent++
		case <-stop:
			return sent
		}
		if pause > 0 {
			select { // wait, but wake up at once if asked to stop
			case <-time.After(pause):
			case <-stop:
				return sent
			}
		}
	}
	return sent
}
