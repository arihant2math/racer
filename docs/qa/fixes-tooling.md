# Fixes: tooling, track compiler, CLIs, scripts (fix/tooling)

Covers QA-TOOL-1 to QA-TOOL-17 and QA-EDGE-8, QA-EDGE-12, QA-EDGE-13 (QA-EDGE-5/6/7/14/16 are
duplicates and their tests now pass too). Every QA repro test in these areas runs without
`#[ignore]` and passes. Reports: [`tooling-data-build.md`](tooling-data-build.md),
[`adversarial-edge-cases.md`](adversarial-edge-cases.md).

## Majors

### QA-TOOL-12: data paths baked in at compile time

Root cause: `racer_dynamics::spec::cars_dir()` and `racer_trackc::tracks_dir()` were
`env!("CARGO_MANIFEST_DIR")/../../data/...`, so every binary looked in the checkout it was
built from.

Fix: a new crate, `racer-data`, with one function, `racer_data::dir()`. It returns the first of
`RACER_DATA_DIR` (if set and not empty), `data/` next to the executable (if it is a directory),
and the compile-time checkout path (so `cargo run`/`cargo test` need nothing). The two call
sites now call `racer_data::dir().join("cars" | "tracks")`; nothing else changed in the
dynamics crate, so merges with fix/dynamics stay trivial. Documented in the README ("Layout").

Test: done by hand (needs a relocated binary): `target/debug/racer-trackc` copied to a scratch
directory, the checkout's `data/` moved away, then `compile` on a copy of the track:

| Setup | Result |
|---|---|
| no data anywhere | `validate` ok, then `.../crates/racer-data/../../data/cars/vw-golf-gti-mk8/car.toml: No such file or directory` (the old failure, now only when nothing else is found) |
| `RACER_DATA_DIR=<moved data>` | compiles; all generated files byte-identical to the committed ones |
| `data/` copied next to the binary | compiles; byte-identical |

Open: the game's error when no data is found is still the `expect` in `race_scene.rs` (game
code, fix/game-ui); it now names the path it tried.

### QA-TOOL-2: grid slots may overlap or sit ahead of the line

Root cause: `validate` never looked at `grid_row_spacing`. Slots `i` and `i + 2` share a
column `2 * grid_row_spacing` apart, and consecutive slots are only side by side if the
columns are at least a car width apart.

Fix: `grid_row_spacing` must be in `2.5..=15` m (same-column slots at least a 5 m car length
apart; 16 slots stay within 250 m of the line), and if the columns are closer than 2.4 m
(`2 * |grid_column_offset|`) the spacing must be at least 5 m. A positive spacing also keeps
every slot behind the line (slot 0 is 4 m back). The column fit check now uses
`|grid_column_offset|`, so a large negative offset can no longer put cars off the road.

Test: `qa_malformed::rejects_overlapping_or_reversed_grid_slots`, `qa_edge_5_*`, and the
`mutated_tracks_are_rejected_or_sound` property, whose grid-overlap soundness check now also
covers these values (its `known_bad` filter is gone).

### QA-TOOL-3 (+EDGE-5): non-finite race layout values pass validation

Root cause: all race checks were comparisons that NaN fails silently, and none checked
finiteness.

Fix: a `validate_race_layout` step that runs before any geometry is built: `grid_slots`,
`grid_row_spacing` and `checkpoint_spacing` ranges (range checks reject NaN), a finite
`grid_column_offset`, and finite start, sector and sprint-finish points within the plan
extent.

Test: `qa_malformed::rejects_non_finite_race_layout`, `accepted_tracks_have_finite_grid_poses`,
`qa_edge_5_*`.

## Minors

### QA-TOOL-1: `curb_width` never validated

Fix: `curb_width` must be in `0.3..=2` m (rejects NaN, 0 and negatives). Test:
`qa_malformed::rejects_bad_curb_width`.

### QA-TOOL-4: track id written unescaped into `summary.toml`

