//! Memory index commands - the `MEM.*` family.
//!
//! The dotted prefix follows the convention Redis modules use (`FT.`,
//! `JSON.`, `TS.`), so any Redis client reaches these through the
//! generic "send this command" call it already has. There is no second
//! port, no HTTP stack, and nothing for a client to install.
//!
//! Argument parsing is table-driven through [`Args`]: every command is
//! `MEM.VERB key` followed by keyword-tagged options in any order.
//! Repeated options (`META`, `FILTER`) each take a fixed number of
//! words, so the grammar never depends on where an option sits.

use std::time::Duration;

use super::{exact_args, min_args, parse_float, parse_int, syntax_error, Checked};
use crate::app::App;
use crate::resp::Reply;
use crate::store::StoreType;
use crate::types::memory::filter::{Clause, Field, Filter, Op};
use crate::types::memory::fuse::Fusion;
use crate::types::memory::record::MemoryRecord;
use crate::types::memory::vector::{encode_le_f32, parse_le_f32, Metric, VectorError};
use crate::types::memory::{
    unix_millis, AddRequest, Hit, Memory, MemoryConfig, MemoryError, Mode, Weights,
};
use crate::util::bytes::{eq_ignore_case, to_display, Bytes};

pub fn dispatch(app: &mut App, name: &str, argv: &[Bytes]) -> Reply {
    match handle(app, name, argv) {
        Ok(reply) | Err(reply) => reply,
    }
}

// --- errors -------------------------------------------------------

fn no_such_index() -> Reply {
    Reply::error("ERR no such memory index")
}

fn already_exists() -> Reply {
    Reply::error("ERR memory index already exists")
}

fn describe(error: MemoryError) -> Reply {
    Reply::error(match error {
        MemoryError::VectorsNotSupported => {
            "ERR this memory index stores no vectors; create it with MODE VECTOR or MODE HYBRID"
                .to_string()
        }
        MemoryError::TextNotSupported => {
            "ERR this memory index does not index text; create it with MODE SEARCH or MODE HYBRID"
                .to_string()
        }
        MemoryError::VectorRequired => {
            "ERR a VECTOR-mode index needs a vector on every record".to_string()
        }
        MemoryError::NoSuchRecord => "ERR no such record".to_string(),
        MemoryError::TooLargeToScan { records, limit } => format!(
            "ERR a scan of {} vectors exceeds mem-max-scan ({}); raise it or narrow the query with FILTER",
            records, limit
        ),
        MemoryError::ExistenceUnmet => "ERR record existence condition not met".to_string(),
        MemoryError::Full { limit } => {
            format!("ERR memory index is full at mem-max-records ({})", limit)
        }
        MemoryError::Vector(e) => match e {
            VectorError::NotFloats => {
                "ERR vector is not float32 bytes; its length must be a multiple of 4".to_string()
            }
            VectorError::WrongDimension { expected, got } => {
                format!("ERR wrong vector dimension, expected {} got {}", expected, got)
            }
            VectorError::NotFinite => "ERR vector contains NaN or infinity".to_string(),
            VectorError::ZeroVector => {
                "ERR a zero vector has no direction to compare under COSINE".to_string()
            }
        },
    })
}

// --- argument parsing ---------------------------------------------

/// A cursor over the option words that follow `MEM.VERB key`.
struct Args<'a> {
    argv: &'a [Bytes],
    at: usize,
}

impl<'a> Args<'a> {
    /// Options start after the command name and the key.
    fn new(argv: &'a [Bytes]) -> Args<'a> {
        Args { argv, at: 2 }
    }

    fn done(&self) -> bool {
        self.at >= self.argv.len()
    }

    fn peek(&self) -> &'a [u8] {
        &self.argv[self.at]
    }

    /// Consumes the current word if it is `word`.
    fn take(&mut self, word: &str) -> bool {
        if !self.done() && eq_ignore_case(self.peek(), word) {
            self.at += 1;
            return true;
        }
        false
    }

