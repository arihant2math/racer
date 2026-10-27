//! District style rules (`data/city/districts/<id>.toml`).

use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    /// Glass curtain wall tower on a podium.
    Curtain,
    /// Stone/render office block with punched windows and a cornice.
    Masonry,
    /// Brick warehouse loft with large windows, fire escapes, water tower.
    Loft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wall {
    Brick,
    Plaster,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Archetype {
    pub name: String,
    pub style: Style,
    pub weight: f32,
    pub floors: [u32; 2],
    pub floor_height: f32,
    pub ground_floor: f32,
    pub bay_width: [f32; 2],
    /// Window width and height as fractions of the bay / floor.
    #[serde(default = "default_window")]
    pub window: [f32; 2],
    #[serde(default)]
    pub podium_floors: [u32; 2],
    #[serde(default)]
    pub setback: f32,
    pub wall: Wall,
    #[serde(default)]
    pub fire_escapes: f32,
    #[serde(default)]
    pub min_lot_width: f32,
}

fn default_window() -> [f32; 2] {
    [0.5, 0.55]
}

pub type Rgb = [f32; 3];

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Palette {
    pub brick: Vec<Rgb>,
    pub plaster: Vec<Rgb>,
    pub stone: Vec<Rgb>,
    pub glass: Vec<Rgb>,
    pub awning: Vec<Rgb>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Ground {
    pub shopfront: f32,
    pub awning: f32,
    pub bay_width: [f32; 2],
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Roof {
    pub ac_units_per_100m2: f32,
    pub water_tower: f32,
    pub bulkhead: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Street {
    pub lamp_spacing: f32,
    pub tree_spacing: f32,
    pub tree_chance: f32,
    pub bin_spacing: f32,
    pub hydrant_spacing: f32,
    pub bench_chance: f32,
    pub meter_spacing: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct District {
    pub name: String,
    pub seed: u64,
    pub lot_width: [f32; 2],
    #[serde(rename = "archetype")]
    pub archetypes: Vec<Archetype>,
    pub palette: Palette,
    pub ground: Ground,
    pub roof: Roof,
    pub street: Street,
}

impl District {
    pub fn parse(text: &str) -> Result<Self, String> {
        let d: District = toml::from_str(text).map_err(|e| e.to_string())?;
        d.check()?;
        Ok(d)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The Downtown rules shipped in the repository.
    pub fn downtown() -> Self {
        Self::parse(include_str!("../../../data/city/districts/downtown.toml"))
            .expect("data/city/districts/downtown.toml is valid")
    }

    fn check(&self) -> Result<(), String> {
        let range = |name: &str, r: [f32; 2]| {
            if r[0] > 0.0 && r[0] <= r[1] {
                Ok(())
            } else {
                Err(format!("{name}: invalid range {r:?}"))
            }
        };
        range("lot_width", self.lot_width)?;
        range("ground.bay_width", self.ground.bay_width)?;
        if self.archetypes.is_empty() {
            return Err("no archetypes".into());
        }
        for a in &self.archetypes {
            range(&format!("{}.bay_width", a.name), a.bay_width)?;
            if a.floors[0] == 0 || a.floors[0] > a.floors[1] || a.weight < 0.0 {
                return Err(format!("{}: invalid floors or weight", a.name));
            }
        }
        let p = &self.palette;
        for (name, list) in [("brick", &p.brick), ("plaster", &p.plaster), ("stone", &p.stone), ("glass", &p.glass), ("awning", &p.awning)] {
            if list.is_empty() || list.iter().flatten().any(|v| !(0.0..=1.0).contains(v)) {
                return Err(format!("palette.{name}: empty or out of 0..1"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downtown_rules_parse() {
        let d = District::downtown();
        assert_eq!(d.name, "Downtown");
        assert_eq!(d.archetypes.len(), 3);
        assert!(d.archetypes.iter().any(|a| a.style == Style::Curtain));
    }

    #[test]
    fn bad_ranges_are_rejected() {
        let text = include_str!("../../../data/city/districts/downtown.toml").replace("lot_width = [12.0, 30.0]", "lot_width = [30.0, 12.0]");
        assert!(District::parse(&text).unwrap_err().contains("lot_width"));
    }
}
