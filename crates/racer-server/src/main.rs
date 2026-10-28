use std::process::ExitCode;

use bevy::app::AppExit;
use racer_server::{ServerConfig, build_app};
use racer_sim::{
    ai::Difficulty,
    net::protocol::Conditions,
    race::{Collisions, RaceMode},
};

const USAGE: &str = "usage: racer-server [--max-ticks N] [--bind ADDR] [--players N] \
[--lobby-timeout SECS] [--mode circuit|sprint|time-trial] [--laps N] \
[--collisions full|ghost-first-lap|off] [--grid N] [--no-ai-fill] [--difficulty NAME] \
[--conditions RTT_MS[,JITTER_MS[,LOSS_PERCENT]]] [--report FILE] [--linger SECS]

The netcode key is read from RACER_SERVER_KEY (64 hex digits); without it the
development key is used and clients can connect directly.";

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
    let config = match std::env::var("RACER_SERVER_KEY") {
        Ok(hex) => match parse_key(&hex) {
            Some(key) => ServerConfig { key, ..config },
            None => {
                eprintln!("racer-server: RACER_SERVER_KEY must be 64 hex digits");
                return ExitCode::from(2);
            }
        },
        Err(_) => config,
    };
    let mut app = build_app(&config);
    racer_sim::finish_headless(&mut app);
    match app.run() {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

fn parse_key(hex: &str) -> Option<[u8; 32]> {
    let hex = hex.trim();
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut key = [0; 32];
    for (i, b) in key.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(key)
}

/// The configuration from the command line; `None` for `--help`.
fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<ServerConfig>, String> {
    let mut config = ServerConfig::default();
    while let Some(arg) = args.next() {
        let mut value = |what: &str| args.next().ok_or_else(|| format!("{arg} needs {what}"));
        fn number<T: std::str::FromStr>(arg: &str, v: String, what: &str) -> Result<T, String> {
            v.parse()
                .map_err(|_| format!("{arg} takes {what}, got {v:?}"))
        }
        match arg.as_str() {
            "--max-ticks" => {
                let v = value("a value")?;
                config.max_ticks = Some(number(&arg, v, "a non-negative integer")?);
            }
            "--bind" => {
                let v = value("an address")?;
                config.bind = number(&arg, v, "an address like 0.0.0.0:7777")?;
            }
            "--players" => config.players = number(&arg, value("a count")?, "a count")?,
            "--grid" => {
                config.grid = number(&arg, value("a count")?, "a count")?;
                if !(1..=8).contains(&config.grid) {
                    return Err("--grid takes 1 to 8".into());
                }
            }
            "--lobby-timeout" => {
                let v: f32 = number(&arg, value("seconds")?, "seconds")?;
                if !(v.is_finite() && v >= 0.0) {
                    return Err("--lobby-timeout takes seconds".into());
                }
                config.lobby_timeout = v;
            }
            "--linger" => {
                let v: f32 = number(&arg, value("seconds")?, "seconds")?;
                if !(v.is_finite() && v >= 0.0) {
                    return Err("--linger takes seconds".into());
                }
                config.linger = v;
            }
            "--laps" => {
                config.race.laps = number(&arg, value("a count")?, "a count")?;
                if !(1..=50).contains(&config.race.laps) {
                    return Err("--laps takes 1 to 50".into());
                }
            }
            "--mode" => {
                config.race.mode = match value("a mode")?.as_str() {
                    "circuit" => RaceMode::Circuit,
                    "sprint" => RaceMode::Sprint,
                    "time-trial" => RaceMode::TimeTrial,
                    m => return Err(format!("unknown mode {m:?}")),
                }
            }
            "--collisions" => {
                config.race.collisions = match value("full, ghost-first-lap or off")?.as_str() {
                    "full" => Collisions::Full,
                    "ghost-first-lap" => Collisions::GhostFirstLap,
                    "off" => Collisions::Off,
                    c => return Err(format!("unknown collisions option {c:?}")),
                }
            }
            "--no-ai-fill" => config.ai_fill = false,
            "--difficulty" => {
                let d = value("a difficulty")?;
                config.difficulty = Some(
                    Difficulty::ALL
                        .into_iter()
                        .find(|x| x.name().eq_ignore_ascii_case(&d))
                        .ok_or_else(|| format!("unknown difficulty {d:?}"))?,
                );
            }
            "--conditions" => config.conditions = Conditions::parse(&value("conditions")?)?,
            "--report" => config.report = Some(value("a file")?.into()),
            "-h" | "--help" => return Ok(None),
            _ => return Err(format!("unknown argument {arg:?}")),
        }
    }
    Ok(Some(config))
}
