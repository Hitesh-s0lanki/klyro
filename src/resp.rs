//! RESP2, the protocol Redis clients speak.
//!
//! Two halves. [`Reply`] is what a command hands back, and
//! [`encode`] turns it into bytes. [`parse_request`] goes the other
//! way, pulling one command out of a connection's read buffer.
//!
//! Requests arrive in one of two shapes. Real clients send an array of
//! bulk strings (`*2\r\n$3\r\nGET\r\n$1\r\nk\r\n`). People typing at
//! `nc` or `telnet` send a bare line (`GET k`), which Redis calls an
//! inline command and accepts too; that is what keeps this server
//! usable without a client library.

use crate::util::bytes::Bytes;

/// Which protocol version a connection negotiated. RESP3 adds typed
/// aggregates; every one of them has a RESP2 spelling that this module
/// falls back to, so commands describe *what* they return and never
/// which wire form it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Resp2,
    Resp3,
}

impl Protocol {
    pub fn from_version(version: i64) -> Option<Protocol> {
        match version {
            2 => Some(Protocol::Resp2),
            3 => Some(Protocol::Resp3),
            _ => None,
        }
    }

    pub fn version(self) -> i64 {
        match self {
            Protocol::Resp2 => 2,
            Protocol::Resp3 => 3,
        }
    }
}

/// A reply, described by meaning rather than by wire form.
///
/// RESP3's boolean type has no variant here on purpose: every Klyro
/// command whose answer reads as true/false is one Redis replies to
/// with an integer 0 or 1 in both protocol versions.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// `+OK\r\n` - a short, known-good status.
    Simple(&'static str),
    /// `-ERR ...\r\n`. The first word is conventionally an error code.
    Error(String),
    /// `:42\r\n`
    Integer(i64),
    /// `$5\r\nhello\r\n` - arbitrary bytes, length-prefixed.
    Bulk(Bytes),
    /// The "no such value" answer: a null bulk string in RESP2, the
    /// dedicated null type in RESP3.
    Nil,
    /// A null array. Distinct from an empty one, and distinct from
    /// `Nil` only in RESP2.
    NilArray,
    /// `*2\r\n...`
    Array(Vec<Reply>),
    /// Field/value pairs: a flat array in RESP2, a map in RESP3.
    Map(Vec<(Reply, Reply)>),
    /// Unordered unique items: an array in RESP2, a set in RESP3.
    Set(Vec<Reply>),
    /// An out-of-band message - a pub/sub delivery, or the
    /// confirmation of a (un)subscribe. RESP3 gives these their own
    /// type so a client can tell them from the reply to whatever it
    /// asked; RESP2 has no such marker, which is why a RESP2
    /// connection may not run anything but subscribe commands while it
    /// holds a subscription.
    Push(Vec<Reply>),
    /// A number that is a *score*, not a count: a bulk string in RESP2,
    /// the double type in RESP3.
    Double(f64),
    /// Members with their scores. RESP2 flattens them into one array;
    /// RESP3 nests each pair, which is why this can't just be built out
    /// of `Array` at the call site.
    ScoredMembers(Vec<(Bytes, f64)>),
}

impl Reply {
    pub fn ok() -> Reply {
        Reply::Simple("OK")
    }

    /// A bulk string from anything string-shaped.
    pub fn bulk(value: impl Into<Bytes>) -> Reply {
        Reply::Bulk(value.into())
    }

    /// Commands whose answer is a count-like 0 or 1 keep using
    /// `Integer`; this is for the ones Redis types as a boolean.
    pub fn bool(value: bool) -> Reply {
        Reply::Integer(value as i64)
    }

    pub fn error(message: impl Into<String>) -> Reply {
        Reply::Error(message.into())
    }

    /// The reply for arguments that don't fit a command's shape.
    pub fn wrong_arity(command: &str) -> Reply {
        Reply::Error(format!(
            "ERR wrong number of arguments for '{}' command",
            command.to_ascii_lowercase()
        ))
    }

    pub fn array(items: Vec<Reply>) -> Reply {
        Reply::Array(items)
    }

    /// An out-of-band frame whose first element names the kind of
    /// message, the shape every pub/sub frame takes.
    pub fn push(kind: &'static str, rest: Vec<Reply>) -> Reply {
        let mut items = Vec::with_capacity(rest.len() + 1);
        items.push(Reply::bulk(kind));
        items.extend(rest);
        Reply::Push(items)
    }