    /// The next `n` words, or a syntax error if the option was cut off.
    fn values(&mut self, n: usize) -> Checked<&'a [Bytes]> {
        if self.at + n > self.argv.len() {
            return Err(syntax_error());
        }
        let slice = &self.argv[self.at..self.at + n];
        self.at += n;
        Ok(slice)
    }

    fn value(&mut self) -> Checked<&'a [u8]> {
        Ok(&self.values(1)?[0])
    }

    fn unexpected(&self) -> Reply {
        Reply::error(format!(
            "ERR unexpected argument '{}'",
            to_display(self.peek())
        ))
    }
}

/// Reads `VEC <blob>` or `FVEC <n> <f1> .. <fn>`.
///
/// `VEC` is the one clients should use: a raw little-endian float32
/// blob is four bytes per dimension and is exactly what `struct.pack`
/// or a `Float32Array` already holds. `FVEC` spells the same vector as
/// decimal words so a query can be typed into `redis-cli`.
fn read_vector(args: &mut Args) -> Checked<Option<Vec<f32>>> {
    if args.take("VEC") {
        let blob = args.value()?;
        return match parse_le_f32(blob) {
            Ok(values) => Ok(Some(values)),
            Err(e) => Err(describe(MemoryError::Vector(e))),
        };
    }
    if args.take("FVEC") {
        let count = parse_int(args.value()?)?;
        if count < 0 {
            return Err(syntax_error());
        }
        let words = args.values(count as usize)?;
        let mut values = Vec::with_capacity(words.len());
        for word in words {
            let value = parse_float(word)? as f32;
            if !value.is_finite() {
                return Err(describe(MemoryError::Vector(VectorError::NotFinite)));
            }
            values.push(value);
        }
        return Ok(Some(values));
    }
    Ok(None)
}

/// Reads a repeated `FILTER field op value` clause.
fn read_filter_clause(args: &mut Args, filter: &mut Filter) -> Checked<bool> {
    if !args.take("FILTER") {
        return Ok(false);
    }
    let words = args.values(3)?;
    let (Some(field), Some(op)) = (Field::parse(&words[0]), Op::parse(&words[1])) else {
        return Err(Reply::error(format!(
            "ERR bad filter clause '{} {}'",
            to_display(&words[0]),
            to_display(&words[1])
        )));
    };
    filter.push(Clause {
        field,
        op,
        value: words[2].clone(),
    });
    Ok(true)
}

/// The options every retrieval command shares.
struct Retrieval {
    topk: usize,
    /// How many candidates each index contributes. Above `topk`,
    /// because fusion can only reorder what it is given.
    candidates: usize,
    filter: Filter,
    returns: Returns,
}

impl Retrieval {
    fn new(app: &App) -> Retrieval {
        Retrieval {
            topk: 10,
            candidates: app.config.mem_max_candidates.max(10),
            filter: Filter::default(),
            returns: Returns::default(),
        }
    }

    /// Consumes one shared option, or reports the word that isn't one.
    fn read(&mut self, args: &mut Args, app: &App) -> Checked<()> {
        if args.take("TOPK") {
            let n = parse_int(args.value()?)?;
            if n < 1 || n as usize > app.config.mem_max_topk {
                return Err(Reply::error(format!(
                    "ERR TOPK must be between 1 and mem-max-topk ({})",
                    app.config.mem_max_topk
                )));
            }
            self.topk = n as usize;
            self.candidates = app.config.mem_max_candidates.max(self.topk);
            return Ok(());
        }
        if self.returns.read(args) || read_filter_clause(args, &mut self.filter)? {
            return Ok(());
        }
        Err(args.unexpected())
    }
}

/// Renders a ranked result set. `score` sits right after `id` so the
/// two fields a caller always reads come first.
fn hits_reply(memory: &Memory, hits: Vec<Hit>, topk: usize, returns: Returns) -> Reply {
    let replies = hits
        .into_iter()
        .take(topk)
        .filter_map(|hit| {
            let record = memory.get(&hit.id)?;
            let Reply::Map(mut fields) = record_reply(memory, record, returns) else {
                return None;
            };
            fields.insert(1, (Reply::bulk("score"), double(hit.score)));
            if returns.scores {
                fields.push((Reply::bulk("keyword_score"), double(hit.keyword)));
                fields.push((Reply::bulk("vector_score"), double(hit.vector)));
                fields.push((Reply::bulk("recency_score"), double(hit.recency)));
            }
            Some(Reply::Map(fields))
        })
        .collect();
    Reply::array(replies)
}

