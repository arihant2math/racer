# QA: tooling, data and build

QA pass on 2026-10-27 over `tools/*.sh`, `racer-capture`, `racer-trackc` and the track format,
`racer-server`/`racer-backend` smoke behaviour, `.github/workflows/ci.yml`, toolchain pins and the
README. Fresh container (4 cores, Ubuntu 24.04, lavapipe), starting from `tools/setup.sh`.

Repro tests are ignored with their bug id, so `cargo test --workspace` stays green. Run them with:

```sh
cargo test -p racer-trackc --test qa_malformed -- --ignored
cargo test -p racer-capture --test qa_cli -- --ignored
cargo test -p racer-server --test qa_args -- --ignored
cargo test -p racer-backend --test qa_addr -- --ignored
```

In the repro commands, `$T` stands for a copy of `data/tracks/downtown-loop` with the one-line
edit shown (e.g. `mkdir t && sed 's/^curb_width = 0.9/curb_width = nan/' data/tracks/downtown-loop/track.toml > t/track.toml`).

## Bugs

### QA-TOOL-1: `curb_width` is never validated (minor)

Repro: `$T`: `curb_width = nan` (or `-5.0`, `0.0`); `racer-trackc validate $T`.
Expected: an error such as "curb_width outside 0.3..=2 m". Actual: `ok ... 40094 triangles`; the
curb check `a < d.curb_width + 0.3` is false for NaN, and a negative width passes it. Curb geometry
is then built from the bad width.
Location: `crates/racer-trackc/src/geometry.rs:597` (only use of `curb_width` in `validate`; no
range or finiteness check, unlike `wall_height` at `:573`).
Test: `qa_malformed::rejects_bad_curb_width`. Confidence: high.

### QA-TOOL-2: grid slots may overlap or sit ahead of the start line (major)

Repro: `$T`: `grid_row_spacing = 0.0` (or `1.0`, `-7.0`); `racer-trackc validate $T`.
Expected: rejected; slots `i` and `i + 2` share a column and are `2 * grid_row_spacing` apart, so
anything under a car length (~5 m, i.e. spacing < ~2.5 m) puts two cars in the same place, and a
negative spacing puts the grid past the start line. Actual: `ok`, 8 grid slots. With `0.0`, slots
0/2/4/6 and 1/3/5/7 are each exactly coincident: an 8-car race spawns cars inside each other.
Location: `crates/racer-trackc/src/geometry.rs:683-700` checks slot count, straightness and column
fit but never `grid_row_spacing`. Test: `qa_malformed::rejects_overlapping_or_reversed_grid_slots`.
Confidence: high (validation result); the in-game effect of overlapping spawns is inferred.

### QA-TOOL-3: non-finite race layout values pass validation (major)

Repro: `$T` with any one of `grid_row_spacing = nan`, `grid_column_offset = nan`,
`checkpoint_spacing = nan`, `checkpoint_spacing = inf`, `sprint_finish = [nan, 0.0]`; then
`racer-trackc validate $T`.
Expected: "non-finite value" errors, as nodes get. Actual: `ok` every time. With
`grid_row_spacing = nan` every grid pose is NaN; with `checkpoint_spacing = nan`/`inf` the track
has 2 checkpoints (only the sector ends) instead of 24; a NaN sprint finish is projected to an
arbitrary distance that happens to pass the range check. (`start = [nan, nan]` is rejected, but by
unrelated grid/sprint messages, not by a "non-finite start" error.) All comparisons used
(`off > 3.0`, `checkpoint_spacing < 20.0`, `grid_column_offset + 1.2 > half_width`) are false for NaN.
Location: `crates/racer-trackc/src/geometry.rs:682-726` (race section of `validate`; no
`is_finite` on `RaceLayout`). Tests: `qa_malformed::rejects_non_finite_race_layout`,
`qa_malformed::accepted_tracks_have_finite_grid_poses`. Confidence: high.

### QA-TOOL-4: `compile` writes the track id unescaped into `summary.toml` (minor)

Repro: `$T`: `id = "a\"b"`; `racer-trackc compile $T`.
Expected: the id is rejected by `validate`, or escaped. Actual: `validate` passes, `compile` writes
`generated/summary.toml` containing `id = "a"b"`, then fails reading it back
(`TOML parse error at line 4, column 17`), leaving the broken file on disk. A backslash in the id
does the same. Ids are also never checked against the directory name or a safe character set.
Location: `crates/racer-trackc/src/lib.rs:208` (`id = \"{}\"` via `format!`; same pattern for car
ids at `:220`). Test: `qa_malformed::compile_handles_ids_that_need_toml_escaping`. Confidence: high.

