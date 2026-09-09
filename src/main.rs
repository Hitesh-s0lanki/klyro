mod app;
mod commands;
mod persist;
mod server;
mod store;
mod types;
mod util;

use std::env;
use std::process::ExitCode;

const KLYRO_NAME: &str = "Klyro";
const KLYRO_TAGLINE: &str = "The high-performance in-memory data server";
const KLYRO_VERSION: &str = "0.1.0";

const DEFAULT_PORT: u16 = 7171;
const DEFAULT_DUMP_PATH: &str = "klyro.dump";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    let port: u16 = if args.len() > 1 {
        match args[1].parse::<i32>() {
            Ok(p) if p > 0 && p <= 65535 => p as u16,
            _ => {
                eprintln!("usage: {} [port] [dump-file]", args[0]);
                return ExitCode::FAILURE;
            }
        }
    } else {
        DEFAULT_PORT
    };
    let dump_path = args.get(2).map(String::as_str).unwrap_or(DEFAULT_DUMP_PATH);

    println!("{} {} - {}", KLYRO_NAME, KLYRO_VERSION, KLYRO_TAGLINE);

    let mut app = app::App::new(dump_path);
    app.persist.load(&mut app.store);

    if let Err(e) = server::run(port, &mut app) {
        eprintln!("error: {}", e);
        return ExitCode::FAILURE;
    }

    app.persist.save(&mut app.store);

    println!("ok");
    ExitCode::SUCCESS
}