Root cause: ids were never checked, and `compile` formats them into TOML strings (and file
names) as is.

Fix: `geometry::is_valid_id`: 1 to 64 of `a-z`, `0-9`, `-`, `_` (every shipped id fits).
`validate` rejects other track ids and names with control characters; `compile` rejects car ids
that don't fit before writing anything; `load_dir` rejects them in `summary.toml`. With only
those characters no escaping is needed. Test: `compile_handles_ids_that_need_toml_escaping`,
new `rejects_ids_unsafe_for_files_and_toml`.

Not done: checking the id against the directory name. Tests (and users) validate copies in
directories with other names, and nothing reads the id back except the summary.

### QA-TOOL-5 (+EDGE-6, EDGE-7): `validate` has no size bound

Root cause: `validate` built the geometry before checking anything that sizes it
(`grid_slots` allocates, `checkpoint_spacing` drives a `while s < length` loop that never ends
for 0 or below an f32 ulp), and nothing bounded the track size (1 m samples, O(n²) overlap
check, 0.5 m brute-force projection).

Fix: scalar checks first (above), then bounds before building: 3 to 500 nodes, node
coordinates within 50 km of the origin (also keeps f32 positions to about 1 cm), at most 16
sector ends, and a lap of at most 25 km (computed from the route, which is O(nodes)). The worst
accepted case, a 23 km lap, validates in 2.3 s in the dev profile. The O(n²) overlap check is
kept: the bound makes it cheap enough, and a spatial index would be more code than the bound
justifies.

Test: `validate_terminates_on_huge_tracks` (1e15 scale: instant error), `qa_edge_6_*`
(checkpoint spacing 0 and 1e-4: exit 1 with a `checkpoint_spacing` error), `qa_edge_7_*`
(`grid_slots = i64::MAX`, 1e6 scale), new `rejects_laps_over_the_length_bound_quickly`
(15× scale, 33 km).

### QA-TOOL-6: `--max-ticks 0` runs one tick

Root cause: the limit was only checked in `FixedUpdate`, after the tick counter.

Fix: the same check also runs in `Startup`, so a limit of 0 exits before the first fixed
step. Test: `qa_args::max_ticks_zero_runs_no_ticks`, new in-process
`smoke::zero_max_ticks_exits_before_the_first_tick` (waits 4 ticks' worth before the first
update, then checks `TickCount` is 0).

### QA-TOOL-7 (+EDGE-14): server argument parsing

Fix: `racer-server` parses its arguments strictly: `--max-ticks N` (non-negative integer),
`-h`/`--help`; a missing or malformed value or an unknown argument prints the usage and exits
2. Test: `qa_args::malformed_max_ticks_is_a_clean_usage_error`,
`args::qa_edge_14_bad_max_ticks_is_a_usage_error`.

### QA-TOOL-8: `ai-laps --car <unknown>`, `ai-races --races 0` pass

Fix: unknown car ids, `--laps 0` and `--races 0` are usage errors, before the track is loaded.
Test: `qa_cli::ai_laps_rejects_an_unknown_car`, `ai_races_rejects_zero_races`.

### QA-TOOL-9 (+EDGE-16): `ai-races --first` overflow

Root cause: `first + races`, and race indices are later multiplied as `u64` for the grid
shuffle seed (`race.rs`), which also overflows above 2³².

Fix: `first + races` must not overflow and must be at most `u32::MAX`; `races * laps` uses
`saturating_mul`. Test: `qa_cli::ai_races_rejects_overflowing_first_without_panicking`, new
`ai_races_rejects_first_beyond_the_seed_range`.

### QA-TOOL-10: `diff` accepts NaN and negative tolerances

Fix: `--max-fraction` must be in `0..=1` and `--max-mean` in `0..=255` (NaN and infinities
fail the range check); otherwise a usage error (exit 2). Test: `qa_cli::diff_rejects_nan_tolerances`,
new `diff_rejects_negative_and_oversized_tolerances`.