### QA-TOOL-5: `validate` is quadratic with no size bound; huge tracks never finish (minor)

Repro: scale every plan coordinate (`at`, `start`, `sectors`, `sprint_finish`) of the shipped
track by `k` and run `racer-trackc validate`. `k = 10` (22 km): 1.7 s; `k = 40` (88 km): 27 s;
`k = 1e15`: still running after 5 min (killed).
Expected: a bound on coordinates/length (the game cannot use a 1e18 m track) and a fast error.
Actual: the self-overlap check is O(samples^2) (`geometry.rs:664`), `Route::project` walks the whole
route at 0.5 m per call (`:272`), and `build` sizes the sample vector from the length (`:291`), so
validation time and memory grow without limit.
Test: `qa_malformed::validate_terminates_on_huge_tracks` (20 s budget). Confidence: high.

### QA-TOOL-6: `racer-server --max-ticks 0` runs one tick (minor)

Repro: `cargo run -p racer-server -- --max-ticks 0`.
Expected: exits after 0 ticks (or rejects 0). Actual: logs `reached 1 ticks, shutting down`, the
same as `--max-ticks 1`. `count_ticks` runs before the check, so the check first sees 1.
Location: `crates/racer-server/src/lib.rs:43-53`. Test: `qa_args::max_ticks_zero_runs_no_ticks`.
Confidence: high.

### QA-TOOL-7: `racer-server` argument parsing: silent hangs and panics (minor)

Repro and actual:
`racer-server --max-ticks` (no value), `--max-ticks=10`, `--max-tick 10`: run forever with no
warning (the flag is ignored). `--max-ticks abc`, `--max-ticks -1`: panic
(`--max-ticks takes an integer`, exit 101 with a backtrace).
Expected: usage error with exit 2, as `racer-trackc` and `racer-capture` do; unknown arguments
rejected. A scripted smoke run with a typo never terminates.
Location: `crates/racer-server/src/main.rs:4-8`. Test: `qa_args::malformed_max_ticks_is_a_clean_usage_error`.
Confidence: high.

### QA-TOOL-8: `ai-laps --car <unknown>` and `ai-races --races 0` pass (minor)

Repro: `racer-capture ai-laps out --car no-such-car` / `racer-capture ai-races out --races 0`.
Expected: an error (unknown car id; nothing to run). Actual: an empty report ("ordered ok" /
every row "ok"), `solo.md`/`mixed.md` written, exit 0, in under a second. A typo in a car id makes
the AI verification report green without driving a lap.
Location: `crates/racer-capture/src/main.rs:250-253` (filter with no check that a car matched),
`:303-307`. Tests: `qa_cli::ai_laps_rejects_an_unknown_car`, `qa_cli::ai_races_rejects_zero_races`.
Confidence: high.

### QA-TOOL-9: `ai-races --first` overflow panics (minor)

Repro: `racer-capture ai-races out --races 2 --first 18446744073709551615`.
Expected: usage error. Actual: `panicked at crates/racer-capture/src/main.rs:307:36: attempt to
add with overflow` (dev profile; wraps silently in release). Location: `main.rs:307`.
Test: `qa_cli::ai_races_rejects_overflowing_first_without_panicking`. Confidence: high.

### QA-TOOL-10: `diff` accepts NaN and negative tolerances (minor)

Repro: `racer-capture diff g.png g.png --max-fraction nan` (same for `--max-mean nan`, `-1`).
Expected: usage error (exit 2). Actual: `FAIL ... mean error 0.000, differing pixels 0.000%`, exit 1:
identical images reported as a regression. `f64::from_str` accepts `nan`/`inf` and no range check
follows. Location: `crates/racer-capture/src/main.rs:162-166`. Test: `qa_cli::diff_rejects_nan_tolerances`.
Confidence: high.

### QA-TOOL-11: `RACER_BACKEND_ADDR=localhost:8080` panics (minor)

Repro: `RACER_BACKEND_ADDR=localhost:8080 cargo run -p racer-backend`.
Expected: listens (the README just says "RACER_BACKEND_ADDR to change"), or a clean error.
Actual: `panicked at crates/racer-backend/src/main.rs:8:10: RACER_BACKEND_ADDR must be a socket
address`: it is parsed as `SocketAddr`, so host names are not accepted, and the failure is a panic.
Location: `crates/racer-backend/src/main.rs:5-8`. Test: `qa_addr::hostname_address_does_not_panic`.
Confidence: high.

### QA-TOOL-12: data paths are baked in at compile time (major)

