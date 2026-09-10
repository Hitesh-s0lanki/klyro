//! Saves and loads the whole keyspace to a single dump file.
//!
//! Format version 2 is length-prefixed, because keys and values are now
//! arbitrary bytes and a line-oriented format cannot represent a value
//! containing a newline. Every blob is written as its length on one
//! line, then exactly that many bytes, then a newline:
//!
//! ```text
//! KLYRO-DUMP 2
//! STRING <expire-at-ms>
//! <blob key>
//! <blob value>
//! LIST <expire-at-ms> <count>
//! <blob key>
//! <blob element> * count
//! ```
//!
//! Hashes write a field blob and a value blob per entry, sets one blob
//! per member, and sorted sets a member blob plus a score line.
//! `<expire-at-ms>` is `-1` for a key with no TTL.
//!
//! Version 3 adds the `MEMORY` record for memory indexes. It is
//! otherwise identical to version 2, so one reader handles both and a
//! version 2 dump is simply one that happens to contain no memory
//! index. What a `MEMORY` record stores is the index's configuration
//! and its records - never its inverted index or its vector array,
//! both of which are derivable and are rebuilt on load. Serializing
//! them would roughly double the dump and add a second format to keep
//! in step with the first.
//!
//! Version 1 dumps - the original whitespace-delimited text format -
//! still load, so an existing dump survives the upgrade. They are
//! rewritten as the current version on the next save.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::store::{Store, StoreType};
use crate::types::memory::record::MemoryRecord;
use crate::types::memory::vector::{encode_le_f32, parse_le_f32, Metric};
use crate::types::memory::{from_unix_millis, unix_millis, Memory, MemoryConfig, Mode, Weights};
use crate::util::bytes::{format_f64, parse_f64, Bytes};

const DUMP_MAGIC: &str = "KLYRO-DUMP 3";
/// Version 2 differs only by having no `MEMORY` record, so the same
/// reader loads it.
const V2_MAGIC: &str = "KLYRO-DUMP 2";
const LEGACY_MAGIC: &str = "KLYRO-DUMP 1";

pub struct Persist {
    path: PathBuf,
    last_check: Option<Instant>,
}

/// Reads the length-prefixed records a version 2 dump is made of.
struct DumpReader<R: BufRead> {
    inner: R,
}

impl<R: BufRead> DumpReader<R> {
    /// The next line, without its newline. `None` at end of file.
    fn line(&mut self) -> io::Result<Option<String>> {
        let mut text = String::new();
        if self.inner.read_line(&mut text)? == 0 {
            return Ok(None);
        }
        Ok(Some(text.trim_end_matches(['\r', '\n']).to_string()))
    }

    /// One length-prefixed blob.
    fn blob(&mut self) -> io::Result<Option<Bytes>> {
        let Some(header) = self.line()? else {
            return Ok(None);
        };
        let Ok(len) = header.trim().parse::<usize>() else {
            return Ok(None);
        };
        let mut buf = vec![0u8; len];
        self.inner.read_exact(&mut buf)?;
        let mut newline = [0u8; 1];
        // The trailing newline is a separator, not part of the payload.
        let _ = self.inner.read_exact(&mut newline);
        Ok(Some(buf))
    }

    fn score(&mut self) -> io::Result<Option<f64>> {
        Ok(self.line()?.and_then(|text| parse_f64(text.as_bytes())))
    }
}

fn write_blob(f: &mut File, payload: &[u8]) -> io::Result<()> {
    writeln!(f, "{}", payload.len())?;
    f.write_all(payload)?;
    f.write_all(b"\n")
}

fn unix_millis_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

impl Persist {
    pub fn new(path: &str) -> Self {
        Persist {
            path: PathBuf::from(path),
            last_check: None,
        }
    }

    /// Loads the dump file at the configured path into `store`, if one
    /// exists. Call once at startup, before accepting connections.
    ///
    /// `max_terms` is the tokenizer cap a memory index rebuilds its
    /// keyword index under, so a dump reloads under the running
    /// configuration rather than whatever wrote it.
    pub fn load(&self, store: &mut Store, max_terms: usize) {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return, // no dump yet; nothing to load
        };
        let mut reader = DumpReader {
            inner: BufReader::new(file),
        };

