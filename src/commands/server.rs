//! Connection and server-lifecycle commands, plus the two
//! introspection commands: INFO and CONFIG.

use std::time::UNIX_EPOCH;

use super::{reply_list, usage};
use crate::app::App;
use crate::config::{SetError, PARAMETERS};
use crate::server::Conn;
use crate::util::glob::glob_match;
use crate::util::memory::{human_bytes, peak_bytes, used_bytes};
use crate::util::strutil::{next_token, trim};

/// INFO's sections, in the order a bare INFO prints them.
const SECTIONS: &[&str] = &[
    "server",
    "clients",
    "memory",
    "persistence",
    "stats",
    "keyspace",
];

pub fn dispatch(app: &mut App, conn: &mut Conn, cmd: &str, rest: &str) {
    match cmd {
        "PING" => conn.reply("PONG\r\n"),

        "ECHO" => {
            let message = trim(rest);
            if message.is_empty() {
                return usage(conn, "ECHO message");
            }
            conn.reply(&format!("VALUE {}\r\n", message));
        }

        "INFO" => info(app, conn, rest),

        "CONFIG" => config(app, conn, rest),

        "SAVE" => {
            let ok = app.save();
            conn.reply(if ok {
                "OK\r\n"
            } else {
                "ERR the save failed; see the server log\r\n"
            });
        }

        "QUIT" => {
            conn.reply("BYE\r\n");
            conn.request_close();
        }

        "SHUTDOWN" => {
            conn.reply("SHUTTING_DOWN\r\n");
            app.running = false;
        }

        _ => conn.reply("ERR unknown command\r\n"),
    }
}

fn unix_seconds(time: std::time::SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `INFO [section]` - `server`, `clients`, `memory`, `persistence`,
/// `stats`, `keyspace`, or `all`/`default`/nothing for every one.
fn info(app: &mut App, conn: &mut Conn, rest: &str) {
    let requested = trim(rest).to_ascii_lowercase();
    let wanted: Vec<&str> = match requested.as_str() {
        "" | "all" | "default" => SECTIONS.to_vec(),
        name if SECTIONS.contains(&name) => {
            vec![SECTIONS.iter().find(|s| **s == name).expect("just matched")]
        }
        _ => {
            return usage(
                conn,
                "INFO [server|clients|memory|persistence|stats|keyspace|all]",
            )
        }
    };

    let mut lines: Vec<String> = Vec::new();
    for (i, section) in wanted.iter().enumerate() {
        if i > 0 {
            lines.push(String::new()); // blank line between sections
        }
        append_section(app, section, &mut lines);
    }
    reply_list(conn, lines);
}

fn append_section(app: &mut App, section: &str, lines: &mut Vec<String>) {
    // A section is a bare `# Name` header followed by `key:value`
    // lines, the shape Redis's INFO uses.
    macro_rules! add {
        ($key:expr, $value:expr $(,)?) => {
            lines.push(format!("{}:{}", $key, $value))
        };
    }

    match section {
        "server" => {
            lines.push("# Server".to_string());
            add!("klyro_version", crate::KLYRO_VERSION.to_string());
            add!("process_id", std::process::id().to_string());
            add!("tcp_bind", app.config.bind.clone());
            add!("tcp_port", app.config.port.to_string());
            let uptime = app.stats.uptime().as_secs();
            add!("uptime_in_seconds", uptime.to_string());
            add!("uptime_in_days", (uptime / 86_400).to_string());
        }

        "clients" => {
            lines.push("# Clients".to_string());
            add!("connected_clients", app.stats.connected_clients.to_string());
            add!("maxclients", app.config.maxclients.to_string());
            add!(
                "rejected_connections",
                app.stats.rejected_connections.to_string(),
            );
        }

        "memory" => {
            // Real numbers, from the counting global allocator - this
            // is the whole process, not just the keyspace.
            let used = used_bytes();
            let peak = peak_bytes();
            lines.push("# Memory".to_string());
            add!("used_memory", used.to_string());
            add!("used_memory_human", human_bytes(used));
            add!("used_memory_peak", peak.to_string());
            add!("used_memory_peak_human", human_bytes(peak));
        }

        "persistence" => {
            lines.push("# Persistence".to_string());
            add!("dbfilename", app.config.dbfilename.clone());
            add!(
                "changes_since_last_save",
                app.store.dirty_count().to_string(),
            );
            add!(
                "last_save_time",
                app.stats.last_save_at.map_or(0, unix_seconds).to_string(),
            );
            add!(
                "last_save_status",
                if app.stats.last_save_ok { "ok" } else { "err" }.to_string(),
            );
            add!("total_saves", app.stats.save_count.to_string());
            add!(
                "save_interval_seconds",
                app.config.save_interval.as_secs().to_string(),
            );
        }

        "stats" => {
            lines.push("# Stats".to_string());
            add!(
                "total_connections_received",
                app.stats.total_connections.to_string(),
            );
            add!(
                "total_commands_processed",
                app.stats.total_commands.to_string(),
            );
            add!("keyspace_hits", app.stats.keyspace_hits.to_string());
            add!("keyspace_misses", app.stats.keyspace_misses.to_string());
            add!("expired_keys", app.store.expired_count().to_string());
        }

        "keyspace" => {
            // One O(n) walk of the keyspace, so this is the one INFO
            // section whose cost grows with the dataset.
            lines.push("# Keyspace".to_string());
            let keys = app.store.size();
            let expires = app.store.volatile_size();
            lines.push(format!("db0:keys={},expires={}", keys, expires));
            for (kind, count) in app.store.type_breakdown() {
                lines.push(format!("{}:{}", kind.name().to_ascii_lowercase(), count));
            }
        }

        _ => {}
    }
}

const CONFIG_USAGE: &str = "CONFIG GET pattern | CONFIG SET parameter value | CONFIG RESETSTAT";

fn config(app: &mut App, conn: &mut Conn, rest: &str) {
    let mut rest = rest;
    let Some(subcommand) = next_token(&mut rest) else {
        return usage(conn, CONFIG_USAGE);
    };

    match subcommand.to_ascii_uppercase().as_str() {
        "GET" => {
            let pattern = trim(rest);
            if pattern.is_empty() {
                return usage(conn, CONFIG_USAGE);
            }
            let matches: Vec<String> = PARAMETERS
                .iter()
                .filter(|name| glob_match(pattern, name))
                .filter_map(|name| {
                    app.config
                        .get(name)
                        .map(|value| format!("{} {}", name, value))
                })
                .collect();
            reply_list(conn, matches);
        }

        "SET" => {
            let name = next_token(&mut rest);
            let value = trim(rest);
            let Some(name) = name.filter(|_| !value.is_empty()) else {
                return usage(conn, CONFIG_USAGE);
            };
            let name = name.to_ascii_lowercase();
            match app.config.set(&name, value, false) {
                Ok(()) => {
                    // dbfilename only takes effect on the next save,
                    // which App::save already reads from the config.
                    conn.reply("OK\r\n")
                }
                Err(SetError::Unknown) => {
                    conn.reply(&format!("ERR unknown parameter `{}`\r\n", name))
                }
                Err(e) => conn.reply(&format!("ERR {} `{}`\r\n", e, name)),
            }
        }

        "RESETSTAT" => {
            app.stats.reset();
            conn.reply("OK\r\n");
        }

        _ => usage(conn, CONFIG_USAGE),
    }
}
