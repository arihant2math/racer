//! Player options, kept in `settings.toml` in [`config_dir`](crate::online::config_dir).
//!
//! The file has one table per section of the Options screen:
//!
//! ```toml
//! [interface]
//! speed_units = "kmh"   # or "mph"
//! ```
//!
//! Loading never fails: a missing file gives the defaults; a file that isn't
//! TOML gives the defaults and a warning; an unknown key or a bad value is
//! skipped with a warning and the rest of the file is kept.
//!
//! To add an option: a field in a section struct (with its default in the
//! struct's `Default`), a variant of [`OptionRow`] with its label, value and
//! change, and the row in [`SECTIONS`]. A new section is a new struct, a
//! field of [`Settings`] and an entry in [`SECTIONS`]. Loading, saving and the
//! Options screen pick it up from there.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Every player option, as stored in `settings.toml`.
#[derive(Resource, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub interface: Interface,
}

/// The "Interface" section.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Interface {
    pub speed_units: SpeedUnit,
}

/// How speeds are shown to the player. The game works in m/s throughout.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpeedUnit {
    #[default]
    #[serde(rename = "kmh", alias = "km/h")]
    Kmh,
    #[serde(rename = "mph")]
    Mph,
}

/// Metres per mile (international mile).
const METRES_PER_MILE: f64 = 1609.344;

impl SpeedUnit {
    pub const ALL: [SpeedUnit; 2] = [SpeedUnit::Kmh, SpeedUnit::Mph];

    /// A speed in m/s, in this unit.
    pub fn from_mps(self, mps: f64) -> f64 {
        match self {
            SpeedUnit::Kmh => mps * 3.6,
            SpeedUnit::Mph => mps * 3600.0 / METRES_PER_MILE,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SpeedUnit::Kmh => "km/h",
            SpeedUnit::Mph => "mph",
        }
    }

    /// A speed in m/s as whole units, right-aligned to `width` digits:
    /// `format(27.8, 3)` is `"100 km/h"`.
    pub fn format(self, mps: f64, width: usize) -> String {
        format!("{:width$.0} {}", self.from_mps(mps), self.label())
    }
}

/// A row of the Options screen: one option of [`Settings`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionRow {
    SpeedUnits,
}

/// The Options screen, section by section.
pub const SECTIONS: &[(&str, &[OptionRow])] = &[("Interface", &[OptionRow::SpeedUnits])];

impl OptionRow {
    pub fn label(self) -> &'static str {
        match self {
            OptionRow::SpeedUnits => "Speed units",
        }
    }

    pub fn value(self, s: &Settings) -> String {
        match self {
            OptionRow::SpeedUnits => s.interface.speed_units.label().into(),
        }
    }

    /// Steps the option one value forward (`dir` 1) or back (-1).
    pub fn change(self, s: &mut Settings, dir: i32) {
        match self {
            OptionRow::SpeedUnits => {
                s.interface.speed_units =
                    crate::flow::cycle(&SpeedUnit::ALL, s.interface.speed_units, dir)
            }
        }
    }
}

/// Where the settings are saved. Only the game binary inserts it (see
/// [`SettingsFile::load`]): tests and captures run on the defaults and never
/// touch the player's file.
#[derive(Resource, Clone, Debug)]
pub struct SettingsFile(pub PathBuf);

impl SettingsFile {
    /// `settings.toml` in the config directory.
    pub fn default_path() -> PathBuf {
        crate::online::config_dir().join("settings.toml")
    }

    /// Reads the file (see the module docs for what happens to bad files).
    pub fn load(&self) -> Settings {
        match std::fs::read_to_string(&self.0) {
            Ok(text) => {
                let (settings, problems) = parse(&text);
                for p in problems {
                    warn!("{}: {p}", self.0.display());
                }
                settings
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!("{}: not there yet, default options", self.0.display());
                Settings::default()
            }
            Err(e) => {
                warn!("{}: {e}; default options", self.0.display());
                Settings::default()
            }
        }
    }