        let magic = match reader.line() {
            Ok(Some(line)) => line,
            _ => return,
        };
        let loaded = if magic.starts_with(DUMP_MAGIC) || magic.starts_with(V2_MAGIC) {
            self.load_v2(&mut reader, max_terms)
        } else if magic.starts_with(LEGACY_MAGIC) {
            self.load_v1(&mut reader)
        } else {
            eprintln!(
                "persist_load: {} is not a Klyro dump, ignoring",
                self.path.display()
            );
            return;
        };

        match loaded {
            Ok(entries) => {
                for entry in entries {
                    entry.install(store);
                }
            }
            Err(e) => eprintln!("persist_load: {}: {}", self.path.display(), e),
        }
    }

    fn load_v2<R: BufRead>(
        &self,
        reader: &mut DumpReader<R>,
        max_terms: usize,
    ) -> io::Result<Vec<LoadedEntry>> {
        let mut entries = Vec::new();
        while let Some(header) = reader.line()? {
            if header.is_empty() {
                continue;
            }
            let mut parts = header.split_whitespace();
            let (Some(kind), Some(expire_at)) = (parts.next(), parts.next()) else {
                continue;
            };
            let expire_at: i64 = expire_at.parse().unwrap_or(-1);
            let count: usize = parts.next().and_then(|c| c.parse().ok()).unwrap_or(0);

            let Some(key) = reader.blob()? else { break };
            let value = match kind {
                "STRING" => {
                    let Some(v) = reader.blob()? else { break };
                    LoadedValue::Str(v)
                }
                "LIST" | "SET" => {
                    let mut items = Vec::with_capacity(count);
                    for _ in 0..count {
                        let Some(item) = reader.blob()? else { break };
                        items.push(item);
                    }
                    if kind == "LIST" {
                        LoadedValue::List(items)
                    } else {
                        LoadedValue::Set(items)
                    }
                }
                "HASH" => {
                    let mut pairs = Vec::with_capacity(count);
                    for _ in 0..count {
                        let (Some(field), Some(value)) = (reader.blob()?, reader.blob()?) else {
                            break;
                        };
                        pairs.push((field, value));
                    }
                    LoadedValue::Hash(pairs)
                }
                "ZSET" => {
                    let mut pairs = Vec::with_capacity(count);
                    for _ in 0..count {
                        let (Some(member), Some(score)) = (reader.blob()?, reader.score()?) else {
                            break;
                        };
                        pairs.push((member, score));
                    }
                    LoadedValue::Zset(pairs)
                }
                "MEMORY" => match read_memory(reader, count, max_terms)? {
                    Some(memory) => LoadedValue::Memory(Box::new(memory)),
                    // A truncated record leaves the reader mid-stream,
                    // so nothing after it can be trusted either.
                    None => break,
                },
                _ => continue,
            };
            entries.push(LoadedEntry {
                key,
                value,
                expire_at,
            });
        }
        Ok(entries)
    }

    /// The original text format: whitespace-delimited, one record per
    /// line, with an optional `EXPIREAT key seconds` line after a key.
    fn load_v1<R: BufRead>(&self, reader: &mut DumpReader<R>) -> io::Result<Vec<LoadedEntry>> {
        let mut entries: Vec<LoadedEntry> = Vec::new();
        while let Some(line) = reader.line()? {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.splitn(3, ' ');
            let (Some(kind), Some(key)) = (parts.next(), parts.next()) else {
                continue;
            };
            let tail = parts.next().unwrap_or("");
            let key = key.as_bytes().to_vec();

            if kind == "EXPIREAT" {
                // Applies to the key named on this line, which the
                // record just above introduced.
                if let (Some(entry), Ok(seconds)) = (
                    entries.iter_mut().rev().find(|e| e.key == key),
                    tail.trim().parse::<i64>(),
                ) {
                    entry.expire_at = seconds.saturating_mul(1000);
                }
                continue;
            }

            let count: usize = tail.trim().parse().unwrap_or(0);
            let mut next_line =
                || -> io::Result<Bytes> { Ok(reader.line()?.unwrap_or_default().into_bytes()) };
            let value = match kind {
                "STRING" => LoadedValue::Str(tail.as_bytes().to_vec()),
                "LIST" | "SET" => {
                    let mut items = Vec::with_capacity(count);
                    for _ in 0..count {
                        items.push(next_line()?);
                    }
                    if kind == "LIST" {
                        LoadedValue::List(items)
                    } else {
                        LoadedValue::Set(items)
                    }
                }
                "HASH" => {
                    let mut pairs = Vec::with_capacity(count);
                    for _ in 0..count {
                        let line = reader.line()?.unwrap_or_default();
                        match line.split_once(' ') {
                            Some((field, value)) => {
                                pairs.push((field.as_bytes().to_vec(), value.as_bytes().to_vec()))
                            }
                            None => pairs.push((line.into_bytes(), Vec::new())),
                        }
                    }
                    LoadedValue::Hash(pairs)
                }
                "ZSET" => {
                    let mut pairs = Vec::with_capacity(count);
                    for _ in 0..count {
                        let line = reader.line()?.unwrap_or_default();
                        if let Some((member, score)) = line.rsplit_once(' ') {
                            if let Some(score) = parse_f64(score.as_bytes()) {
                                pairs.push((member.as_bytes().to_vec(), score));
                            }
                        }
                    }
                    LoadedValue::Zset(pairs)
                }
                _ => continue,
            };
            entries.push(LoadedEntry {
                key,
                value,
                expire_at: -1,
            });
        }
        Ok(entries)
    }

    /// Writes the whole keyspace to `path`, atomically, by writing a
    /// temp file and renaming it over the target. `path` becomes this
    /// instance's path from now on, which is how `CONFIG SET
    /// dbfilename` redirects the next save. Returns whether the write
    /// succeeded.
    pub fn save_to(&mut self, path: &str, store: &mut Store) -> bool {
        self.path = PathBuf::from(path);
        match self.save_inner(store) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("persist_save: {}", e);
                false
            }
        }
    }

    fn save_inner(&mut self, store: &mut Store) -> io::Result<()> {
        let mut tmp_path = self.path.clone().into_os_string();
        tmp_path.push(".tmp");
        let tmp_path = PathBuf::from(tmp_path);

        let mut f = File::create(&tmp_path)?;
        writeln!(f, "{}", DUMP_MAGIC)?;

        for (key, kind) in store.foreach_entry() {
            // An absolute deadline, so downtime is accounted for.
            let ttl_ms = store.pttl_ms(&key);
            let expire_at = if ttl_ms < 0 {
                -1
            } else {
                unix_millis_now() + ttl_ms
            };

            match kind {
                StoreType::String => {
                    let value = store.get_string(&key).unwrap_or_default();
                    writeln!(f, "STRING {}", expire_at)?;
                    write_blob(&mut f, &key)?;
                    write_blob(&mut f, &value)?;
                }
                StoreType::List => {
                    if let Some(list) = store.read_list(&key) {
                        let items: Vec<Bytes> = list.iter().cloned().collect();
                        writeln!(f, "LIST {} {}", expire_at, items.len())?;
                        write_blob(&mut f, &key)?;
                        for item in items {
                            write_blob(&mut f, &item)?;
                        }
                    }
                }
                StoreType::Hash => {
                    if let Some(hash) = store.read_hash(&key) {
                        let pairs: Vec<(Bytes, Bytes)> =
                            hash.iter().map(|(f, v)| (f.clone(), v.clone())).collect();
                        writeln!(f, "HASH {} {}", expire_at, pairs.len())?;
                        write_blob(&mut f, &key)?;
                        for (field, value) in pairs {
                            write_blob(&mut f, &field)?;
                            write_blob(&mut f, &value)?;
                        }
                    }
                }
                StoreType::Set => {
                    if let Some(set) = store.read_set(&key) {
                        let members: Vec<Bytes> = set.iter().cloned().collect();
                        writeln!(f, "SET {} {}", expire_at, members.len())?;
                        write_blob(&mut f, &key)?;
                        for member in members {
                            write_blob(&mut f, &member)?;
                        }
                    }
                }
                StoreType::Memory => {
                    if let Some(memory) = store.get_existing_memory(&key) {
                        let records = memory.records_for_dump();
                        writeln!(f, "MEMORY {} {}", expire_at, records.len())?;
                        write_blob(&mut f, &key)?;
                        let config = memory.config();
                        writeln!(
                            f,
                            "{} {} {} {} {} {} {} {} {}",
                            config.mode.name(),
                            config.dim,
                            config.metric.name(),
                            config.weights.keyword,
                            config.weights.vector,
                            config.weights.recency,
                            config.weights.importance,
                            config.half_life.as_secs(),
                            memory.next_id(),
                        )?;
                        for (record, vector) in records {
                            write_blob(&mut f, &record.id)?;
                            write_blob(&mut f, &record.text)?;
                            writeln!(
                                f,
                                "{} {} {} {} {} {}",
                                unix_millis(record.created_at),
                                unix_millis(record.updated_at),
                                record.importance,
                                record.expire_at.map_or(-1, unix_millis),
                                record.meta().len(),
                                vector.map_or(0, |v| v.len()),
                            )?;
                            for (field, value) in record.meta() {
                                write_blob(&mut f, field)?;
                                write_blob(&mut f, value)?;
                            }
                            if let Some(values) = vector {
                                write_blob(&mut f, &encode_le_f32(values))?;
                            }
                        }
                    }
                }
                StoreType::Zset => {
                    if let Some(zset) = store.read_zset(&key) {
                        let pairs: Vec<(Bytes, f64)> =
                            zset.iter().map(|(m, s)| (m.to_vec(), s)).collect();
                        writeln!(f, "ZSET {} {}", expire_at, pairs.len())?;
                        write_blob(&mut f, &key)?;
                        for (member, score) in pairs {
                            write_blob(&mut f, &member)?;
                            f.write_all(&format_f64(score))?;
                            f.write_all(b"\n")?;
                        }
                    }
                }
            }
        }

        f.sync_all()?;
        drop(f);
        fs::rename(&tmp_path, &self.path)?;
        store.reset_dirty();
        Ok(())
    }

    /// Whether `interval` has elapsed since the last check *and* the
    /// store has unsaved changes. Advances the check clock as a side
    /// effect, so the caller gets one `true` per interval rather than
    /// one per tick.
    pub fn autosave_due(&mut self, interval: Duration, store: &Store) -> bool {
        let now = Instant::now();
        match self.last_check {
            None => {
                // Start the clock rather than firing immediately: at
                // startup nothing has changed yet anyway.
                self.last_check = Some(now);
                return false;
            }
            Some(last) if now.duration_since(last) < interval => return false,
            Some(_) => self.last_check = Some(now),
        }
        store.dirty_count() > 0
    }
}