/// Which parts of a record a reply should carry.
#[derive(Default, Clone, Copy)]
struct Returns {
    no_text: bool,
    meta: bool,
    vector: bool,
    scores: bool,
}

impl Returns {
    /// Reads one of the shared return flags, if the cursor is on one.
    fn read(&mut self, args: &mut Args) -> bool {
        if args.take("NOTEXT") {
            self.no_text = true;
        } else if args.take("WITHMETA") {
            self.meta = true;
        } else if args.take("WITHVEC") {
            self.vector = true;
        } else if args.take("WITHSCORES") {
            self.scores = true;
        } else {
            return false;
        }
        true
    }
}

// --- replies ------------------------------------------------------

/// Widens an `f32` for a RESP double without dragging its binary
/// representation along: `0.85f32 as f64` is 0.8500000238418579, which
/// is what a client would then see. Going through the shortest text
/// that round-trips the `f32` gives back the number the client sent.
fn double(value: f32) -> Reply {
    Reply::Double(format!("{}", value).parse().unwrap_or(value as f64))
}

fn meta_reply(record: &MemoryRecord) -> Reply {
    Reply::Map(
        record
            .meta()
            .iter()
            .map(|(field, value)| (Reply::bulk(field.clone()), Reply::bulk(value.clone())))
            .collect(),
    )
}

/// One record as a map. RESP3 clients see a dictionary and RESP2
/// clients the flat array they already unpack, both from `Reply::Map`.
fn record_reply(memory: &Memory, record: &MemoryRecord, returns: Returns) -> Reply {
    let mut fields = vec![(Reply::bulk("id"), Reply::bulk(record.id.clone()))];
    if !returns.no_text {
        fields.push((Reply::bulk("text"), Reply::bulk(record.text.clone())));
    }
    fields.push((Reply::bulk("importance"), double(record.importance)));
    fields.push((
        Reply::bulk("created_at"),
        Reply::Integer(unix_millis(record.created_at)),
    ));
    fields.push((
        Reply::bulk("updated_at"),
        Reply::Integer(unix_millis(record.updated_at)),
    ));
    fields.push((
        Reply::bulk("pttl"),
        Reply::Integer(memory.record_pttl(&record.id)),
    ));
    if returns.meta {
        fields.push((Reply::bulk("meta"), meta_reply(record)));
    }
    if returns.vector {
        let vector = memory
            .vector_of(record)
            .map_or(Reply::Nil, |values| Reply::bulk(encode_le_f32(values)));
        fields.push((Reply::bulk("vector"), vector));
    }
    Reply::Map(fields)
}

// --- dispatch -----------------------------------------------------

fn handle(app: &mut App, name: &str, argv: &[Bytes]) -> Checked<Reply> {
    match name {
        "MEM.CREATE" => create(app, argv),
        "MEM.INFO" => info(app, argv),
        "MEM.CONFIG" => config(app, argv),
        "MEM.CARD" => card(app, argv),
        "MEM.ADD" => add(app, argv),
        "MEM.GET" | "MEM.MGET" => get(app, name, argv),
        "MEM.DEL" => del(app, argv),
        "MEM.SETMETA" => set_meta(app, argv),
        "MEM.DELMETA" => del_meta(app, argv),
        "MEM.EXPIRE" => expire(app, argv),
        "MEM.SCAN" => scan(app, argv),
        "MEM.SEARCH" => search(app, argv),
        "MEM.VSEARCH" => vsearch(app, argv),
        "MEM.QUERY" => query(app, argv),
        _ => Ok(Reply::error("ERR unknown command")),
    }
}

