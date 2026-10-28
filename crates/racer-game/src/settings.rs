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
    /// Rounds half away from zero, and never shows `-0`.
    pub fn format(self, mps: f64, width: usize) -> String {
        let whole = self.from_mps(mps).round() + 0.0;
        format!("{whole:width$.0} {}", self.label())
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
    ///
    /// Keys this version doesn't know (written by a newer one) are kept. A
    /// file that isn't TOML at all is kept as `settings.toml.bad` before it is
    /// replaced.
    pub fn save(&self, settings: &Settings) -> Result<(), String> {
        let ours = toml::Table::try_from(settings).map_err(|e| e.to_string())?;
        let merged = match std::fs::read_to_string(&self.0) {
            Ok(text) => match toml::from_str::<toml::Table>(&text) {
                Ok(existing) => merge(existing, ours),
                Err(_) => {
                    let mut bad = self.0.as_os_str().to_owned();
                    bad.push(".bad");
                    if std::fs::copy(&self.0, &bad).is_ok() {
                        warn!(
                            "{}: not valid TOML, kept as {}",
                            self.0.display(),
                            Path::new(&bad).display()
                        );
                    }
                    ours
                }
            },
            Err(_) => ours,
        };
        let text = toml::to_string(&merged).map_err(|e| e.to_string())?;
        write_atomic(&self.0, &text).map_err(|e| format!("{}: {e}", self.0.display()))
    }
}

/// `existing` with every key of `ours` written over it, section by section.
fn merge(mut existing: toml::Table, ours: toml::Table) -> toml::Table {
    for (section, value) in ours {
        match (existing.get_mut(&section), value) {
            (Some(toml::Value::Table(old)), toml::Value::Table(new)) => old.extend(new),
            (_, value) => {
                existing.insert(section, value);
            }
        }
    }
    existing
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    // One temporary file per process: two games saving at once each rename
    // a whole file into place, and the last one wins.
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp);
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(text.as_bytes())?;
        f.sync_all()
    });
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        std::fs::remove_file(&tmp).ok();
    }
    result
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
        // Reversing slowly in the driving overlay: no "-0".
        assert_eq!(SpeedUnit::Kmh.format(-0.1, 5), "    0 km/h");
        assert_eq!(SpeedUnit::Mph.format(-5.0, 5), "  -11 mph");
        // Halves round up, the way a speedometer needle reads.
        assert_eq!(SpeedUnit::Kmh.format(62.5 / 3.6, 3), " 63 km/h");
        assert_eq!(SpeedUnit::Mph.format(f64::NAN, 3), "NaN mph");
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
        let names: Vec<_> = std::fs::read_dir(dir.join("sub"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["settings.toml"], "temporary file left");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("racer-settings-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn saving_keeps_keys_from_newer_versions() {
        let dir = scratch("newer");
        let file = SettingsFile(dir.join("settings.toml"));
        std::fs::write(
            &file.0,
            "[interface]\nspeed_units = \"furlongs\"\nhud_scale = 1.5\n[graphics]\nshadows = true\n",
        )
        .unwrap();
        let mut s = file.load();
        s.interface.speed_units = SpeedUnit::Mph;
        file.save(&s).unwrap();
        let text = std::fs::read_to_string(&file.0).unwrap();
        let table: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(
            table["interface"]["speed_units"].as_str(),
            Some("mph"),
            "{text}"
        );
        assert_eq!(
            table["interface"]["hud_scale"].as_float(),
            Some(1.5),
            "{text}"
        );
        assert_eq!(table["graphics"]["shadows"].as_bool(), Some(true), "{text}");
        assert_eq!(file.load(), s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_corrupt_file_is_kept_aside_when_replaced() {
        let dir = scratch("corrupt");
        let file = SettingsFile(dir.join("settings.toml"));
        std::fs::write(&file.0, b"\xff\xfe[[[not toml").unwrap();
        assert_eq!(file.load(), Settings::default());
        std::fs::write(&file.0, "[[[not toml").unwrap();
        assert_eq!(file.load(), Settings::default());
        let mut s = Settings::default();
        s.interface.speed_units = SpeedUnit::Mph;
        file.save(&s).unwrap();
        assert_eq!(file.load(), s);
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.toml.bad")).unwrap(),
            "[[[not toml"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unreadable_and_unwritable_paths_give_errors_not_panics() {
        let dir = scratch("unwritable");
        // A directory where the file should be: defaults, and saving fails.
        let file = SettingsFile(dir.join("settings.toml"));
        std::fs::create_dir(&file.0).unwrap();
        assert_eq!(file.load(), Settings::default());
        assert!(file.save(&Settings::default()).is_err());
        // A file where the directory should be (works as root too).
        std::fs::write(dir.join("plain"), "").unwrap();
        let file = SettingsFile(dir.join("plain").join("settings.toml"));
        assert_eq!(file.load(), Settings::default());
        assert!(file.save(&Settings::default()).is_err());
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "temporary file left: {names:?}");
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
