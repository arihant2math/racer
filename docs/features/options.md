# Options screen, speed units, app icon (`feat/options`)

## What was built

**Options screen.** The main menu has an Options item (between Direct connect and Quit) and so
does the pause menu (offline: Resume, Restart race, Options, Main menu; online: Resume, Options,
Leave). From the main menu it is its own state, `AppState::Options`; in a race it replaces the
pause menu as an overlay (the race stays paused offline) and Back or Esc brings the pause menu
back with the cursor on Options. Same menu code as every other screen (`flow.rs`): arrows or
d-pad, Left/Right or Enter/South change a row, Esc/East/Backspace go back, mouse clicks work,
`FitToWindow` scales the panel. Screens can now have section headings (`Row::Heading`, through
`screen_rows`; `screen` is unchanged for every other menu).

**Speed units.** `settings::SpeedUnit` (`kmh`, `mph`) with `from_mps` and `format(mps, width)`.
The race HUD speedometer and the driving-scene debug overlay (`driving.rs`) use it. No other
player-visible speed exists: the results table, lobby and setup screens show none. Internals stay
in m/s; the telemetry CSVs (`racer-capture`) and `racer-trackc` output stay in km/h. With the
default (km/h) both texts are byte-for-byte what they were, so no golden changed for this.

**Settings file.** `crates/racer-game/src/settings.rs`. `settings.toml` in `online::config_dir()`
(`RACER_CONFIG_DIR`, else e.g. `~/.config/racer`):

```toml
[interface]
speed_units = "mph"     # or "kmh" ("km/h" is accepted too)
```

Only the game binary (`main.rs`) inserts a `SettingsFile`: it loads it at startup before the
plugins run, and `SettingsPlugin` (in `GamePlugin`) saves whenever the `Settings` resource
changes (not on the first frame), through a temporary file and a rename. Tests and
`racer-capture` never insert a `SettingsFile`, so they run on the defaults and never read or
write the player's file. Loading never panics:

| File | Result |
|---|---|
| missing | defaults, `INFO ... not there yet, default options` |
| unreadable / not TOML | defaults, one `WARN` with the parse error |
| unknown table or key (`[graphics]` before that exists, `interface.colour`) | that key ignored, `WARN unknown option "..."`, the rest kept |
| bad value (`speed_units = "furlongs"`, `interface = 3`) | default for that key only, `WARN bad value ...` |

Each known key is tried on its own against the defaults, so this works for any field added later
with no extra code.

**App icon.** `tools/make_icon.py` (Pillow; Inter Display Black Italic, falls back to other bold
fonts or takes a font path) draws a white italic R with a soft shadow on a rounded tile in the
menu's orange, each size drawn separately (no margin and a larger letter at 16-32 px, so the
small ones stay legible). Output in `assets/icon/`: `icon-{16,24,32,48,64,128,256,512,1024}.png`,
`icon.ico` (16-256), `icon.icns` (16-1024), plus `racer.desktop`. `.gitignore` now ignores
`/assets/*` except `assets/icon/` (built car models stay uncommitted).

`crates/racer-game/src/icon.rs` embeds `icon-64.png` (`include_bytes!`, so it never depends on the
asset directory) and `WindowIconPlugin` sets it on the primary window through
`bevy::winit::WINIT_WINDOWS` once winit has made the window (main thread, `NonSendMarker`). The
window's `name` is `racer`: the X11 `WM_CLASS` and the Wayland app id, which the `.desktop` file's
`StartupWMClass=racer` / `Icon=racer` match. winit 0.30 sets the icon on Windows and X11 only;
Wayland and macOS ignore it (see gaps).

## Adding an option or a section

The example is a Graphics section (planned for `feat/render`, which was cancelled); any other section works the same way.

1. In `settings.rs`, a section struct with `#[derive(Serialize, Deserialize, Clone, Debug,
   Default, PartialEq)]` and `#[serde(default)]` (write `impl Default` by hand if a default is not
   the type's zero), and a field for it in `Settings`: `pub graphics: Graphics`. The table name is
   the field name, `[graphics]`.
2. One `OptionRow` variant per row, with its `label`, `value` and `change` arms (`flow::cycle`
   steps through a list of values; `dir` is +1 or -1).
3. One `SECTIONS` entry: `("Graphics", &[OptionRow::Shadows, ...])`.

That is all: the Options screen lists the section under its heading, the row is drawn and changed
by the generic menu code, the file loads, saves and tolerates bad values for the new keys. Read the
values anywhere with `Res<Settings>` (it always exists under `GamePlugin`; use
`Option<Res<Settings>>` in code that also runs without it, as `driving.rs` does), and react to
changes with `resource_changed::<Settings>`. If the panel grows too tall, `FitToWindow` scales it
down; a long list may want a scroll instead.

## Verification

All on this branch's head, exit codes checked:

| Check | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| cross clippy `x86_64-pc-windows-msvc`, `aarch64-apple-darwin` (`CARGO_FEATURE_PURE=1`) | both clean |
| `cargo test --workspace` | all pass, ignored counts unchanged |

New tests: `settings.rs` (conversion: 1 mph = 0.44704 m/s, 100 km/h = 62.137 mph; formatting and
padding; TOML round trip; file round trip into a new directory with no `.tmp` left; missing file;
garbage, wrong types and bad values give defaults; unknown keys and a bad value cost only
themselves), `icon.rs` (the embedded PNG decodes to 64x64 RGBA), and `tests/options.rs`, headless
like `qa_flow.rs`: Options from the main menu by keyboard (Right/Left/Enter change the units, Back
returns), by gamepad (d-pad, South, East) and back with Esc, from the pause menu in a race (race
stays paused, Esc returns to the pause menu, after Resume the HUD reads `... mph` and no
`km/h`), and a change saved to a `SettingsFile` and loaded again by a new app (no file written
before a change).

Window icon, under Xvfb with `target/debug/racer-game` (X11): `xprop` on the "Racer" window gave
`_NET_WM_ICON(CARDINAL) = Icon (64 x 64)` and `WM_CLASS = "", "racer"`; I decoded the property's
4098 cardinals back into an image and it is the orange R. The game logged
`.../settings.toml: not there yet, default options` on that first start.

Screenshots I looked at: all icon PNGs (16-64 px side by side at 4x, and 256 px; the R reads at
16 px); `tools/capture.sh menu` → `main-menu.png` (Options item between Direct connect and Quit)
and `options.png` (heading "Interface", row `Speed units  <  km/h  >`, Back); `tools/capture.sh
units` → `hud-mph.png` (solo race on autopilot, HUD `83 mph`, gear 3). Goldens: `menu/main-menu.png`
changed (the new item, 4.2 % of pixels) and `menu/options.png` is new; `menu/setup.png` matches.
The `race` and `driving` goldens are unaffected because the km/h strings are unchanged.

## Known gaps

The `.ico` is not embedded in the Windows exe: that needs a resource compiler in a build script
(`embed-resource`/`winresource` with `rc.exe` or `llvm-rc`), which would also run under the
cross-target clippy; left for M7 distribution, along with a macOS app bundle using `icon.icns`
and installing `racer.desktop` plus the PNGs into `hicolor` on Linux. On Wayland (and macOS)
winit 0.30 cannot set a window icon; the compositor shows the `.desktop` icon once installed.
Windows was not run, only cross-checked. The `units` capture has no golden and is not in CI. The
Options screen has one option for now, and leaving it puts the main-menu cursor back on Race
rather than Options.