/// Looks the index up, distinguishing "missing" from "wrong type" the
/// way every other Klyro command does.
fn index<'a>(app: &'a mut App, key: &[u8]) -> Checked<&'a mut Memory> {
    match app.store.peek_type(key) {
        Some(StoreType::Memory) => Ok(app.store.get_existing_memory(key).expect("type checked")),
        Some(_) => Err(super::wrongtype()),
        None => Err(no_such_index()),
    }
}

fn create(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "MEM.CREATE", 1)?;
    let mut mode = Mode::Hybrid;
    let mut dim: Option<usize> = None;
    let mut metric = Metric::Cosine;
    let mut weights: Option<Weights> = None;
    let mut half_life = Duration::from_secs(app.config.mem_recency_halflife);

    let mut args = Args::new(argv);
    while !args.done() {
        if args.take("MODE") {
            mode = Mode::parse(args.value()?).ok_or_else(syntax_error)?;
        } else if args.take("DIM") {
            let n = parse_int(args.value()?)?;
            if n < 1 || n as usize > app.config.mem_max_dim {
                return Err(Reply::error(format!(
                    "ERR DIM must be between 1 and mem-max-dim ({})",
                    app.config.mem_max_dim
                )));
            }
            dim = Some(n as usize);
        } else if args.take("METRIC") {
            metric = Metric::parse(args.value()?).ok_or_else(syntax_error)?;
        } else if args.take("WEIGHTS") {
            let words = args.values(4)?;
            let mut parsed = [0f32; 4];
            for (slot, word) in parsed.iter_mut().zip(words) {
                *slot = parse_float(word)? as f32;
            }
            weights = Some(Weights {
                keyword: parsed[0],
                vector: parsed[1],
                recency: parsed[2],
                importance: parsed[3],
            });
        } else if args.take("HALFLIFE") {
            let seconds = parse_int(args.value()?)?;
            if seconds < 1 {
                return Err(Reply::error("ERR HALFLIFE must be positive"));
            }
            half_life = Duration::from_secs(seconds as u64);
        } else {
            return Err(args.unexpected());
        }
    }

    if mode.stores_vectors() && dim.is_none() {
        return Err(Reply::error(
            "ERR DIM is required for a VECTOR or HYBRID index",
        ));
    }
    // A DIM on a keyword-only index is almost always a client that
    // meant HYBRID. Silently ignoring it would leave MEM.INFO
    // reporting 0 against a number they passed, which hides the slip
    // until a MEM.ADD is rejected for carrying a vector.
    if !mode.stores_vectors() && dim.is_some() {
        return Err(Reply::error(
            "ERR a SEARCH index stores no vectors, so DIM does not apply; did you mean MODE HYBRID?",
        ));
    }
    if let Some(weights) = weights {
        if !weights.is_valid() {
            return Err(Reply::error("ERR WEIGHTS must be finite and non-negative"));
        }
    }

    let mut config = MemoryConfig::new(mode, dim.unwrap_or(0), metric);
    config.half_life = half_life;
    if let Some(weights) = weights {
        config.weights = weights;
    }

    if app.store.peek_type(&argv[1]).is_some() {
        return Err(match app.store.peek_type(&argv[1]) {
            Some(StoreType::Memory) => already_exists(),
            _ => super::wrongtype(),
        });
    }
    app.store.create_memory(&argv[1], Memory::new(config));
    Ok(Reply::ok())
}

fn info(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    exact_args(argv, "MEM.INFO", 1)?;
    let memory = index(app, &argv[1])?;
    let config = memory.config().clone();
    Ok(Reply::Map(vec![
        (Reply::bulk("mode"), Reply::bulk(config.mode.name())),
        (Reply::bulk("dim"), Reply::Integer(config.dim as i64)),
        (Reply::bulk("metric"), Reply::bulk(config.metric.name())),
        (
            Reply::bulk("weights"),
            Reply::Map(vec![
                (Reply::bulk("keyword"), double(config.weights.keyword)),
                (Reply::bulk("vector"), double(config.weights.vector)),
                (Reply::bulk("recency"), double(config.weights.recency)),
                (Reply::bulk("importance"), double(config.weights.importance)),
            ]),
        ),
        (
            Reply::bulk("halflife"),
            Reply::Integer(config.half_life.as_secs() as i64),
        ),
        (Reply::bulk("records"), Reply::Integer(memory.len() as i64)),
        (
            Reply::bulk("vectors"),
            Reply::Integer(memory.vector_count() as i64),
        ),
        (
            Reply::bulk("terms"),
            Reply::Integer(memory.term_count() as i64),
        ),
        (Reply::bulk("avg_doc_len"), double(memory.avg_doc_len())),
        (
            Reply::bulk("bytes"),
            Reply::Integer(memory.heap_bytes() as i64),
        ),
    ]))
}

