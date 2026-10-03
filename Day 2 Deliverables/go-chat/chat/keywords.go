package chat

// This file: extra feature, saved keywords (Day 1 Report, Appendix).
//
// A user saves words or phrases they care about and can rerun those searches
// later. Keywords live in the saved_keywords table (user, keyword, created
// time), so they survive restarts, and each rerun is the spec's keyword search.
//
// Keywords are stored trimmed and in lowercase: the search ignores capital
// letters anyway, so "Rust" and "rust" are the same saved keyword.
//
// The SQL text is identical to rust-chat/src/keywords.rs.

import (
	"errors"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"
)

// MaxKeywordChars is the longest keyword allowed, in characters.
const MaxKeywordChars = 50

// ErrInvalidKeyword is returned for an empty or too long keyword.
var ErrInvalidKeyword = errors.New("keyword must be 1-50 characters")

// SavedKeyword is one keyword a user saved.
type SavedKeyword struct {
	ID        int64
	User      UserID
	Keyword   string
	CreatedAt time.Time
}

// NormalizeKeyword trims and lowercases a keyword and checks its length.
func NormalizeKeyword(word string) (string, error) {
	w := strings.ToLower(strings.TrimSpace(word))
	n := utf8.RuneCountInString(w)
	if n < 1 || n > MaxKeywordChars {
		return "", fmt.Errorf("%w (got %d)", ErrInvalidKeyword, n)
	}
	return w, nil
}

// SaveKeyword saves a keyword for a user. added is false when the user had
// already saved it; the original row is returned then. (Go returns an extra
// bool here; Rust returns an enum with two cases, SaveOutcome.)
func (s *Store) SaveKeyword(user UserID, word string, at time.Time) (k SavedKeyword, added bool, err error) {
	keyword, err := NormalizeKeyword(word)
	if err != nil {
		return SavedKeyword{}, false, err
	}
	// ON CONFLICT ... DO NOTHING: a second save of the same keyword changes no
	// rows instead of failing; RowsAffected says how many rows changed.
	res, err := s.db.Exec(`INSERT INTO saved_keywords (user_id, keyword, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (user_id, keyword) DO NOTHING`, user, keyword, at.UTC().Format(TimeLayout))
	if err != nil {
		return SavedKeyword{}, false, fmt.Errorf("save keyword: %w", err)
	}
	changed, err := res.RowsAffected()
	if err != nil {
		return SavedKeyword{}, false, err
	}
	var created string
	k = SavedKeyword{User: user, Keyword: keyword}
	if err := s.db.QueryRow(`SELECT id, created_at FROM saved_keywords WHERE user_id = ?1 AND keyword = ?2`,
		user, keyword).Scan(&k.ID, &created); err != nil {
		return SavedKeyword{}, false, fmt.Errorf("read keyword: %w", err)
	}
	if k.CreatedAt, err = time.Parse(TimeLayout, created); err != nil {
		return SavedKeyword{}, false, fmt.Errorf("bad timestamp in database: %w", err)
	}
	return k, changed == 1, nil
}

// Keywords returns a user's saved keywords in alphabetical order.
func (s *Store) Keywords(user UserID) ([]SavedKeyword, error) {
	rows, err := s.db.Query(`SELECT id, keyword, created_at FROM saved_keywords WHERE user_id = ?1 ORDER BY keyword`, user)
	if err != nil {
		return nil, fmt.Errorf("list keywords: %w", err)
	}
	defer rows.Close()
	var out []SavedKeyword
	for rows.Next() {
		k := SavedKeyword{User: user}
		var created string
		if err := rows.Scan(&k.ID, &k.Keyword, &created); err != nil {
			return nil, err
		}
		if k.CreatedAt, err = time.Parse(TimeLayout, created); err != nil {
			return nil, fmt.Errorf("bad timestamp in database: %w", err)
		}
		out = append(out, k)
	}
	return out, rows.Err()
}

// ForgetKeyword removes one of a user's saved keywords; removed is false if
// it was not saved.
func (s *Store) ForgetKeyword(user UserID, word string) (removed bool, err error) {
	keyword, err := NormalizeKeyword(word)
	if err != nil {
		return false, err
	}
	res, err := s.db.Exec(`DELETE FROM saved_keywords WHERE user_id = ?1 AND keyword = ?2`, user, keyword)
	if err != nil {
		return false, fmt.Errorf("forget keyword: %w", err)
	}
	n, err := res.RowsAffected()
	return n == 1, err
}

// RunSavedKeywords reruns every saved keyword of viewer as a keyword search
// over the viewer's saved messages (newest first).
func (s *Store) RunSavedKeywords(viewer UserID) ([]KeywordMatches, error) {
	keywords, err := s.Keywords(viewer)
	if err != nil {
		return nil, err
	}
	out := make([]KeywordMatches, 0, len(keywords))
	for _, k := range keywords {
		found, err := s.Search(viewer, Search{Kind: SearchKeyword, Keyword: k.Keyword})
		if err != nil {
			return nil, err
		}
		out = append(out, KeywordMatches{Keyword: k, Saved: found})
	}
	return out, nil
}