Repro: build, then move or remove the checkout's data (simulated:
`cp target/debug/racer-trackc /elsewhere; mv data/cars data/cars.x; /elsewhere/racer-trackc compile <copy of the track>`).
Actual: `racer-trackc: /home/user/racer/crates/racer-dynamics/../../data/cars/vw-golf-gti-mk8/car.toml:
No such file or directory`. The game panics the same way entering a race
(`racer_trackc::load("downtown-loop").expect("track data is checked by tests")`,
`crates/racer-game/src/race_scene.rs:39`).
Expected: binaries find data relative to the executable or a configurable root. PLAN.md section 8
plans unsigned builds on GitHub Releases; those cannot run on any other machine, and a moved or
re-cloned checkout breaks already-built binaries (and `target/` caches restored by CI at a
different path).
Location: `crates/racer-dynamics/src/spec.rs:328` and `crates/racer-trackc/src/lib.rs:32`
(`env!("CARGO_MANIFEST_DIR")`). No test (needs a relocated binary). Confidence: high.

### QA-TOOL-13: `telemetry.sh` and `ai-check.sh` resolve `<out-dir>` against the repo root and ignore `CARGO_TARGET_DIR` (minor)

Repro: `cd /tmp/x && /path/to/racer/tools/telemetry.sh out vw-golf-gti-mk8`.
Expected: output in `/tmp/x/out`, as `tools/capture.sh` does. Actual: both scripts `cd "$ROOT"`
before using the argument, so a relative `<out-dir>` lands in the repository (`racer/out`). They
also run `target/release/racer-capture` directly, so with `CARGO_TARGET_DIR` set they run a stale
binary or fail with "No such file"; `capture.sh` asks `cargo metadata` for the target dir.
Location: `tools/telemetry.sh:14-16`, `tools/ai-check.sh:20-24`. Confidence: high (observed; see "Runs").

### QA-TOOL-14: `setup.sh` accepts any `cargo` on PATH without the pinned toolchain (minor)

Repro: a host with a distro `cargo` and no `rustup` (simulated:
`env PATH=<dir with a cargo that prints "cargo 1.75.0">:/usr/bin:/bin HOME=<empty> tools/setup.sh`).
Expected: install rustup and the toolchain pinned in `rust-toolchain.toml`, or fail. Actual:
`==> cargo 1.75.0 (distro)`, `==> setup complete`, exit 0; the next `cargo build` fails on
edition 2024 / `rust-version = "1.97"`.
Location: `tools/setup.sh:62-73` (rustup is only installed when no `cargo` exists, and the
toolchain step is skipped when `rustup` is absent). Confidence: high.

### QA-TOOL-15: `racer-capture scene` validates its arguments late (minor)

Repro: `xvfb-run -a racer-capture scene greybox out --size 0x0`.
Expected: `invalid --size`. Actual: panic in Bevy
(`bevy_pbr-0.19.1/src/atmosphere/environment.rs:116:64: called Option::unwrap() on a None value`),
exit 1. Related: an unknown scene name (`tools/capture.sh nosuch out`) is only rejected after the
output directory is created and the window and renderer are up (empty `out` left behind).
Location: `crates/racer-capture/src/main.rs:94-118` (size parsed without a lower bound;
`scenes::add_scene` called after `DefaultPlugins` and `create_dir_all`). Confidence: high.

### QA-TOOL-16: each `-p` build compiles its own Bevy (minor, build time)

Repro: `cargo build --workspace --locked` (16 min 47 s from clean on 4 cores), then
`tools/capture.sh greybox out`.
Expected: the capture script reuses the workspace build. Actual: its
`cargo build -p racer-capture` recompiles `bevy_pbr`, `avian3d`, `racer-sim`, `racer-game` and the
rest (over 10 minutes here) because crates enable different Bevy features
(`racer-game`: `3d, ui, free_camera`; `racer-server`: `std, multi_threaded, bevy_log`;
`racer-capture`: `3d`), and Cargo unifies features per build invocation. The README's
`cargo run -p racer-game` / `-p racer-server` and CI's capture job pay the same cost.
Location: `crates/*/Cargo.toml` bevy feature lists; `tools/capture.sh:22`. Confidence: high
(observed recompilation; cause from the manifests).

### QA-TOOL-17: `compile` leaves stale generated files behind (minor)

Repro: put `speed-old-car.csv` in `generated/` (or remove a car from `racer_dynamics::spec::CARS`),
run `racer-trackc compile`. Expected: `generated/` is rewritten as the README says. Actual: files
for cars no longer compiled stay, and look current. Location: `crates/racer-trackc/src/lib.rs:197-244`
(writes files, never clears the directory). Confidence: high (low impact today).