fn config(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "MEM.CONFIG", 2)?;
    // Mode, dimension, and metric are absent on purpose: every stored
    // vector is laid out against them and every posting was built under
    // them, so changing one would invalidate the index rather than
    // reconfigure it.
    let mut args = Args::new(argv);
    let mut weights: Option<Weights> = None;
    let mut half_life: Option<Duration> = None;
    while !args.done() {
        if args.take("WEIGHTS") {
            let words = args.values(4)?;
            let mut parsed = [0f32; 4];
            for (slot, word) in parsed.iter_mut().zip(words) {
                *slot = parse_float(word)? as f32;
            }
            let candidate = Weights {
                keyword: parsed[0],
                vector: parsed[1],
                recency: parsed[2],
                importance: parsed[3],
            };
            if !candidate.is_valid() {
                return Err(Reply::error("ERR WEIGHTS must be finite and non-negative"));
            }
            weights = Some(candidate);
        } else if args.take("HALFLIFE") {
            let seconds = parse_int(args.value()?)?;
            if seconds < 1 {
                return Err(Reply::error("ERR HALFLIFE must be positive"));
            }
            half_life = Some(Duration::from_secs(seconds as u64));
        } else {
            return Err(args.unexpected());
        }
    }

    let memory = index(app, &argv[1])?;
    if let Some(weights) = weights {
        memory.set_weights(weights);
    }
    if let Some(half_life) = half_life {
        memory.set_half_life(half_life);
    }
    app.store.mark_dirty();
    Ok(Reply::ok())
}

fn card(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    exact_args(argv, "MEM.CARD", 1)?;
    let memory = index(app, &argv[1])?;
    Ok(Reply::Integer(memory.len() as i64))
}

fn add(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "MEM.ADD", 1)?;
    let mut request = AddRequest::default();
    let mut has_text = false;
    let mut args = Args::new(argv);

    while !args.done() {
        if args.take("ID") {
            request.id = Some(args.value()?.to_vec());
        } else if args.take("TEXT") {
            let text = args.value()?;
            if text.len() > app.config.mem_max_text_bytes {
                return Err(Reply::error(format!(
                    "ERR text exceeds mem-max-text-bytes ({})",
                    app.config.mem_max_text_bytes
                )));
            }
            request.text = text.to_vec();
            has_text = true;
        } else if args.take("META") {
            let words = args.values(2)?;
            request.meta.push((words[0].clone(), words[1].clone()));
        } else if args.take("IMPORTANCE") {
            let value = parse_float(args.value()?)? as f32;
            if !(0.0..=1.0).contains(&value) {
                return Err(Reply::error("ERR IMPORTANCE must be between 0 and 1"));
            }
            request.importance = Some(value);
        } else if args.take("TTL") {
            let seconds = parse_int(args.value()?)?;
            if seconds < 1 {
                return Err(Reply::error("ERR TTL must be positive"));
            }
            request.ttl = Some(Duration::from_secs(seconds as u64));
        } else if args.take("NX") {
            request.require_new = true;
        } else if args.take("XX") {
            request.require_existing = true;
        } else {
            match read_vector(&mut args)? {
                Some(values) => request.vector = Some(values),
                None => return Err(args.unexpected()),
            }
        }
    }

    if !has_text {
        return Err(Reply::error("ERR TEXT is required"));
    }
    if request.require_new && request.require_existing {
        return Err(Reply::error("ERR NX and XX are mutually exclusive"));
    }

    let (max_terms, max_records) = (app.config.mem_max_terms_per_doc, app.config.mem_max_records);
    let memory = index(app, &argv[1])?;
    match memory.add(request, max_terms, max_records) {
        Ok(id) => {
            app.store.mark_dirty();
            Ok(Reply::Bulk(id))
        }
        Err(e) => Err(describe(e)),
    }
}