/// Reads one `MEMORY` record's configuration line and its records.
/// `None` means the dump ran out mid-record.
fn read_memory<R: BufRead>(
    reader: &mut DumpReader<R>,
    count: usize,
    max_terms: usize,
) -> io::Result<Option<Memory>> {
    let Some(header) = reader.line()? else {
        return Ok(None);
    };
    let fields: Vec<&str> = header.split_whitespace().collect();
    if fields.len() < 9 {
        return Ok(None);
    }
    let (Some(mode), Some(metric)) = (
        Mode::parse(fields[0].as_bytes()),
        Metric::parse(fields[2].as_bytes()),
    ) else {
        return Ok(None);
    };
    let number = |at: usize| fields[at].parse::<f32>().unwrap_or_default();
    let mut config = MemoryConfig::new(mode, fields[1].parse().unwrap_or(0), metric);
    let weights = Weights {
        keyword: number(3),
        vector: number(4),
        recency: number(5),
        importance: number(6),
    };
    if weights.is_valid() {
        config.weights = weights;
    }
    if let Ok(seconds) = fields[7].parse::<u64>() {
        config.half_life = Duration::from_secs(seconds);
    }

    let mut memory = Memory::new(config);
    memory.set_next_id(fields[8].parse().unwrap_or(1));

    for _ in 0..count {
        let (Some(id), Some(text), Some(line)) = (reader.blob()?, reader.blob()?, reader.line()?)
        else {
            return Ok(None);
        };
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 6 {
            return Ok(None);
        }
        let millis = |at: usize| parts[at].parse::<i64>().unwrap_or(-1);
        let mut record = MemoryRecord::new(id, text, from_unix_millis(millis(0)));
        record.updated_at = from_unix_millis(millis(1));
        // Clamped on the way in: the command layer bounds this, but a
        // dump is a file on disk and may have been edited by hand.
        record.importance = parts[2].parse().unwrap_or(0.5f32).clamp(0.0, 1.0);
        record.expire_at = match millis(3) {
            at if at >= 0 => Some(from_unix_millis(at)),
            _ => None,
        };
        let meta_count: usize = parts[4].parse().unwrap_or(0);
        let vector_len: usize = parts[5].parse().unwrap_or(0);

        for _ in 0..meta_count {
            let (Some(field), Some(value)) = (reader.blob()?, reader.blob()?) else {
                return Ok(None);
            };
            record.set_meta(field, value);
        }
        let vector = if vector_len > 0 {
            let Some(blob) = reader.blob()? else {
                return Ok(None);
            };
            parse_le_f32(&blob).ok()
        } else {
            None
        };
        memory.load_record(record, vector, max_terms);
    }
    Ok(Some(memory))
}

