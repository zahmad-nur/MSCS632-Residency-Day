package chat

// This file: the SQLite store (Day 1 Report: Storage Strategy and Table 5).
//
//   - One row per message, in a messages table that points at a users table.
//   - The database itself enforces the spec's rules with CHECK constraints, so
//     a bad row is refused even if a bug lets it past the Go code.
//   - A batch of messages is saved in ONE transaction: all rows or none.
//
// The SQL text is identical to rust-chat/src/store.rs. The driver is
// github.com/mattn/go-sqlite3, which uses the same C SQLite library as Rust's
// rusqlite, so the two versions are compared on the same database engine.

import (
	"database/sql"
	"errors"
	"fmt"
	"time"

	_ "github.com/mattn/go-sqlite3" // registers the "sqlite3" driver
)

const schema = `
CREATE TABLE IF NOT EXISTS users (
    id         INTEGER PRIMARY KEY,
    username   TEXT NOT NULL UNIQUE CHECK (length(username) BETWEEN 3 AND 20),
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
    id           INTEGER PRIMARY KEY,
    sender_id    INTEGER NOT NULL REFERENCES users(id),
    recipient_id INTEGER NOT NULL REFERENCES users(id),
    body         TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 500),
    timestamp    TEXT NOT NULL,
    CHECK (sender_id <> recipient_id)
);
CREATE INDEX IF NOT EXISTS idx_messages_pair_time ON messages(sender_id, recipient_id, timestamp);
CREATE TABLE IF NOT EXISTS saved_keywords (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id),
    keyword    TEXT NOT NULL CHECK (length(keyword) BETWEEN 1 AND 50),
    created_at TEXT NOT NULL,
    UNIQUE (user_id, keyword)
);
`

// selectMessages is the start of every message query; the columns are read in this order.
const selectMessages = "SELECT id, sender_id, recipient_id, body, timestamp FROM messages "

// ErrUnknownUser is returned when a username is not in the database.
var ErrUnknownUser = errors.New("unknown user")

// Store is the SQLite layer.
type Store struct {
	db *sql.DB
}

// OpenStore opens (or creates) the database file and makes sure the tables
// exist. Use ":memory:" for a temporary database (tests and the demo).
func OpenStore(path string) (*Store, error) {
	db, err := sql.Open("sqlite3", path)
	if err != nil {
		return nil, fmt.Errorf("open database %q: %w", path, err)
	}
	// database/sql keeps a pool of connections. One connection keeps the
	// settings below in force for every query and makes ":memory:" work.
	db.SetMaxOpenConns(1)

	var mode string // WAL lets reads continue during a write (Day 1 Report, Table 5)
	if err := db.QueryRow("PRAGMA journal_mode = WAL").Scan(&mode); err != nil {
		db.Close()
		return nil, fmt.Errorf("set WAL mode: %w", err)
	}
	if _, err := db.Exec("PRAGMA foreign_keys = ON;" + schema); err != nil {
		db.Close()
		return nil, fmt.Errorf("create tables: %w", err)
	}
	return &Store{db: db}, nil
}

// Close closes the database. Callers usually write `defer store.Close()`.
func (s *Store) Close() error { return s.db.Close() }

// EnsureUser returns the user with this name, adding them first if they are new.
func (s *Store) EnsureUser(name string) (User, error) {
	return s.EnsureUserAt(name, time.Now().UTC())
}

// EnsureUserAt is EnsureUser with a given creation time (tests and the demo).
func (s *Store) EnsureUserAt(name string, createdAt time.Time) (User, error) {
	if err := ValidateUsername(name); err != nil {
		return User{}, err
	}
	if _, err := s.db.Exec(`INSERT OR IGNORE INTO users (username, created_at) VALUES (?1, ?2)`,
		name, createdAt.UTC().Format(TimeLayout)); err != nil {
		return User{}, fmt.Errorf("add user %s: %w", name, err)
	}
	return s.User(name)
}