### QA-TOOL-11: `RACER_BACKEND_ADDR=localhost:8080` panics

Root cause: parsed as `SocketAddr` with `expect`.

Fix: the string goes to `TcpListener::bind`, which resolves `host:port`; any bind error prints
`racer-backend: <addr>: <error>` and exits 1. Test: `qa_addr::hostname_address_does_not_panic`,
new `bad_address_is_a_clean_error`. README shows the host-name form.

### QA-TOOL-13: `telemetry.sh`/`ai-check.sh` resolve `<out-dir>` against the repo

Fix: like `capture.sh`, the scripts no longer `cd` into the repository: cargo runs in a
subshell there, the binary comes from `cargo metadata`'s target directory (so
`CARGO_TARGET_DIR` works), and the plot scripts are called by absolute path. Test: by hand,
`tools/ai-check.sh` from a scratch directory with a relative out dir (see "Runs").

### QA-TOOL-14: `setup.sh` accepts any `cargo`

Fix: rustup is installed whenever `rustup` is missing (not only when `cargo` is), and after
`rustup toolchain install` the script checks that `cargo --version` is the channel pinned in
`rust-toolchain.toml`; otherwise it fails with a message to put `~/.cargo/bin` first on PATH.
Test: by hand: a fake `cargo` printing `cargo 1.75.0` and a no-op `rustup` on PATH → exit 1
with that message; the real setup still passes and is idempotent.

### QA-TOOL-15: `racer-capture scene` validates its arguments late

Fix: the scene name and size (each side `1..=8192`) are checked before the output directory,
window or renderer are created. Test: new `qa_cli::scene_rejects_bad_arguments_before_starting`
(`nosuch`, `0x0`, `1280x0`, `99999x720`: exit 2, no directory created; needs no display).

### QA-TOOL-16: each `-p` build compiles its own Bevy

Root cause: not Bevy's own feature lists. `cargo tree -e features` shows Bevy resolves to the
same features for `-p racer-game`, `-p racer-capture` and `--workspace`; the difference was
two crates deep in everyone's graph: racer-backend's axum enabled `tracing/log` (through its
default `tower-log` feature) and `serde_core/default`. A workspace build therefore compiled
`tracing` and `serde_core` with different features from a `-p racer-game` or `-p racer-capture`
build, and almost all of Bevy (and wgpu, naga, avian) depends on them.

Fix: axum is used without `tower-log` (the backend doesn't use tower's log output), and
`racer-data`, which is in the graph of every crate that uses Bevy, enables `serde_core`'s
default features (an unused dependency, commented as such). `cargo tree` now shows no
differences between `--workspace` and `-p racer-game` or `-p racer-capture`, and after
`cargo build --workspace`, `cargo build -p racer-capture` compiles nothing but the racer
crates' own changes (see "Runs").

Left open (by design): `-p racer-server` and `-p racer-sim` resolve a smaller, headless Bevy
(no `3d`/`ui`), so they compile their own copy the first time. Unifying that would put the
renderer into the server build. Cargo's `resolver.feature-unification = "workspace"` would
fix all of this but still needs `-Zfeature-unification` on 1.97. Test builds (`cargo test`)
also differ from plain builds through dev-dependencies (proptest), as Cargo intends.
Documented in the README ("Build, run, test").

### QA-TOOL-17: `compile` leaves stale generated files

Fix: after writing, `compile` removes `speed-*.csv` files in `generated/` it did not write;
other files are left alone. Test: new `qa_malformed::compile_removes_stale_generated_files`.

### QA-EDGE-8: a short `drive` table in `summary.toml` panics `load_dir`

Root cause: `load_dir` trusted the generated files. (The panic is in `CarModel::drive_at` in
`profile.rs`, solver code that fix/ai may touch, so the fix is in loading instead.)