fn get(app: &mut App, name: &str, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, name, 2)?;
    let single = name == "MEM.GET";
    // MEM.GET takes one id then flags; MEM.MGET takes only ids, so that
    // an id can never be mistaken for a flag.
    let (ids, flag_start) = if single {
        (&argv[2..3], 3)
    } else {
        (&argv[2..], argv.len())
    };

    let mut returns = Returns::default();
    let mut args = Args {
        argv,
        at: flag_start,
    };
    while !args.done() {
        if !returns.read(&mut args) {
            return Err(args.unexpected());
        }
    }

    let memory = index(app, &argv[1])?;
    let replies: Vec<Reply> = ids
        .iter()
        .map(|id| match memory.get(id) {
            Some(record) => record_reply(memory, record, returns),
            None => Reply::Nil,
        })
        .collect();

    Ok(if single {
        replies.into_iter().next().unwrap_or(Reply::Nil)
    } else {
        Reply::array(replies)
    })
}

fn del(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "MEM.DEL", 2)?;
    let max_terms = app.config.mem_max_terms_per_doc;
    let memory = index(app, &argv[1])?;
    let removed = argv[2..]
        .iter()
        .filter(|id| memory.del(id, max_terms))
        .count();
    app.store.mark_dirty();
    Ok(Reply::Integer(removed as i64))
}

fn set_meta(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "MEM.SETMETA", 3)?;
    if !argv[3..].len().is_multiple_of(2) {
        return Err(Reply::wrong_arity("MEM.SETMETA"));
    }
    let pairs: Vec<(Bytes, Bytes)> = argv[3..]
        .chunks(2)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect();
    let memory = index(app, &argv[1])?;
    match memory.set_meta(&argv[2], pairs) {
        Ok(added) => {
            app.store.mark_dirty();
            Ok(Reply::Integer(added as i64))
        }
        Err(e) => Err(describe(e)),
    }
}

fn del_meta(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "MEM.DELMETA", 3)?;
    let fields = argv[3..].to_vec();
    let memory = index(app, &argv[1])?;
    match memory.remove_meta(&argv[2], &fields) {
        Ok(removed) => {
            app.store.mark_dirty();
            Ok(Reply::Integer(removed as i64))
        }
        Err(e) => Err(describe(e)),
    }
}

fn expire(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    exact_args(argv, "MEM.EXPIRE", 3)?;
    let seconds = parse_int(&argv[3])?;
    // 0 clears the deadline, matching how PERSIST relates to EXPIRE.
    let ttl = match seconds {
        0 => None,
        n if n > 0 => Some(Duration::from_secs(n as u64)),
        _ => return Err(Reply::error("ERR TTL must not be negative")),
    };
    let memory = index(app, &argv[1])?;
    match memory.set_record_ttl(&argv[2], ttl) {
        Ok(()) => {
            app.store.mark_dirty();
            Ok(Reply::bool(true))
        }
        Err(MemoryError::NoSuchRecord) => Ok(Reply::bool(false)),
        Err(e) => Err(describe(e)),
    }
}

fn scan(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "MEM.SCAN", 2)?;
    let cursor = parse_int(&argv[2])?;
    if cursor < 0 {
        return Err(Reply::error("ERR invalid cursor"));
    }
    let mut count = app.config.scan_default_count;
    let mut filter = Filter::default();
    let mut args = Args { argv, at: 3 };
    while !args.done() {
        if args.take("COUNT") {
            let n = parse_int(args.value()?)?;
            if n < 1 {
                return Err(syntax_error());
            }
            count = n as usize;
        } else if !read_filter_clause(&mut args, &mut filter)? {
            return Err(args.unexpected());
        }
    }

    let memory = index(app, &argv[1])?;
    let (ids, next) = memory.scan(cursor as usize, count, &filter);
    Ok(Reply::array(vec![
        Reply::bulk(next.to_string()),
        Reply::bulk_array(ids),
    ]))
}