// User looks up an existing user by name.
func (s *Store) User(name string) (User, error) {
	var u User
	var created string
	err := s.db.QueryRow(`SELECT id, username, created_at FROM users WHERE username = ?1`, name).
		Scan(&u.ID, &u.Username, &created)
	if errors.Is(err, sql.ErrNoRows) {
		return User{}, fmt.Errorf("%w: %s", ErrUnknownUser, name)
	}
	if err != nil {
		return User{}, fmt.Errorf("look up user %s: %w", name, err)
	}
	if u.CreatedAt, err = time.Parse(TimeLayout, created); err != nil {
		return User{}, fmt.Errorf("bad timestamp in database: %w", err)
	}
	return u, nil
}

// Names maps user IDs to usernames, used to print messages with names.
func (s *Store) Names() (map[UserID]string, error) {
	rows, err := s.db.Query(`SELECT id, username FROM users`)
	if err != nil {
		return nil, fmt.Errorf("list users: %w", err)
	}
	defer rows.Close()
	names := make(map[UserID]string)
	for rows.Next() {
		var id UserID
		var name string
		if err := rows.Scan(&id, &name); err != nil {
			return nil, err
		}
		names[id] = name
	}
	return names, rows.Err()
}

// SaveBatch saves messages as one row each, inside ONE transaction.
//
// Memory: the slice is passed by sharing, not copying. SaveBatch writes the
// new IDs straight into the caller's messages (batch[i].ID), which works only
// because both point at the same underlying array. IDs are written only after
// the commit succeeds, so on error the messages are left unsaved (ID 0) and
// the caller can retry them.
func (s *Store) SaveBatch(batch []Message) error {
	tx, err := s.db.Begin()
	if err != nil {
		return fmt.Errorf("begin: %w", err)
	}
	// Go needs this explicit line. If anything below fails, the deferred
	// Rollback undoes the batch; after a successful Commit it does nothing.
	defer tx.Rollback()

	stmt, err := tx.Prepare(`INSERT INTO messages (sender_id, recipient_id, body, timestamp) VALUES (?1, ?2, ?3, ?4)`)
	if err != nil {
		return fmt.Errorf("prepare: %w", err)
	}
	defer stmt.Close()

	ids := make([]int64, 0, len(batch))
	for _, m := range batch {
		res, err := stmt.Exec(m.From, m.To, m.Body, m.Timestamp.UTC().Format(TimeLayout))
		if err != nil {
			return fmt.Errorf("insert message: %w", err)
		}
		id, err := res.LastInsertId()
		if err != nil {
			return fmt.Errorf("read new id: %w", err)
		}
		ids = append(ids, id)
	}
	if err := tx.Commit(); err != nil {
		return fmt.Errorf("commit: %w", err)
	}
	for i := range batch { // index, not `for _, m`: m would be a copy
		batch[i].ID = ids[i]
	}
	return nil
}

// Conversation returns every message between two users, oldest first.
func (s *Store) Conversation(a, b UserID) ([]Message, error) {
	return s.queryMessages(`WHERE (sender_id = ?1 AND recipient_id = ?2) OR (sender_id = ?2 AND recipient_id = ?1) ORDER BY timestamp, id`, a, b)
}

// CountMessages returns how many messages are saved.
func (s *Store) CountMessages() (int64, error) {
	var n int64
	err := s.db.QueryRow(`SELECT COUNT(*) FROM messages`).Scan(&n)
	return n, err
}

// queryMessages runs selectMessages + a WHERE clause and turns each row into a
// Message. rows.Scan copies each column into a variable passed by pointer.
func (s *Store) queryMessages(where string, args ...any) ([]Message, error) {
	rows, err := s.db.Query(selectMessages+where, args...)
	if err != nil {
		return nil, fmt.Errorf("query messages: %w", err)
	}
	defer rows.Close()
	var out []Message
	for rows.Next() {
		var m Message
		var ts string
		if err := rows.Scan(&m.ID, &m.From, &m.To, &m.Body, &ts); err != nil {
			return nil, err
		}
		if m.Timestamp, err = time.Parse(TimeLayout, ts); err != nil {
			return nil, fmt.Errorf("bad timestamp in database: %w", err)
		}
		out = append(out, m)
	}
	return out, rows.Err()
}