Fix: `load_dir` checks each car model (valid id; finite, positive mass, accelerations, traction
and top speed; finite non-negative drag and lift; 2 to 1000 finite drive entries) and that
every racing-line offset lies between the walls (rejects NaN), with an error that says to run
`racer-trackc compile`. Test: `qa_edge_8_short_drive_table_panics_load`, new
`load_rejects_corrupt_generated_data` (NaN offset, negative braking, NaN top speed).

### QA-EDGE-12: banking change in the hairpin builds a 31° apron ramp

Fix: after building, `validate` rejects a road or apron triangle steeper than 15° (banking
and grade combined; the shipped track is under 12°), naming the place. The shrunk case (node 6
at -7.64°) is now rejected. The regression test was written to require acceptance and a flat
apron; it now accepts either outcome, as the report's "Expected" allows. The property test's
soundness check now holds the aprons to the same 0.95 normal as the road.

### QA-EDGE-13: road below the ground plane accepted

Fix: `validate` rejects any sample whose road or apron edge (banking included) is below
`GROUND_Y`. The property test's elevation edits now include -3..0 m. Test:
`qa_edge_13_road_below_the_ground_plane_is_accepted`.

## Other changes

The CI "Validate tracks" step runs with `--locked` like the others (the report's nit).

## Runs

All on this branch, in a fresh container after `tools/setup.sh` (4 cores, lavapipe).

| Step | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| `cargo clippy -p racer-game --features wheel --all-targets --locked -- -D warnings` | clean |
| `CARGO_FEATURE_PURE=1 cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings` | clean |
| same for `aarch64-apple-darwin` | clean |
| `cargo test --workspace --locked` | all pass; the only ignored tests left belong to the other fix groups |
| `racer-trackc validate data/tracks/*/` | ok (2143 m, 24 checkpoints, 8 grid slots, unchanged) |
| `racer-trackc compile` on a copy of the shipped track (relocated binary, both data lookups) | every generated file byte-identical to the committed ones |
| `cargo build --workspace`, then `cargo build -p racer-capture`, `-p racer-game` (QA-TOOL-16) | nothing recompiled (0 `Compiling` lines; QA measured over 10 min of Bevy rebuild here before the fix) |
| `tools/capture.sh greybox`/`driving`/`menu`, then `racer-capture diff` against the goldens and `check` | all ok (greybox and menu bit-identical; driving mean error ≤ 0.16, as before) |
| `racer-game` with no args, `race --car 2 --ai 7 --difficulty pro --laps 3`, `race --autopilot`, `drive`, `drive --car 3 --skidpad`, `greybox` under Xvfb for 45 s each | ran until killed, no panics |
| `racer-server --max-ticks 2400`; `--max-ticks` (no value) | `reached 2400 ticks`; usage error, exit 2 |
| `RACER_BACKEND_ADDR=localhost:9000 racer-backend`, `curl localhost:9000/health` | `"ok"` |
| `cargo test -p racer-dynamics` | pass |
| `tools/ai-check.sh rel-ai 5 4 5` and `tools/telemetry.sh rel-tel vw-golf-gti-mk8`, run from a scratch directory whose name has a space (QA-TOOL-13) | outputs written to `<scratch>/rel-ai` and `<scratch>/rel-tel`, nothing in the repository. Telemetry exit 0. ai-check exit 1: every solo row ok, but the mixed-race Alien row fails on 3 resets in traffic at 381-383 m and 954 m (turn 1 and the ~946 m corner), which is QA-AI-2 (fix/ai), as the QA report's cross-area note predicted for this CI step |
| fake distro `cargo 1.75.0` + no-op `rustup` on PATH, `tools/setup.sh` (QA-TOOL-14) | exit 1, "not the pinned 1.97.0"; real setup ok and idempotent |

Not run: the `race` capture (release, 2400 s timeout; unchanged code paths), and the scripts
with `CARGO_TARGET_DIR` set (another full release build; they use the same `cargo metadata`
lookup as `capture.sh`).
