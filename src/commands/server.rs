//! Connection and server-lifecycle commands, plus the two
//! introspection commands: INFO and CONFIG.

use std::fmt::Write as _;
use std::time::UNIX_EPOCH;

use super::{exact_args, min_args, Checked, Response};
use crate::app::App;
use crate::config::{SetError, PARAMETERS};
use crate::resp::{Protocol, Reply};
use crate::util::bytes::{eq_ignore_case, to_display, to_upper, Bytes};
use crate::util::glob::glob_match;
use crate::util::memory::{human_bytes, peak_bytes, used_bytes};

/// INFO's sections, in the order a bare INFO prints them.
const SECTIONS: &[&str] = &[
    "server",
    "clients",
    "memory",
    "persistence",
    "stats",
    "keyspace",
    "memorydb",
];

pub fn dispatch(app: &mut App, name: &str, argv: &[Bytes]) -> Response {
    let close = matches!(name, "QUIT" | "SHUTDOWN");
    let reply = match handle(app, name, argv) {
        Ok(reply) | Err(reply) => reply,
    };
    // A successful HELLO is the one command that changes how later
    // replies are encoded.
    let protocol = match (name, &reply) {
        ("HELLO", Reply::Map(_)) => Some(negotiated_protocol(argv)),
        _ => None,
    };
    Response {
        reply,
        close,
        protocol,
    }
}

fn negotiated_protocol(argv: &[Bytes]) -> Protocol {
    argv.get(1)
        .and_then(|v| crate::util::bytes::parse_i64(v))
        .and_then(Protocol::from_version)
        .unwrap_or(Protocol::Resp2)
}

fn handle(app: &mut App, name: &str, argv: &[Bytes]) -> Checked<Reply> {
    match name {
        "PING" => match argv.len() {
            1 => Ok(Reply::Simple("PONG")),
            2 => Ok(Reply::bulk(argv[1].clone())),
            _ => Err(Reply::wrong_arity(name)),
        },

        "ECHO" => {
            exact_args(argv, name, 1)?;
            Ok(Reply::bulk(argv[1].clone()))
        }

        "HELLO" => hello(argv),

        // Clients probe COMMAND on connect to learn the command table.
        // An empty array means "no introspection available", which they
        // accept rather than treating as an error.
        "COMMAND" => Ok(Reply::Array(Vec::new())),

        "INFO" => info(app, argv),

        "CONFIG" => config(app, argv),

        "SAVE" => {
            if app.save() {
                Ok(Reply::ok())
            } else {
                Ok(Reply::error("ERR the save failed; see the server log"))
            }
        }

        "QUIT" => Ok(Reply::ok()),

        "SHUTDOWN" => {
            app.running = false;
            Ok(Reply::ok())
        }

        _ => Ok(Reply::error("ERR unknown command")),
    }
}

/// `HELLO [protover]` - the handshake modern clients open with.
///
/// Klyro speaks RESP2 only, so a request for RESP3 is refused with
/// NOPROTO, which is the reply clients are built to downgrade on.
fn hello(argv: &[Bytes]) -> Checked<Reply> {
    let protocol = match argv.get(1) {
        None => Protocol::Resp2,
        Some(version) => {
            match crate::util::bytes::parse_i64(version).and_then(Protocol::from_version) {
                Some(p) => p,
                None => return Ok(Reply::error("NOPROTO unsupported protocol version")),
            }
        }
    };
    Ok(Reply::Map(vec![
        (Reply::bulk("server"), Reply::bulk("klyro")),
        (Reply::bulk("version"), Reply::bulk(crate::KLYRO_VERSION)),
        (Reply::bulk("proto"), Reply::Integer(protocol.version())),
        (Reply::bulk("id"), Reply::Integer(0)),
        (Reply::bulk("mode"), Reply::bulk("standalone")),
        (Reply::bulk("role"), Reply::bulk("master")),
        (Reply::bulk("modules"), Reply::Array(Vec::new())),
    ]))
}

