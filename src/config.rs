//! Runtime configuration: defaults, an optional config file, and the
//! get/set surface behind the CONFIG command.
//!
//! Every tunable that used to be a `const` somewhere in the source
//! lives here instead, so `CONFIG SET` can move it without a restart.
//! The two that can't move - the address and port the listener is
//! already bound to - are marked immutable and rejected at runtime.

use std::fmt;
use std::fs;
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 7171;
pub const DEFAULT_DUMP_PATH: &str = "klyro.dump";

/// Why a `CONFIG SET` was refused.
#[derive(Debug, PartialEq)]
pub enum SetError {
    Unknown,
    Immutable,
    BadValue,
}

impl fmt::Display for SetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetError::Unknown => write!(f, "unknown parameter"),
            SetError::Immutable => write!(f, "parameter cannot be changed at runtime"),
            SetError::BadValue => write!(f, "invalid value for parameter"),
        }
    }
}

pub struct Config {
    /// Listener address. Fixed once the socket is bound.
    pub bind: String,
    /// Listener port. Fixed once the socket is bound.
    pub port: u16,
    /// Where the keyspace snapshot is written. Changing it at runtime
    /// redirects the *next* save; the current file is left alone.
    pub dbfilename: String,
    /// How often the autosave check runs.
    pub save_interval: Duration,
    /// How often the active expired-key sweep runs.
    pub sweep_interval: Duration,
    /// Connection ceiling. Past it, new connections are told so and
    /// closed rather than being silently dropped.
    pub maxclients: usize,
    /// Ceiling on a stored string, enforced by APPEND and SETRANGE.
    pub max_string_bytes: usize,
    /// The COUNT a SCAN uses when the caller doesn't give one.
    pub scan_default_count: usize,
    /// Ceiling on score/member pairs in a single ZADD.
    pub zadd_max_pairs: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind: "0.0.0.0".to_string(),
            port: DEFAULT_PORT,
            dbfilename: DEFAULT_DUMP_PATH.to_string(),
            save_interval: Duration::from_secs(60),
            sweep_interval: Duration::from_secs(1),
            maxclients: 10_000,
            max_string_bytes: 64 * 1024,
            scan_default_count: 10,
            zadd_max_pairs: 128,
        }
    }
}

/// Every parameter name, in the order CONFIG GET reports them.
pub const PARAMETERS: &[&str] = &[
    "bind",
    "port",
    "dbfilename",
    "save-interval",
    "sweep-interval",
    "maxclients",
    "max-string-bytes",
    "scan-default-count",
    "zadd-max-pairs",
];

/// Parses a positive integer, rejecting zero - every numeric parameter
/// here is a size or an interval, and none of them is meaningful at 0.
fn positive(value: &str) -> Result<u64, SetError> {
    match value.parse::<u64>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(SetError::BadValue),
    }
}

impl Config {
    /// The current value of `name`, formatted the way CONFIG GET
    /// reports it. `None` for an unknown parameter.
    pub fn get(&self, name: &str) -> Option<String> {
        Some(match name {
            "bind" => self.bind.clone(),
            "port" => self.port.to_string(),
            "dbfilename" => self.dbfilename.clone(),
            "save-interval" => self.save_interval.as_secs().to_string(),
            "sweep-interval" => self.sweep_interval.as_millis().to_string(),
            "maxclients" => self.maxclients.to_string(),
            "max-string-bytes" => self.max_string_bytes.to_string(),
            "scan-default-count" => self.scan_default_count.to_string(),
            "zadd-max-pairs" => self.zadd_max_pairs.to_string(),
            _ => return None,
        })
    }

