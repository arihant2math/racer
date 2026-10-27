//! City/track compiler. M0 skeleton: argument handling only; validation and
//! mesh generation arrive with the first track data (M1/M2).

use std::process::ExitCode;

const USAGE: &str = "usage: racer-trackc <validate|compile> <data-dir>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--version" => {
            println!("racer-trackc {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [cmd, dir] if cmd == "validate" || cmd == "compile" => {
            eprintln!("racer-trackc: `{cmd}` is not implemented yet ({dir})");
            ExitCode::from(3)
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
