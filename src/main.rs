mod app;
mod client;
mod commands;
mod config;
mod persist;
mod pubsub;
mod resp;
mod server;
mod stats;
mod store;
mod types;
mod util;
mod watch;

use std::env;
use std::process::ExitCode;

use config::Config;

/// Counting every allocation is what lets INFO report real memory use
/// rather than an estimate. See `util/memory.rs` for the cost.
#[global_allocator]
static ALLOCATOR: util::memory::CountingAllocator = util::memory::CountingAllocator;

const KLYRO_NAME: &str = "Klyro";
const KLYRO_TAGLINE: &str = "The high-performance in-memory data server";
pub const KLYRO_VERSION: &str = "0.1.0";
/// Reported as `redis_version` by INFO. Client libraries gate command
/// availability on it, so it names the Redis release whose command
/// shapes Klyro implements - not a claim to be that server.
pub const REDIS_COMPAT_VERSION: &str = "7.0.0";

const USAGE: &str = "usage: klyro [port [dump-file]] | klyro <config-file>";

/// Builds the configuration from the command line.
///
/// Two forms, so the original positional arguments keep working: a
/// numeric first argument is a port (optionally followed by a dump
/// path), and anything else is a config file. Later CLI arguments win
/// over the file, so `klyro klyro.conf` and `klyro 7171` don't need
/// different code paths downstream.
fn config_from_args(args: &[String]) -> Result<Config, String> {
    let mut config = Config::default();
    let mut rest = &args[1..];

    if let Some(first) = rest.first() {
        if first == "--config" {
            let path = rest.get(1).ok_or_else(|| USAGE.to_string())?;
            load_into(&mut config, path)?;
            rest = &rest[2..];
        } else if first.parse::<i64>().is_err() {
            load_into(&mut config, first)?;
            rest = &rest[1..];
        }
    }

    if let Some(port) = rest.first() {
        match port.parse::<i64>() {
            Ok(p) if (1..=65535).contains(&p) => config.port = p as u16,
            _ => return Err(USAGE.to_string()),
        }
    }
    if let Some(dump_path) = rest.get(1) {
        config.dbfilename = dump_path.clone();
    }
    if rest.len() > 2 {
        return Err(USAGE.to_string());
    }
    Ok(config)
}

fn load_into(config: &mut Config, path: &str) -> Result<(), String> {
    config
        .load_file(path)
        .map_err(|problems| format!("{}:\n  {}", path, problems.join("\n  ")))
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    let config = match config_from_args(&args) {
        Ok(c) => c,
        Err(message) => {
            eprintln!("{}", message);
            return ExitCode::FAILURE;
        }
    };

    println!("{} {} - {}", KLYRO_NAME, KLYRO_VERSION, KLYRO_TAGLINE);

    let mut app = app::App::new(config);
    let max_terms = app.config.mem_max_terms_per_doc;
    app.persist.load(&mut app.store, max_terms);

    if let Err(e) = server::run(&mut app) {
        eprintln!("error: {}", e);
        return ExitCode::FAILURE;
    }

    app.save();

    println!("ok");
    ExitCode::SUCCESS
}