    /// Applies `value` to `name`. Used by both CONFIG SET and the
    /// config-file loader, so a file can't set anything CONFIG SET
    /// can't - except `bind`/`port`, which `allow_startup_only` opens
    /// up for the loader.
    pub fn set(
        &mut self,
        name: &str,
        value: &str,
        allow_startup_only: bool,
    ) -> Result<(), SetError> {
        match name {
            "bind" | "port" if !allow_startup_only => Err(SetError::Immutable),
            "bind" => {
                if value.is_empty() {
                    return Err(SetError::BadValue);
                }
                self.bind = value.to_string();
                Ok(())
            }
            "port" => {
                self.port = value.parse::<u16>().map_err(|_| SetError::BadValue)?;
                if self.port == 0 {
                    return Err(SetError::BadValue);
                }
                Ok(())
            }
            "dbfilename" => {
                if value.is_empty() {
                    return Err(SetError::BadValue);
                }
                self.dbfilename = value.to_string();
                Ok(())
            }
            "save-interval" => {
                self.save_interval = Duration::from_secs(positive(value)?);
                Ok(())
            }
            "sweep-interval" => {
                self.sweep_interval = Duration::from_millis(positive(value)?);
                Ok(())
            }
            "maxclients" => {
                self.maxclients = positive(value)? as usize;
                Ok(())
            }
            "max-string-bytes" => {
                self.max_string_bytes = positive(value)? as usize;
                Ok(())
            }
            "scan-default-count" => {
                self.scan_default_count = positive(value)? as usize;
                Ok(())
            }
            "zadd-max-pairs" => {
                self.zadd_max_pairs = positive(value)? as usize;
                Ok(())
            }
            _ => Err(SetError::Unknown),
        }
    }

    /// Reads a config file: one `name value` pair per line, `#`
    /// comments and blank lines ignored. Returns every problem found
    /// rather than stopping at the first, so one typo doesn't hide the
    /// next.
    pub fn load_file(&mut self, path: &str) -> Result<(), Vec<String>> {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => return Err(vec![format!("cannot read {}: {}", path, e)]),
        };

        let mut problems = Vec::new();
        for (number, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (name, value) = match line.split_once(char::is_whitespace) {
                Some((n, v)) => (n, v.trim()),
                None => {
                    problems.push(format!("line {}: `{}` has no value", number + 1, line));
                    continue;
                }
            };
            if let Err(e) = self.set(&name.to_ascii_lowercase(), value, true) {
                problems.push(format!("line {}: {} `{}`", number + 1, e, name));
            }
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, contents: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("klyro_config_test_{}_{}", std::process::id(), name));
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn every_listed_parameter_is_readable() {
        let config = Config::default();
        for name in PARAMETERS {
            assert!(
                config.get(name).is_some(),
                "{name} is listed but not readable"
            );
        }
        assert_eq!(config.get("nonsense"), None);
    }

    #[test]
    fn set_round_trips_through_get() {
        let mut config = Config::default();
        config.set("maxclients", "50", false).unwrap();
        assert_eq!(config.get("maxclients"), Some("50".to_string()));

        config.set("save-interval", "300", false).unwrap();
        assert_eq!(config.get("save-interval"), Some("300".to_string()));

        config.set("sweep-interval", "250", false).unwrap();
        assert_eq!(config.sweep_interval, Duration::from_millis(250));
    }

    #[test]
    fn bind_and_port_are_immutable_at_runtime() {
        let mut config = Config::default();
        assert_eq!(config.set("port", "1234", false), Err(SetError::Immutable));
        assert_eq!(
            config.set("bind", "127.0.0.1", false),
            Err(SetError::Immutable)
        );
        // The config-file loader is allowed to set them.
        assert!(config.set("port", "1234", true).is_ok());
        assert_eq!(config.port, 1234);
    }

    #[test]
    fn zero_and_junk_are_rejected() {
        let mut config = Config::default();
        for value in ["0", "-1", "", "many"] {
            assert_eq!(
                config.set("maxclients", value, false),
                Err(SetError::BadValue),
                "for {value:?}"
            );
        }
        assert_eq!(config.set("nonsense", "1", false), Err(SetError::Unknown));
    }

    #[test]
    fn load_file_reads_pairs_and_ignores_comments() {
        let path = temp_file(
            "ok",
            "# a comment\n\nport 9999\nmaxclients 42   # trailing comment\n",
        );
        let mut config = Config::default();
        config.load_file(path.to_str().unwrap()).unwrap();
        assert_eq!(config.port, 9999);
        assert_eq!(config.maxclients, 42);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_file_reports_every_problem() {
        let path = temp_file("bad", "nonsense 1\nmaxclients 0\nlonely\nport 8080\n");
        let mut config = Config::default();
        let problems = config.load_file(path.to_str().unwrap()).unwrap_err();
        assert_eq!(problems.len(), 3);
        // Valid lines still applied, so one typo doesn't lose the rest.
        assert_eq!(config.port, 8080);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_file_reports_a_missing_file() {
        let mut config = Config::default();
        assert!(config.load_file("/nonexistent/klyro.conf").is_err());
    }
}