    /// Writes the file (through a temporary file, so a crash never leaves
    /// half of it).
    pub fn save(&self, settings: &Settings) -> Result<(), String> {
        let text = toml::to_string(settings).map_err(|e| e.to_string())?;
        write_atomic(&self.0, &text).map_err(|e| format!("{}: {e}", self.0.display()))
    }
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Parses a settings file: the settings, and what was wrong with it.
///
/// Each known key is taken on its own, so one bad value or unknown key costs
/// only itself; a file that is not TOML at all gives the defaults.
pub fn parse(text: &str) -> (Settings, Vec<String>) {
    let mut problems = Vec::new();
    let file: toml::Table = match toml::from_str(text) {
        Ok(t) => t,
        Err(e) => {
            problems.push(format!("not valid TOML ({e}); default options"));
            return (Settings::default(), problems);
        }
    };
    let Ok(mut accepted) = toml::Table::try_from(Settings::default()) else {
        return (Settings::default(), problems);
    };
    for (section, value) in file {
        let Some(defaults) = accepted.get(&section).cloned() else {
            problems.push(format!("unknown option \"{section}\", ignored"));
            continue;
        };
        match (value, defaults) {
            (toml::Value::Table(keys), toml::Value::Table(known)) => {
                for (key, value) in keys {
                    if !known.contains_key(&key) {
                        problems.push(format!("unknown option \"{section}.{key}\", ignored"));
                        continue;
                    }
                    let mut candidate = accepted.clone();
                    if let Some(toml::Value::Table(t)) = candidate.get_mut(&section) {
                        t.insert(key.clone(), value.clone());
                    }
                    if candidate.clone().try_into::<Settings>().is_ok() {
                        accepted = candidate;
                    } else {
                        problems.push(format!(
                            "bad value {value} for \"{section}.{key}\", default kept"
                        ));
                    }
                }
            }
            (value, toml::Value::Table(_)) => {
                problems.push(format!(
                    "\"{section}\" should be a table, not {value}; ignored"
                ));
            }
            (value, _) => {
                let mut candidate = accepted.clone();
                candidate.insert(section.clone(), value.clone());
                if candidate.clone().try_into::<Settings>().is_ok() {
                    accepted = candidate;
                } else {
                    problems.push(format!("bad value {value} for \"{section}\", default kept"));
                }
            }
        }
    }
    (accepted.try_into().unwrap_or_default(), problems)
}

/// Keeps [`Settings`] as a resource (defaults unless already inserted) and
/// saves it whenever it changes, if a [`SettingsFile`] is set.
pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Settings>().add_systems(
            Last,
            save_on_change
                .run_if(resource_exists::<SettingsFile>)
                .run_if(resource_changed::<Settings>),
        );
    }
}

fn save_on_change(settings: Res<Settings>, file: Res<SettingsFile>, mut first: Local<bool>) {
    // The first run sees the settings as loaded, not changed.
    if !std::mem::replace(&mut *first, true) {
        return;
    }
    match file.save(&settings) {
        Ok(()) => info!("{}: options saved", file.0.display()),
        Err(e) => warn!("options not saved: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_conversion() {
        assert!((SpeedUnit::Kmh.from_mps(10.0) - 36.0).abs() < 1e-9);
        // 1 mph = 0.44704 m/s exactly.
        assert!((SpeedUnit::Mph.from_mps(0.44704) - 1.0).abs() < 1e-9);
        assert!((SpeedUnit::Mph.from_mps(100.0 / 3.6) - 62.137_119).abs() < 1e-5);
    }

    #[test]
    fn speed_formatting() {
        assert_eq!(SpeedUnit::Kmh.format(100.0 / 3.6, 3), "100 km/h");
        assert_eq!(SpeedUnit::Mph.format(100.0 / 3.6, 3), " 62 mph");
        assert_eq!(SpeedUnit::Kmh.format(0.0, 3), "  0 km/h");
        assert_eq!(SpeedUnit::Mph.format(60.0 * 0.44704, 5), "   60 mph");
    }

    #[test]
    fn round_trip() {
        let mut s = Settings::default();
        s.interface.speed_units = SpeedUnit::Mph;
        let text = toml::to_string(&s).unwrap();
        assert!(
            text.contains("[interface]") && text.contains("speed_units = \"mph\""),
            "{text}"
        );
        assert_eq!(parse(&text), (s, vec![]));
    }

    #[test]
    fn file_round_trip_and_missing_file() {
        let dir = std::env::temp_dir().join(format!("racer-settings-{}", std::process::id()));
        let file = SettingsFile(dir.join("sub").join("settings.toml"));
        assert_eq!(file.load(), Settings::default(), "missing file");
        let mut s = Settings::default();
        s.interface.speed_units = SpeedUnit::Mph;
        file.save(&s).unwrap();
        assert_eq!(file.load(), s);
        assert!(!file.0.with_extension("toml.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bad_files_fall_back_to_defaults() {
        for text in [
            "",
            "\0\u{1}garbage[[",
            "interface = 3",
            "[interface]\nspeed_units = 7",
        ] {
            let (s, _) = parse(text);
            assert_eq!(s, Settings::default(), "{text:?}");
        }
        let (_, problems) = parse("not toml at all");
        assert_eq!(problems.len(), 1, "{problems:?}");
    }

    #[test]
    fn unknown_keys_and_bad_values_cost_only_themselves() {
        let (s, problems) = parse(
            "volume = 3\n[interface]\nspeed_units = \"mph\"\ncolour = \"red\"\n[graphics]\nshadows = true\n",
        );
        assert_eq!(s.interface.speed_units, SpeedUnit::Mph);
        assert_eq!(problems.len(), 3, "{problems:?}");
        let (s, problems) = parse("[interface]\nspeed_units = \"furlongs\"\n");
        assert_eq!(s, Settings::default());
        assert_eq!(problems.len(), 1, "{problems:?}");
        // "km/h" is accepted as well as "kmh".
        assert_eq!(
            parse("[interface]\nspeed_units = \"km/h\"\n")
                .0
                .interface
                .speed_units,
            SpeedUnit::Kmh
        );
    }
}