fn unix_seconds(time: std::time::SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `INFO [section]` - one bulk string of `# Section` headers over
/// `key:value` lines, which is exactly the shape Redis returns and what
/// client libraries parse into a dictionary.
fn info(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    let requested = match argv.len() {
        1 => "all".to_string(),
        2 => to_display(&argv[1]).to_ascii_lowercase(),
        _ => return Err(Reply::wrong_arity("INFO")),
    };
    let wanted: Vec<&str> = match requested.as_str() {
        "all" | "default" | "everything" => SECTIONS.to_vec(),
        name => match SECTIONS.iter().find(|s| **s == name) {
            Some(section) => vec![*section],
            // Redis answers an unknown section with an empty string
            // rather than an error.
            None => Vec::new(),
        },
    };

    let mut out = String::new();
    for section in wanted {
        if !out.is_empty() {
            out.push_str("\r\n");
        }
        append_section(app, section, &mut out);
    }
    Ok(Reply::bulk(out))
}

fn append_section(app: &mut App, section: &str, out: &mut String) {
    macro_rules! line {
        ($key:expr, $value:expr $(,)?) => {
            // Writing to a String is infallible.
            let _ = writeln!(out, "{}:{}\r", $key, $value);
        };
    }
    let _ = writeln!(out, "# {}\r", title_case(section));

    match section {
        "server" => {
            line!("klyro_version", crate::KLYRO_VERSION);
            // Client libraries gate features on redis_version. Klyro
            // implements the 7.x command shapes it supports, so it
            // reports that rather than leaving clients guessing.
            line!("redis_version", crate::REDIS_COMPAT_VERSION);
            line!("process_id", std::process::id());
            line!("tcp_bind", app.config.bind);
            line!("tcp_port", app.config.port);
            let uptime = app.stats.uptime().as_secs();
            line!("uptime_in_seconds", uptime);
            line!("uptime_in_days", uptime / 86_400);
        }

        "clients" => {
            line!("connected_clients", app.stats.connected_clients);
            line!("maxclients", app.config.maxclients);
            line!("rejected_connections", app.stats.rejected_connections);
            line!("watched_keys", app.store.watched_count());
        }

        "memory" => {
            // Real numbers, from the counting global allocator - this
            // is the whole process, not just the keyspace.
            let (used, peak) = (used_bytes(), peak_bytes());
            line!("used_memory", used);
            line!("used_memory_human", human_bytes(used));
            line!("used_memory_peak", peak);
            line!("used_memory_peak_human", human_bytes(peak));
        }

        "persistence" => {
            line!("dbfilename", app.config.dbfilename);
            line!("changes_since_last_save", app.store.dirty_count());
            line!(
                "last_save_time",
                app.stats.last_save_at.map_or(0, unix_seconds)
            );
            line!(
                "last_save_status",
                if app.stats.last_save_ok { "ok" } else { "err" }
            );
            line!("total_saves", app.stats.save_count);
            line!("save_interval_seconds", app.config.save_interval.as_secs());
        }

        "stats" => {
            line!("total_connections_received", app.stats.total_connections);
            line!("total_commands_processed", app.stats.total_commands);
            line!("keyspace_hits", app.stats.keyspace_hits);
            line!("keyspace_misses", app.stats.keyspace_misses);
            line!("expired_keys", app.store.expired_count());
        }

        // Named apart from "memory", which reports the allocator's view
        // of the whole process rather than anything about memory
        // indexes.
        "memorydb" => {
            let keys = app.store.memory_keys();
            let (mut records, mut vectors, mut terms, mut bytes) = (0, 0, 0, 0);
            for key in &keys {
                if let Some(memory) = app.store.get_existing_memory(key) {
                    records += memory.len();
                    vectors += memory.vector_count();
                    terms += memory.term_count();
                    bytes += memory.heap_bytes();
                }
            }
            line!("memory_indexes", keys.len());
            line!("memory_records", records);
            line!("memory_vectors", vectors);
            line!("memory_terms", terms);
            line!("memory_index_bytes", bytes);
            line!("memory_records_expired", app.stats.memory_records_expired);
        }

        "keyspace" => {
            // One O(n) walk of the keyspace, so this is the one INFO
            // section whose cost grows with the dataset.
            let _ = writeln!(
                out,
                "db0:keys={},expires={}\r",
                app.store.size(),
                app.store.volatile_size()
            );
            for (kind, count) in app.store.type_breakdown() {
                line!(kind.name(), count);
            }
        }

        _ => {}
    }
}

fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn config(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "CONFIG", 1)?;
    match to_upper(&argv[1]).as_str() {
        "GET" => {
            min_args(argv, "CONFIG|GET", 2)?;
            // A flat name, value, name, value array, which is what
            // clients unpack into a map.
            let mut pairs = Vec::new();
            for name in PARAMETERS {
                if argv[2..]
                    .iter()
                    .any(|pattern| glob_match(pattern, name.as_bytes()))
                {
                    if let Some(value) = app.config.get(name) {
                        pairs.push((Reply::bulk(*name), Reply::bulk(value)));
                    }
                }
            }
            Ok(Reply::Map(pairs))
        }

        "SET" => {
            exact_args(argv, "CONFIG|SET", 3)?;
            let name = to_display(&argv[2]).to_ascii_lowercase();
            let value = to_display(&argv[3]);
            match app.config.set(&name, &value, false) {
                Ok(()) => Ok(Reply::ok()),
                Err(SetError::Unknown) => Ok(Reply::error(format!(
                    "ERR Unknown option or number of arguments for CONFIG SET - '{}'",
                    name
                ))),
                Err(e) => Ok(Reply::error(format!(
                    "ERR CONFIG SET failed - {} '{}'",
                    e, name
                ))),
            }
        }

        "RESETSTAT" => {
            app.stats.reset();
            Ok(Reply::ok())
        }

        other => Ok(Reply::error(format!(
            "ERR Unknown CONFIG subcommand: {}",
            other
        ))),
    }
}

/// Only used to keep the option-name comparison honest for callers that
/// pass raw bytes.
#[allow(dead_code)]
fn is_option(arg: &[u8], word: &str) -> bool {
    eq_ignore_case(arg, word)
}