fn search(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "MEM.SEARCH", 2)?;
    // The query is argv[2]; options follow it.
    let mut options = Retrieval::new(app);
    let mut args = Args { argv, at: 3 };
    while !args.done() {
        options.read(&mut args, app)?;
    }

    let max_terms = app.config.mem_max_terms_per_doc;
    let memory = index(app, &argv[1])?;
    let hits = memory
        .search_text(&argv[2], options.candidates, &options.filter, max_terms)
        .map_err(describe)?;

    let hits: Vec<Hit> = hits
        .into_iter()
        .map(|(id, score)| Hit {
            id,
            score,
            keyword: score,
            vector: 0.0,
            recency: 0.0,
        })
        .collect();
    Ok(hits_reply(memory, hits, options.topk, options.returns))
}

fn vsearch(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "MEM.VSEARCH", 2)?;
    let mut options = Retrieval::new(app);
    let mut args = Args::new(argv);
    let mut vector = None;
    while !args.done() {
        match read_vector(&mut args)? {
            Some(values) => vector = Some(values),
            None => options.read(&mut args, app)?,
        }
    }
    let Some(mut vector) = vector else {
        return Err(Reply::error("ERR VEC or FVEC is required"));
    };

    let max_scan = app.config.mem_max_scan;
    let memory = index(app, &argv[1])?;
    // Under cosine both sides must be unit length, and the stored side
    // already is.
    memory.prepare_query_vector(&mut vector);
    let hits = memory
        .search_vector(&vector, options.candidates, &options.filter, max_scan)
        .map_err(describe)?;
    let hits: Vec<Hit> = hits
        .into_iter()
        .map(|(id, score)| Hit {
            id,
            score,
            keyword: 0.0,
            vector: score,
            recency: 0.0,
        })
        .collect();
    Ok(hits_reply(memory, hits, options.topk, options.returns))
}

fn query(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "MEM.QUERY", 1)?;
    let mut options = Retrieval::new(app);
    let mut text: Option<Bytes> = None;
    let mut vector: Option<Vec<f32>> = None;
    let mut weights: Option<Weights> = None;
    let mut fusion = Fusion::Linear;

    let mut args = Args::new(argv);
    while !args.done() {
        if args.take("TEXT") {
            text = Some(args.value()?.to_vec());
        } else if args.take("WEIGHTS") {
            let words = args.values(4)?;
            let mut parsed = [0f32; 4];
            for (slot, word) in parsed.iter_mut().zip(words) {
                *slot = parse_float(word)? as f32;
            }
            let candidate = Weights {
                keyword: parsed[0],
                vector: parsed[1],
                recency: parsed[2],
                importance: parsed[3],
            };
            if !candidate.is_valid() {
                return Err(Reply::error("ERR WEIGHTS must be finite and non-negative"));
            }
            weights = Some(candidate);
        } else if args.take("FUSION") {
            fusion = Fusion::parse(args.value()?).ok_or_else(syntax_error)?;
        } else {
            match read_vector(&mut args)? {
                Some(values) => vector = Some(values),
                None => options.read(&mut args, app)?,
            }
        }
    }

    if text.is_none() && vector.is_none() {
        return Err(Reply::error("ERR TEXT, VEC, or FVEC is required"));
    }

    let (max_terms, max_scan) = (app.config.mem_max_terms_per_doc, app.config.mem_max_scan);
    let memory = index(app, &argv[1])?;
    if let Some(values) = vector.as_mut() {
        memory.prepare_query_vector(values);
    }
    let hits = memory
        .query(
            text.as_deref(),
            vector.as_deref(),
            options.candidates,
            &options.filter,
            weights.unwrap_or(memory.config().weights),
            fusion,
            max_terms,
            max_scan,
        )
        .map_err(describe)?;
    Ok(hits_reply(memory, hits, options.topk, options.returns))
}
