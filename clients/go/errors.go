package klyro

import "strings"

// KlyroError represents an "ERR ..." reply from the server. Raw holds the
// full, unparsed reply line exactly as the server sent it (e.g.
// "ERR unknown command").
type KlyroError struct {
	Raw string
}

func (e *KlyroError) Error() string {
	return e.Raw
}

// WrongTypeError is returned when a command is issued against a key that
// holds a different data type (the server's
// "ERR WRONGTYPE Operation against a key holding the wrong kind of value"
// reply). It wraps *KlyroError, so:
//
//   - errors.As(err, &wte) where wte is *WrongTypeError singles out this
//     specific case.
//   - errors.Is(err, ErrWrongType) does the same via the sentinel below.
//   - errors.As(err, &ke) where ke is *KlyroError still matches, and
//     errors.Is/As callers that only care about "some server-side ERR"
//     can keep matching on *KlyroError without change.
type WrongTypeError struct {
	*KlyroError
}

// Unwrap exposes the embedded *KlyroError to errors.Is/errors.As so a
// caller only interested in the generic error type still finds it.
func (e *WrongTypeError) Unwrap() error {
	return e.KlyroError
}

// Is implements the errors.Is contract: any *WrongTypeError matches the
// ErrWrongType sentinel, regardless of the exact raw text carried.
func (e *WrongTypeError) Is(target error) bool {
	_, ok := target.(*WrongTypeError)
	return ok
}

// ErrWrongType is a sentinel usable with errors.Is(err, klyro.ErrWrongType)
// to detect a WRONGTYPE reply without needing errors.As plus a type
// assertion.
var ErrWrongType = &WrongTypeError{&KlyroError{
	Raw: "ERR WRONGTYPE Operation against a key holding the wrong kind of value",
}}

// errFromLine turns a raw "ERR ..." reply line into the appropriate error
// type: *WrongTypeError for the WRONGTYPE case, *KlyroError otherwise.
func errFromLine(line string) error {
	if strings.Contains(line, "WRONGTYPE") {
		return &WrongTypeError{&KlyroError{Raw: line}}
	}
	return &KlyroError{Raw: line}
}
