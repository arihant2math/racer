//! `data/tracks/<id>/track.toml`: the route as corner nodes with per-node
//! width, elevation, banking, aprons and curbs, plus the race layout (start,
//! grid, sectors, checkpoints, sprint finish). See the shipped tracks for an
//! annotated example.

use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackFile {
    pub id: String,
    pub name: String,
    pub defaults: Defaults,
    #[serde(rename = "node")]
    pub nodes: Vec<NodeFile>,
    pub race: RaceLayout,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    pub width: f32,
    pub apron: [f32; 2],
    pub radius: f32,
    pub elevation: f32,
    pub bank_deg: f32,
    pub wall_height: f32,
    pub curb_width: f32,
}

/// A node as written; unset fields take the defaults.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeFile {
    pub at: [f32; 2],
    pub radius: Option<f32>,
    pub width: Option<f32>,
    pub apron: Option<[f32; 2]>,
    pub elevation: Option<f32>,
    pub bank_deg: Option<f32>,
    #[serde(default)]
    pub curbs: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaceLayout {
    /// Start/finish line, projected onto the route.
    pub start: [f32; 2],
    pub grid_slots: usize,
    pub grid_row_spacing: f32,
    pub grid_column_offset: f32,
    /// Points that end sectors 1..n-1, projected onto the route.
    pub sectors: Vec<[f32; 2]>,
    pub checkpoint_spacing: f32,
    pub sprint_finish: [f32; 2],
}

/// A node with the defaults applied.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub at: glam::Vec2,
    pub radius: f32,
    pub width: f32,
    pub apron: [f32; 2],
    pub elevation: f32,
    pub bank: f32,
    pub curbs: bool,
}

impl TrackFile {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn nodes(&self) -> Vec<Node> {
        let d = &self.defaults;
        self.nodes
            .iter()
            .map(|n| Node {
                at: n.at.into(),
                radius: n.radius.unwrap_or(d.radius),
                width: n.width.unwrap_or(d.width),
                apron: n.apron.unwrap_or(d.apron),
                elevation: n.elevation.unwrap_or(d.elevation),
                bank: n.bank_deg.unwrap_or(d.bank_deg).to_radians(),
                curbs: n.curbs,
            })
            .collect()
    }
}