## Cross-area notes

The CI capture job's `tools/ai-check.sh captures/ai 5 4 5` is a short mixed-race run; M2's own
review saw a reduced run (4 races x 3 laps) fail on Alien resets in first-lap traffic, so this step
is a likely source of red CI (AI area). `racer-game race --difficulty <typo>` silently means
"mixed" and `--car`/`--ai`/`--laps` out-of-range values are wrapped or clamped silently
(game-flow area).

## Runs

| Step | Result |
|---|---|
| `tools/setup.sh`, fresh container | ok, 64 s (installed 10 apt packages and the 1.97.0 toolchain) |
| `tools/setup.sh` again, and from `/tmp` | ok, 0.4 s, idempotent |
| `tools/setup.sh --bogus`, `--with-blender=1` | `unknown argument`, exit 2 |
| `tools/setup.sh --with-blender` | ok, 49 s (download, sha256 check, `blender-ok 5.2.3 LTS`) |
| `cargo build --workspace --locked` from clean | ok, 16 min 47 s |
| `tools/capture.sh greybox "<dir with spaces>"` from `/tmp` | ok, 35 s; `diff` against goldens mean error 0.000 |
| `tools/capture.sh greybox out --size 2560x1440` (larger than the Xvfb screen) | ok, 2560x1440 PNGs |
| `tools/capture.sh` with no args, unknown scene, unwritable `/proc/nope` | usage (exit 2), clean errors (exit 2) |
| `cd <dir with spaces> && tools/telemetry.sh rel-out vw-golf-gti-mk8` | ok, 20 min (release build of `racer-capture` from scratch dominates); CSVs and PNGs written to `racer/rel-out`, caller's directory empty (QA-TOOL-13) |
| `CARGO_FEATURE_PURE=1 cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc` / `aarch64-apple-darwin` `-- -D warnings` | clean, 5 min 54 s / 4 min 52 s (with the QA tests) |
| `cargo fmt --all --check`; `cargo clippy -p racer-trackc -p racer-server -p racer-backend -p racer-capture --all-targets --locked -- -D warnings` | clean with the QA tests |

## Checked and fine

`racer-trackc`: unknown fields (`deny_unknown_fields`), missing sections, 0-2 nodes ("need at least
3 nodes"), consecutive duplicate nodes, NaN/inf node values, widths outside 6-25 m, aprons, banking,
radii too large for the room or too tight for the walls, grades,
sectors out of order/duplicate/too short, start/sector/finish off the centreline, `grid_slots` 0 or
over 16, `wall_height` NaN, non-existent paths, a file instead of a directory, unicode and 5000-char
names (a `\u0000` in the name is accepted and printed raw): all clean errors or correct output, exit 1, no panics. `compile` is deterministic: two
parallel compiles of copies are byte-identical to each other and to the committed `generated/`
(3.9 s). Coordinates large enough to overflow `f32` (1e36) give "non-finite value" errors.

`racer-capture`: unknown subcommand/flag and missing flag values give usage errors (exit 2);
`diff` size mismatch (`size mismatch: actual (100, 100), golden (1280, 720)`), missing actual
files, empty golden directory, directory-vs-file, non-PNG input and `--pixel 300` all report
cleanly; `check` rejects single-colour and undecodable images; `telemetry` with an unknown car
fails with exit 1; `scene --timeout 0` times out cleanly with exit 1.

`racer-server`: `--max-ticks 1`, `240` (smoke test) and `u64::MAX` behave. `racer-backend`:
`GET /health` 200 `"ok"`, POST/DELETE 405, unknown route and `/health/` 404, port in use gives a
clean `AddrInUse` error (exit 1), SIGINT shuts down gracefully (exit 0).

CI workflow (read, not run; Actions cannot start jobs on this account): steps, paths and the
`data/tracks/*/` glob are valid (`validate` takes several directories); `rust-cache` runs after
`rustup toolchain install`, so its key includes the pinned toolchain; the wheel-feature clippy only
runs on Linux where `setup.sh` installs `cmake`; `upload-artifact` runs `if: always()`. Nit: the
"Validate tracks" step uses `cargo run` without `--locked` unlike the other steps.

Portability: no Unix-only APIs, `/tmp` or hard-coded `/` separators in Rust sources (paths go
through `Path::join`); no tracked file names differ only by case or use characters invalid on
Windows; car and track ids match their directory names exactly. Ubuntu 24.04's `python3` here is
3.13 without matplotlib, and the scripts correctly fall back to `python3.12`.