    /// An array of bulk strings, the shape most listing commands return.
    pub fn bulk_array<I, T>(items: I) -> Reply
    where
        I: IntoIterator<Item = T>,
        T: Into<Bytes>,
    {
        Reply::Array(items.into_iter().map(Reply::bulk).collect())
    }
}

/// Appends `reply`'s wire form to `out`, in `protocol`'s spelling.
pub fn encode(reply: &Reply, protocol: Protocol, out: &mut Vec<u8>) {
    let resp3 = protocol == Protocol::Resp3;
    match reply {
        Reply::Simple(text) => {
            out.push(b'+');
            out.extend_from_slice(text.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        Reply::Error(message) => {
            out.push(b'-');
            // A newline inside an error would end the frame early, so
            // flatten any that sneak in from a formatted message.
            for byte in message.bytes() {
                out.push(if byte == b'\r' || byte == b'\n' {
                    b' '
                } else {
                    byte
                });
            }
            out.extend_from_slice(b"\r\n");
        }
        Reply::Integer(n) => {
            out.push(b':');
            out.extend_from_slice(n.to_string().as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        Reply::Bulk(payload) => {
            out.push(b'$');
            out.extend_from_slice(payload.len().to_string().as_bytes());
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(payload);
            out.extend_from_slice(b"\r\n");
        }
        Reply::Nil => out.extend_from_slice(if resp3 { b"_\r\n" } else { b"$-1\r\n" }),
        Reply::NilArray => out.extend_from_slice(if resp3 { b"_\r\n" } else { b"*-1\r\n" }),
        Reply::Array(items) => encode_aggregate(b'*', items, protocol, out),
        Reply::Set(items) => {
            encode_aggregate(if resp3 { b'~' } else { b'*' }, items, protocol, out)
        }
        Reply::Push(items) => {
            encode_aggregate(if resp3 { b'>' } else { b'*' }, items, protocol, out)
        }
        Reply::Map(pairs) => {
            if resp3 {
                out.push(b'%');
                out.extend_from_slice(pairs.len().to_string().as_bytes());
                out.extend_from_slice(b"\r\n");
                for (key, value) in pairs {
                    encode(key, protocol, out);
                    encode(value, protocol, out);
                }
            } else {
                // RESP2 has no map: send the pairs flattened, which is
                // what Redis does and what clients expect to re-pair.
                out.push(b'*');
                out.extend_from_slice((pairs.len() * 2).to_string().as_bytes());
                out.extend_from_slice(b"\r\n");
                for (key, value) in pairs {
                    encode(key, protocol, out);
                    encode(value, protocol, out);
                }
            }
        }
        Reply::Double(value) => {
            if resp3 {
                out.push(b',');
                out.extend_from_slice(&double_text(*value));
                out.extend_from_slice(b"\r\n");
            } else {
                encode(&Reply::Bulk(double_text(*value)), protocol, out);
            }
        }
        Reply::ScoredMembers(pairs) => {
            if resp3 {
                // Each member is paired with its score in its own array.
                out.push(b'*');
                out.extend_from_slice(pairs.len().to_string().as_bytes());
                out.extend_from_slice(b"\r\n");
                for (member, score) in pairs {
                    out.extend_from_slice(b"*2\r\n");
                    encode(&Reply::Bulk(member.clone()), protocol, out);
                    encode(&Reply::Double(*score), protocol, out);
                }
            } else {
                out.push(b'*');
                out.extend_from_slice((pairs.len() * 2).to_string().as_bytes());
                out.extend_from_slice(b"\r\n");
                for (member, score) in pairs {
                    encode(&Reply::Bulk(member.clone()), protocol, out);
                    encode(&Reply::Bulk(double_text(*score)), protocol, out);
                }
            }
        }
    }
}

fn encode_aggregate(marker: u8, items: &[Reply], protocol: Protocol, out: &mut Vec<u8>) {
    out.push(marker);
    out.extend_from_slice(items.len().to_string().as_bytes());
    out.extend_from_slice(b"\r\n");
    for item in items {
        encode(item, protocol, out);
    }
}

/// RESP3's double type spells the infinities `inf`/`-inf`, and Redis
/// prints finite scores in their shortest round-trip form.
fn double_text(value: f64) -> Bytes {
    crate::util::bytes::format_f64(value)
}

/// A malformed request. The connection is closed after reporting one,
/// because the parser can no longer tell where the next command starts.
#[derive(Debug, PartialEq)]
pub struct ProtocolError(pub String);

/// One parsed command plus how many bytes of the buffer it used.
#[derive(Debug, PartialEq)]
pub struct Request {
    pub argv: Vec<Bytes>,
    pub consumed: usize,
}

/// Finds the next line at or after `from`, returning its contents and
/// the offset just past the terminator.
///
/// Both `\r\n` and a bare `\n` end a line. Redis is lenient here too,
/// and it is what makes `echo PING | nc host port` work - a shell will
/// not send the carriage return.
fn read_line(buf: &[u8], from: usize) -> Option<(&[u8], usize)> {
    let newline = buf[from..].iter().position(|&b| b == b'\n')? + from;
    let end = if newline > from && buf[newline - 1] == b'\r' {
        newline - 1
    } else {
        newline
    };
    Some((&buf[from..end], newline + 1))
}

/// How many bytes terminate the payload at `at`: 2 for `\r\n`, 1 for a
/// bare `\n`, or `None` if the terminator has not arrived yet.
fn terminator_len(buf: &[u8], at: usize) -> Option<usize> {
    match buf.get(at)? {
        b'\r' => match buf.get(at + 1)? {
            b'\n' => Some(2),
            _ => Some(1),
        },
        b'\n' => Some(1),
        // Not a terminator at all; treat it as one byte so the caller
        // moves on rather than looping.
        _ => Some(1),
    }
}

fn parse_integer(line: &[u8]) -> Option<i64> {
    std::str::from_utf8(line).ok()?.trim().parse().ok()
}

/// Pulls one command off the front of `buf`.
///
/// `Ok(None)` means the buffer holds only part of a command and the
/// caller should read more; nothing is consumed in that case.
pub fn parse_request(buf: &[u8], max_bulk_len: usize) -> Result<Option<Request>, ProtocolError> {
    if buf.is_empty() {
        return Ok(None);
    }
    if buf[0] == b'*' {
        parse_array_request(buf, max_bulk_len)
    } else {
        parse_inline_request(buf)
    }
}

fn parse_array_request(buf: &[u8], max_bulk_len: usize) -> Result<Option<Request>, ProtocolError> {
    let Some((header, mut at)) = read_line(buf, 1) else {
        return Ok(None);
    };
    let Some(count) = parse_integer(header) else {
        return Err(ProtocolError(
            "ERR Protocol error: invalid multibulk length".into(),
        ));
    };
    if count <= 0 {
        // An empty or null array is a no-op, not an error: clients send
        // one after a failed pipeline reset.
        return Ok(Some(Request {
            argv: Vec::new(),
            consumed: at,
        }));
    }
    if count > 1024 * 1024 {
        return Err(ProtocolError(
            "ERR Protocol error: invalid multibulk length".into(),
        ));
    }

    let mut argv = Vec::with_capacity(count as usize);
    for _ in 0..count {
        if at >= buf.len() {
            return Ok(None);
        }
        if buf[at] != b'$' {
            return Err(ProtocolError(format!(
                "ERR Protocol error: expected '$', got '{}'",
                buf[at] as char
            )));
        }
        let Some((header, after_header)) = read_line(buf, at + 1) else {
            return Ok(None);
        };
        let Some(len) = parse_integer(header) else {
            return Err(ProtocolError(
                "ERR Protocol error: invalid bulk length".into(),
            ));
        };
        if len < 0 || len as usize > max_bulk_len {
            return Err(ProtocolError(
                "ERR Protocol error: invalid bulk length".into(),
            ));
        }
        let len = len as usize;
        // The payload plus its terminator must all have arrived.
        if after_header + len >= buf.len() {
            return Ok(None);
        }
        let Some(terminator) = terminator_len(buf, after_header + len) else {
            return Ok(None);
        };
        if after_header + len + terminator > buf.len() {
            return Ok(None);
        }
        argv.push(buf[after_header..after_header + len].to_vec());
        at = after_header + len + terminator;
    }

    Ok(Some(Request { argv, consumed: at }))
}

/// Splits a bare line into arguments, honouring single and double
/// quotes so a value with spaces can still be typed by hand. Redis's
/// inline parser does the same; the escape handling here covers the
/// common `\n`/`\r`/`\t`/`\\`/`\"` cases rather than every one Redis
/// supports.
fn parse_inline_request(buf: &[u8]) -> Result<Option<Request>, ProtocolError> {
    let Some((line, consumed)) = read_line(buf, 0) else {
        // Guard against a peer that never sends a newline.
        if buf.len() > 64 * 1024 {
            return Err(ProtocolError(
                "ERR Protocol error: too big inline request".into(),
            ));
        }
        return Ok(None);
    };

    let mut argv: Vec<Bytes> = Vec::new();
    let mut current: Bytes = Vec::new();
    let mut in_token = false;
    let mut quote: Option<u8> = None;
    let mut i = 0;

    while i < line.len() {
        let byte = line[i];
        match quote {
            Some(q) => {
                if byte == b'\\' && q == b'"' && i + 1 < line.len() {
                    i += 1;
                    current.push(match line[i] {
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        other => other,
                    });
                } else if byte == q {
                    quote = None;
                } else {
                    current.push(byte);
                }
            }
            None => {
                if byte == b'"' || byte == b'\'' {
                    quote = Some(byte);
                    in_token = true;
                } else if byte.is_ascii_whitespace() {
                    if in_token {
                        argv.push(std::mem::take(&mut current));
                        in_token = false;
                    }
                } else {
                    current.push(byte);
                    in_token = true;
                }
            }
        }
        i += 1;
    }

    if quote.is_some() {
        return Err(ProtocolError(
            "ERR Protocol error: unbalanced quotes in request".into(),
        ));
    }
    if in_token {
        argv.push(current);
    }
    Ok(Some(Request { argv, consumed }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(reply: Reply) -> String {
        let mut out = Vec::new();
        encode(&reply, Protocol::Resp2, &mut out);
        String::from_utf8(out).unwrap()
    }

    fn encoded3(reply: Reply) -> String {
        let mut out = Vec::new();
        encode(&reply, Protocol::Resp3, &mut out);
        String::from_utf8(out).unwrap()
    }

    fn parse(input: &str) -> Result<Option<Request>, ProtocolError> {
        parse_request(input.as_bytes(), 512 * 1024 * 1024)
    }

    fn argv_of(input: &str) -> Vec<String> {
        parse(input)
            .unwrap()
            .unwrap()
            .argv
            .into_iter()
            .map(|a| String::from_utf8(a).unwrap())
            .collect()
    }

    #[test]
    fn encodes_every_reply_type() {
        assert_eq!(encoded(Reply::ok()), "+OK\r\n");
        assert_eq!(encoded(Reply::error("ERR nope")), "-ERR nope\r\n");
        assert_eq!(encoded(Reply::Integer(-42)), ":-42\r\n");
        assert_eq!(encoded(Reply::bulk("hello")), "$5\r\nhello\r\n");
        assert_eq!(encoded(Reply::bulk("")), "$0\r\n\r\n");
        assert_eq!(encoded(Reply::Nil), "$-1\r\n");
        assert_eq!(encoded(Reply::NilArray), "*-1\r\n");
        assert_eq!(encoded(Reply::Array(vec![])), "*0\r\n");
        assert_eq!(
            encoded(Reply::Array(vec![Reply::Integer(1), Reply::bulk("a")])),
            "*2\r\n:1\r\n$1\r\na\r\n"
        );
    }

    #[test]
    fn resp3_types_fall_back_to_resp2_spellings() {
        let map = Reply::Map(vec![(Reply::bulk("a"), Reply::Integer(1))]);
        assert_eq!(encoded(map.clone()), "*2\r\n$1\r\na\r\n:1\r\n");
        assert_eq!(encoded3(map), "%1\r\n$1\r\na\r\n:1\r\n");

        let set = Reply::Set(vec![Reply::bulk("a")]);
        assert_eq!(encoded(set.clone()), "*1\r\n$1\r\na\r\n");
        assert_eq!(encoded3(set), "~1\r\n$1\r\na\r\n");

        assert_eq!(encoded(Reply::Double(1.5)), "$3\r\n1.5\r\n");
        assert_eq!(encoded3(Reply::Double(1.5)), ",1.5\r\n");

        assert_eq!(encoded(Reply::Nil), "$-1\r\n");
        assert_eq!(encoded3(Reply::Nil), "_\r\n");
        assert_eq!(encoded3(Reply::NilArray), "_\r\n");
    }

    #[test]
    fn a_push_is_marked_only_in_resp3() {
        let message = Reply::push("message", vec![Reply::bulk("news"), Reply::bulk("hi")]);
        assert_eq!(
            encoded(message.clone()),
            "*3\r\n$7\r\nmessage\r\n$4\r\nnews\r\n$2\r\nhi\r\n"
        );
        assert_eq!(
            encoded3(message),
            ">3\r\n$7\r\nmessage\r\n$4\r\nnews\r\n$2\r\nhi\r\n"
        );
    }

    #[test]
    fn scored_members_flatten_only_in_resp2() {
        let pairs = Reply::ScoredMembers(vec![(b"bob".to_vec(), 50.0)]);
        assert_eq!(encoded(pairs.clone()), "*2\r\n$3\r\nbob\r\n$2\r\n50\r\n");
        assert_eq!(encoded3(pairs), "*1\r\n*2\r\n$3\r\nbob\r\n,50\r\n");
    }

    #[test]
    fn bulk_strings_carry_arbitrary_bytes() {
        // The whole point of the rewrite: a value may hold spaces,
        // newlines, and NUL bytes.
        let payload = b"a b\r\nc\0d".to_vec();
        assert_eq!(
            encoded(Reply::Bulk(payload.clone())),
            "$8\r\na b\r\nc\0d\r\n"
        );
    }

    #[test]
    fn an_error_never_breaks_its_frame() {
        assert_eq!(encoded(Reply::error("bad\r\nvalue")), "-bad  value\r\n");
    }

    #[test]
    fn parses_an_array_request() {
        let request = parse("*2\r\n$3\r\nGET\r\n$3\r\nfoo\r\n").unwrap().unwrap();
        assert_eq!(request.argv, vec![b"GET".to_vec(), b"foo".to_vec()]);
        assert_eq!(request.consumed, 22);
    }

    #[test]
    fn parses_a_binary_payload() {
        let request = parse("*2\r\n$3\r\nSET\r\n$3\r\na\r\n\r\n")
            .unwrap()
            .unwrap();
        assert_eq!(request.argv[1], b"a\r\n".to_vec());
    }

    #[test]
    fn an_incomplete_request_consumes_nothing() {
        for partial in [
            "*2\r\n",
            "*2\r\n$3\r\n",
            "*2\r\n$3\r\nGET\r\n",
            "*2\r\n$3\r\nGET\r\n$3\r\nfo",
            "*2\r\n$3\r\nGET\r\n$3\r\nfoo\r",
        ] {
            assert_eq!(parse(partial), Ok(None), "for {partial:?}");
        }
    }

    #[test]
    fn rejects_malformed_frames() {
        assert!(parse("*2\r\n+GET\r\n").is_err());
        assert!(parse("*abc\r\n").is_err());
        assert!(parse("*1\r\n$abc\r\n").is_err());
        assert!(parse("*1\r\n$-5\r\n").is_err());
    }

    #[test]
    fn rejects_a_bulk_string_over_the_limit() {
        assert!(parse_request(b"*1\r\n$100\r\n", 10).is_err());
    }

    #[test]
    fn an_empty_array_is_a_no_op() {
        let request = parse("*0\r\n").unwrap().unwrap();
        assert!(request.argv.is_empty());
        assert_eq!(request.consumed, 4);
    }

    #[test]
    fn parses_an_inline_command() {
        assert_eq!(argv_of("PING\r\n"), vec!["PING"]);
        assert_eq!(argv_of("SET  k   v\r\n"), vec!["SET", "k", "v"]);
        assert_eq!(argv_of("\r\n"), Vec::<String>::new());
    }

    #[test]
    fn inline_quotes_keep_spaces_together() {
        assert_eq!(
            argv_of("SET k \"hello world\"\r\n"),
            vec!["SET", "k", "hello world"]
        );
        assert_eq!(argv_of("SET k 'a b'\r\n"), vec!["SET", "k", "a b"]);
        assert_eq!(argv_of("SET k \"\"\r\n"), vec!["SET", "k", ""]);
    }

    #[test]
    fn inline_escapes_are_understood_inside_double_quotes() {
        assert_eq!(argv_of("SET k \"a\\nb\"\r\n"), vec!["SET", "k", "a\nb"]);
        // Only double quotes interpret escapes, matching Redis.
        assert_eq!(argv_of("SET k 'a\\nb'\r\n"), vec!["SET", "k", "a\\nb"]);
    }

    #[test]
    fn inline_rejects_unbalanced_quotes() {
        assert!(parse("SET k \"unclosed\r\n").is_err());
    }

    #[test]
    fn an_inline_command_needs_its_newline() {
        assert_eq!(parse("PING"), Ok(None));
    }

    #[test]
    fn a_bare_newline_terminates_a_line() {
        // `echo PING | nc host port` sends no carriage return.
        assert_eq!(argv_of("PING\n"), vec!["PING"]);
        assert_eq!(argv_of("SET k v\n"), vec!["SET", "k", "v"]);
        let request = parse("*2\n$3\nGET\n$3\nfoo\n").unwrap().unwrap();
        assert_eq!(request.argv, vec![b"GET".to_vec(), b"foo".to_vec()]);
        assert_eq!(request.consumed, 17);
    }
}
