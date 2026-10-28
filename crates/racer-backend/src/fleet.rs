//! Dedicated servers for races, behind the [`Fleet`] trait. [`LocalFleet`]
//! starts `racer-server` processes on this machine (development, tests and
//! a single self-hosted box).

use std::{
    io::{BufRead, BufReader},
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use crate::lobby::{Collisions, Mode, Settings};

/// A race to host.
#[derive(Clone, Debug)]
pub struct RaceRequest {
    pub players: usize,
    pub settings: Settings,
    /// netcode.io key for this race (the server signs nothing else).
    pub key: [u8; 32],
}

/// A started server.
#[derive(Clone, Debug)]
pub struct Allocation {
    /// The address clients connect to.
    pub addr: SocketAddr,
}

/// Something that starts dedicated servers. Blocking: called off the async
/// runtime.
pub trait Fleet: Send + Sync {
    fn allocate(&self, race: &RaceRequest) -> Result<Allocation, String>;
}

/// Starts `racer-server` processes locally.
#[derive(Clone, Debug)]
pub struct LocalFleet {
    pub server: PathBuf,
    /// Interface the servers listen on.
    pub bind: IpAddr,
    /// Address clients are told (the bind address unless that is a wildcard).
    pub public: Option<IpAddr>,
}

impl LocalFleet {
    /// `racer-server` from `RACER_SERVER_BIN`, else next to this executable;
    /// listening on `RACER_FLEET_BIND` (default 127.0.0.1) and advertised as
    /// `RACER_FLEET_PUBLIC` (default: the bind address).
    pub fn from_env() -> Result<Self, String> {
        let server = match std::env::var_os("RACER_SERVER_BIN") {
            Some(p) => PathBuf::from(p),
            None => std::env::current_exe()
                .map_err(|e| e.to_string())?
                .with_file_name(format!("racer-server{}", std::env::consts::EXE_SUFFIX)),
        };
        let ip = |var: &str, default: &str| -> Result<IpAddr, String> {
            let v = std::env::var(var).unwrap_or_else(|_| default.into());
            v.parse().map_err(|_| format!("{var}: not an IP address: {v:?}"))
        };
        let public = match std::env::var("RACER_FLEET_PUBLIC") {
            Ok(_) => Some(ip("RACER_FLEET_PUBLIC", "")?),
            Err(_) => None,
        };
        Ok(LocalFleet {
            server,
            bind: ip("RACER_FLEET_BIND", "127.0.0.1")?,
            public,
        })
    }
}

/// The server's command line for a race.
pub fn server_args(race: &RaceRequest, bind: IpAddr) -> Vec<String> {
    let s = &race.settings;
    let mut args = vec![
        "--bind".to_string(),
        SocketAddr::new(bind, 0).to_string(),
        "--players".into(),
        race.players.to_string(),
        "--lobby-timeout".into(),
        "60".into(),
        "--laps".into(),
        s.laps.to_string(),
        "--mode".into(),
        match s.mode {
            Mode::Circuit => "circuit",
            Mode::Sprint => "sprint",
            Mode::TimeTrial => "time-trial",
        }
        .into(),
        "--collisions".into(),
        match s.collisions {
            Collisions::Full => "full",
            Collisions::GhostFirstLap => "ghost-first-lap",
            Collisions::Off => "off",
        }
        .into(),
    ];
    if !s.ai_fill {
        args.push("--no-ai-fill".into());
    }
    args
}

/// The address in the server's `racer-server listening on ADDR` line.
pub fn parse_listening(line: &str) -> Option<SocketAddr> {
    line.trim()
        .strip_prefix("racer-server listening on ")?
        .parse()
        .ok()
}

impl Fleet for LocalFleet {
    fn allocate(&self, race: &RaceRequest) -> Result<Allocation, String> {
        let mut child = Command::new(&self.server)
            .args(server_args(race, self.bind))
            .env("RACER_SERVER_KEY", crate::hex(&race.key))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("{}: {e}", self.server.display()))?;
        let stdout = child.stdout.take().ok_or("no server stdout")?;
        let (tx, rx) = mpsc::channel();
        // Reads the address, then keeps draining stdout and reaps the
        // process when it exits after its race.
        std::thread::spawn(move || {
            let mut sent = false;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if !sent && let Some(addr) = parse_listening(&line) {
                    tx.send(addr).ok();
                    sent = true;
                }
            }
            child.wait().ok();
        });
        let addr = rx
            .recv_timeout(Duration::from_secs(20))
            .map_err(|_| "the server did not start listening in 20 s".to_string())?;
        let ip = self.public.unwrap_or(if addr.ip().is_unspecified() {
            IpAddr::from([127, 0, 0, 1])
        } else {
            addr.ip()
        });
        Ok(Allocation {
            addr: SocketAddr::new(ip, addr.port()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_listening_line() {
        assert_eq!(
            parse_listening("racer-server listening on 127.0.0.1:4000\n"),
            Some("127.0.0.1:4000".parse().unwrap())
        );
        assert_eq!(parse_listening("INFO something else"), None);
    }

    #[test]
    fn server_arguments_follow_the_settings() {
        let race = RaceRequest {
            players: 3,
            settings: Settings {
                laps: 5,
                mode: Mode::Sprint,
                collisions: Collisions::GhostFirstLap,
                ai_fill: false,
                ..Settings::default()
            },
            key: [0; 32],
        };
        let args = server_args(&race, "0.0.0.0".parse().unwrap()).join(" ");
        assert_eq!(
            args,
            "--bind 0.0.0.0:0 --players 3 --lobby-timeout 60 --laps 5 --mode sprint \
             --collisions ghost-first-lap --no-ai-fill"
        );
    }
}
