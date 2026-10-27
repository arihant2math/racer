use racer_server::{ServerConfig, build_app};

fn main() -> bevy::app::AppExit {
    let max_ticks = std::env::args()
        .skip_while(|a| a != "--max-ticks")
        .nth(1)
        .map(|v| v.parse().expect("--max-ticks takes an integer"));
    build_app(&ServerConfig { max_ticks }).run()
}