enum LoadedValue {
    Str(Bytes),
    List(Vec<Bytes>),
    Hash(Vec<(Bytes, Bytes)>),
    Set(Vec<Bytes>),
    Zset(Vec<(Bytes, f64)>),
    Memory(Box<Memory>),
}

struct LoadedEntry {
    key: Bytes,
    value: LoadedValue,
    /// Unix milliseconds, or -1 for no expiry.
    expire_at: i64,
}

impl LoadedEntry {
    fn install(self, store: &mut Store) {
        match self.value {
            LoadedValue::Str(v) => store.set_string(&self.key, &v),
            LoadedValue::List(items) => {
                if let Some(list) = store.get_or_create_list(&self.key) {
                    list.extend(items);
                }
            }
            LoadedValue::Set(members) => {
                if let Some(set) = store.get_or_create_set(&self.key) {
                    set.extend(members);
                }
            }
            LoadedValue::Hash(pairs) => {
                if let Some(hash) = store.get_or_create_hash(&self.key) {
                    hash.extend(pairs);
                }
            }
            LoadedValue::Zset(pairs) => {
                if let Some(zset) = store.get_or_create_zset(&self.key) {
                    for (member, score) in pairs {
                        zset.add(&member, score);
                    }
                }
            }
            LoadedValue::Memory(memory) => {
                store.create_memory(&self.key, *memory);
            }
        }
        if self.expire_at >= 0 {
            let deadline = UNIX_EPOCH + Duration::from_millis(self.expire_at as u64);
            store.set_expire_at(&self.key, Some(deadline));
        }
        // A key whose collection came back empty shouldn't exist.
        store.delete_if_empty(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tokenizer cap a reload rebuilds a memory index under. Any
    /// value works here; the tests index a handful of words.
    const TEST_MAX_TERMS: usize = 1024;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "klyro_persist_test_{}_{}",
            std::process::id(),
            name
        ))
    }

    fn cleanup(path: &PathBuf) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(format!("{}.tmp", path.display()));
    }

    #[test]
    fn round_trips_all_types_and_ttl() {
        let path = temp_path("roundtrip");
        cleanup(&path);

        let mut store = Store::new();
        store.set_string(b"greeting", b"hello persistence");
        store
            .get_or_create_list(b"mylist")
            .unwrap()
            .push_back(b"a".to_vec());
        store
            .get_or_create_hash(b"user")
            .unwrap()
            .insert(b"name".to_vec(), b"Alice".to_vec());
        store
            .get_or_create_set(b"tags")
            .unwrap()
            .insert(b"fast".to_vec());
        store
            .get_or_create_zset(b"board")
            .unwrap()
            .add(b"alice", 100.0);
        store.set_expire_at(
            b"greeting",
            Some(SystemTime::now() + Duration::from_secs(300)),
        );

        let mut persist = Persist::new(path.to_str().unwrap());
        assert!(persist.save_to(path.to_str().unwrap(), &mut store));

        let mut reloaded = Store::new();
        Persist::new(path.to_str().unwrap()).load(&mut reloaded, TEST_MAX_TERMS);

        assert_eq!(
            reloaded.get_string(b"greeting"),
            Some(b"hello persistence".to_vec())
        );
        assert!(reloaded.ttl(b"greeting") > 290);
        assert_eq!(
            reloaded
                .read_list(b"mylist")
                .unwrap()
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![b"a".to_vec()]
        );
        assert_eq!(
            reloaded
                .read_hash(b"user")
                .unwrap()
                .get(b"name".as_slice())
                .unwrap(),
            b"Alice"
        );
        assert!(reloaded
            .read_set(b"tags")
            .unwrap()
            .contains(b"fast".as_slice()));
        assert_eq!(
            reloaded.read_zset(b"board").unwrap().score(b"alice"),
            Some(100.0)
        );
        cleanup(&path);
    }

    #[test]
    fn round_trips_values_holding_newlines_and_nul_bytes() {
        let path = temp_path("binary");
        cleanup(&path);

        let key = b"awkward\r\nkey".to_vec();
        let value = b"line one\nline two\0with a nul".to_vec();
        let mut store = Store::new();
        store.set_string(&key, &value);
        store
            .get_or_create_list(b"l")
            .unwrap()
            .push_back(b"a\nb".to_vec());

        let mut persist = Persist::new(path.to_str().unwrap());
        assert!(persist.save_to(path.to_str().unwrap(), &mut store));

        let mut reloaded = Store::new();
        Persist::new(path.to_str().unwrap()).load(&mut reloaded, TEST_MAX_TERMS);
        assert_eq!(reloaded.get_string(&key), Some(value));
        assert_eq!(reloaded.read_list(b"l").unwrap().front().unwrap(), b"a\nb");
        cleanup(&path);
    }

    #[test]
    fn reads_a_version_1_dump() {
        let path = temp_path("legacy");
        cleanup(&path);
        fs::write(
            &path,
            "KLYRO-DUMP 1\n\
             STRING greeting hello there\n\
             LIST mylist 2\n\
             a\n\
             b\n\
             HASH user 1\n\
             name Alice\n\
             SET tags 1\n\
             fast\n\
             ZSET board 1\n\
             alice 100\n",
        )
        .unwrap();

        let mut store = Store::new();
        Persist::new(path.to_str().unwrap()).load(&mut store, TEST_MAX_TERMS);

        assert_eq!(store.get_string(b"greeting"), Some(b"hello there".to_vec()));
        assert_eq!(store.read_list(b"mylist").unwrap().len(), 2);
        assert_eq!(
            store
                .read_hash(b"user")
                .unwrap()
                .get(b"name".as_slice())
                .unwrap(),
            b"Alice"
        );
        assert!(store
            .read_set(b"tags")
            .unwrap()
            .contains(b"fast".as_slice()));
        assert_eq!(
            store.read_zset(b"board").unwrap().score(b"alice"),
            Some(100.0)
        );
        cleanup(&path);
    }

    #[test]
    fn a_version_1_expireat_line_still_applies() {
        let path = temp_path("legacy_ttl");
        cleanup(&path);
        let future = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 500;
        fs::write(
            &path,
            format!("KLYRO-DUMP 1\nSTRING k v\nEXPIREAT k {}\n", future),
        )
        .unwrap();

        let mut store = Store::new();
        Persist::new(path.to_str().unwrap()).load(&mut store, TEST_MAX_TERMS);
        let ttl = store.ttl(b"k");
        assert!((400..=500).contains(&ttl), "got {ttl}");
        cleanup(&path);
    }

    #[test]
    fn a_file_that_is_not_a_dump_is_ignored() {
        let path = temp_path("garbage");
        cleanup(&path);
        fs::write(&path, "this is not a dump\n").unwrap();

        let mut store = Store::new();
        Persist::new(path.to_str().unwrap()).load(&mut store, TEST_MAX_TERMS);
        assert_eq!(store.size(), 0);
        cleanup(&path);
    }
}
