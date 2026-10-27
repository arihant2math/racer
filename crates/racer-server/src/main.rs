use std::process::ExitCode;

use bevy::app::AppExit;
use racer_server::{ServerConfig, build_app};

const USAGE: &str = "usage: racer-server [--max-ticks N]";

fn main() -> ExitCode {
    let config = match parse_args(std::env::args().skip(1)) {
        Ok(Some(config)) => config,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("racer-server: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match build_app(&config).run() {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

/// The configuration from the command line; `None` for `--help`.
fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<ServerConfig>, String> {
    let mut config = ServerConfig::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--max-ticks" => {
                let value = args.next().ok_or("--max-ticks needs a value")?;
                let max = value.parse().map_err(|_| {
                    format!("--max-ticks takes a non-negative integer, got {value:?}")
                })?;
                config.max_ticks = Some(max);
            }
            "-h" | "--help" => return Ok(None),
            _ => return Err(format!("unknown argument {arg:?}")),
        }
    }
    Ok(Some(config))
}
