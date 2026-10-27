//! Gates 1 (structure and conventions) and 2 (dimensions).

use glam::Vec3;

use crate::{
    model::{Aabb, Model},
    report::Gate,
    spec::CarSpec,
    thresholds::Thresholds,
};

pub const WHEELS: [&str; 4] = ["fl", "fr", "rl", "rr"];

pub const NODES: [&str; 17] = [
    "body",
    "hood",
    "bumper_front",
    "bumper_rear",
    "door_l",
    "door_r",
    "mirror_l",
    "mirror_r",
    "spoiler",
    "glass",
    "lights_front",
    "lights_rear",
    "interior",
    "wheel_fl",
    "wheel_fr",
    "wheel_rl",
    "wheel_rr",
];

pub const SOCKETS: [&str; 8] = [
    "socket_wheel_fl",
    "socket_wheel_fr",
    "socket_wheel_rl",
    "socket_wheel_rr",
    "socket_exhaust_l",
    "socket_exhaust_r",
    "socket_camera_hood",
    "socket_camera_cockpit",
];

pub const MATERIALS: [&str; 9] = [
    "car_paint",
    "glass",
    "rubber",
    "rim",
    "chrome",
    "light_emissive_front",
    "light_emissive_rear",
    "trim_black",
    "interior",
];

fn m(v: f32) -> String {
    format!("{v:.3} m")
}

fn v3(v: Vec3) -> String {
    format!("({:.3}, {:.3}, {:.3})", v.x, v.y, v.z)
}

/// LOD0 renderable geometry bounds (sockets have no geometry).
pub fn lod0_bounds(model: &Model) -> Aabb {
    model.bounds(model.render_nodes(0))
}

/// Hub centres of the four wheels (node origins), if all are present.
pub fn hubs(model: &Model) -> Option<[Vec3; 4]> {
    let mut out = [Vec3::ZERO; 4];
    for (o, w) in out.iter_mut().zip(WHEELS) {
        *o = model.nodes[model.find(&format!("wheel_{w}"))?].translation();
    }
    Some(out)
}

/// Names that look like a Blender duplicate of `name` (`body.001`).
fn blender_duplicates<'a>(model: &'a Model, name: &str) -> Vec<&'a str> {
    model
        .nodes
        .iter()
        .filter(|n| {
            n.base.strip_prefix(name).is_some_and(|rest| {
                rest.len() > 1
                    && rest.starts_with('.')
                    && rest[1..].bytes().all(|b| b.is_ascii_digit())
            })
        })
        .map(|n| n.name.as_str())
        .collect()
}

fn names_check(model: &Model, required: &[&str], lod0_only: bool) -> (Vec<String>, Vec<String>) {
    let mut missing = Vec::new();
    let mut duplicated = Vec::new();
    for &name in required {
        let count = model
            .nodes
            .iter()
            .filter(|n| n.base == name && !n.collision && (!lod0_only || n.lod == 0))
            .count();
        if count == 0 {
            let dups = blender_duplicates(model, name);
            missing.push(if dups.is_empty() {
                name.to_string()
            } else {
                format!("{name} (found {})", dups.join(", "))
            });
        } else if count > 1 {
            duplicated.push(format!("{name} ×{count}"));
        }
    }
    (missing, duplicated)
}

pub fn structure(model: &Model, spec: &CarSpec, t: &Thresholds) -> Gate {
    let mut g = Gate::new(1, "Structure and conventions");

    let (missing, dup) = names_check(model, &NODES, true);
    g.check(
        "nodes",
        "required LOD0 nodes present once",
        if missing.is_empty() && dup.is_empty() {
            format!("all {} present", NODES.len())
        } else {
            format!("missing: [{}] duplicated: [{}]", missing.join(", "), dup.join(", "))
        },
        "all present, unique",
        missing.is_empty() && dup.is_empty(),
    );

    let (missing, dup) = names_check(model, &SOCKETS, false);
    g.check(
        "sockets",
        "required socket empties present once",
        if missing.is_empty() && dup.is_empty() {
            format!("all {} present", SOCKETS.len())
        } else {
            format!("missing: [{}] duplicated: [{}]", missing.join(", "), dup.join(", "))
        },
        "all present, unique",
        missing.is_empty() && dup.is_empty(),
    );
    let lights: Vec<_> = model
        .nodes
        .iter()
        .filter(|n| n.base.starts_with("socket_light_"))
        .collect();
    let front = lights.iter().filter(|n| n.translation().z < 0.0).count();
    let rear = lights.iter().filter(|n| n.translation().z > 0.0).count();
    g.check(
        "light_sockets",
        "socket_light_* empties (front = z < 0)",
        format!(
            "{} ({front} front, {rear} rear): {}",
            lights.len(),
            lights
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        format!("≥ {}, ≥ 1 front and rear", t.min_light_sockets),
        lights.len() >= t.min_light_sockets && front >= 1 && rear >= 1,
    );

    let missing: Vec<&str> = MATERIALS
        .iter()
        .copied()
        .filter(|m| !model.materials.iter().any(|x| x == m))
        .collect();
    g.check(
        "materials",
        "required materials present",
        if missing.is_empty() {
            "all present".to_string()
        } else {
            format!("missing: {}", missing.join(", "))
        },
        MATERIALS.join(", "),
        missing.is_empty(),
    );
    let extra: Vec<&str> = model
        .materials
        .iter()
        .map(String::as_str)
        .filter(|m| !MATERIALS.contains(m))
        .collect();
    if !extra.is_empty() {
        g.info("materials_extra", "other materials", extra.join(", "));
    }
    let unassigned = model
        .nodes
        .iter()
        .flat_map(|n| &n.prims)
        .filter(|p| p.material.is_none())
        .count();
    g.check(
        "material_assigned",
        "primitives without a material",
        unassigned.to_string(),
        "0",
        unassigned == 0,
    );

    let bounds = lod0_bounds(model);
    let size = bounds.size();
    g.check(
        "units",
        "LOD0 length (Z extent) in metres",
        m(size.z),
        format!("{} – {} m", t.units_length.0, t.units_length.1),
        size.z >= t.units_length.0 && size.z <= t.units_length.1,
    );
    g.check(
        "up_axis",
        "+Y up: height is the smallest extent, length (Z) the largest",
        format!("x {:.3}, y {:.3}, z {:.3} m", size.x, size.y, size.z),
        "y < x < z",
        size.y < size.x && size.x < size.z,
    );
    g.check(
        "ground",
        "lowest LOD0 point on the ground (y = 0)",
        m(bounds.min.y),
        format!("|y| ≤ {}", t.ground),
        bounds.min.y.abs() <= t.ground,
    );

    let mirrored: Vec<&str> = model
        .nodes
        .iter()
        .filter(|n| n.has_mesh() && n.world.determinant() < 0.0)
        .map(|n| n.name.as_str())
        .collect();
    g.check(
        "mirrored",
        "mesh nodes with a mirroring (negative determinant) transform",
        if mirrored.is_empty() {
            "none".to_string()
        } else {
            mirrored.join(", ")
        },
        "none",
        mirrored.is_empty(),
    );

    if let Some(h) = hubs(model) {
        let front_z = (h[0].z + h[1].z) / 2.0;
        let rear_z = (h[2].z + h[3].z) / 2.0;
        g.check(
            "facing",
            "front wheels at −Z, left wheels at −X",
            format!(
                "front axle z {front_z:.3}, rear {rear_z:.3}; left x {:.3}/{:.3}, right {:.3}/{:.3}",
                h[0].x, h[2].x, h[1].x, h[3].x
            ),
            "front z < rear z, left x < 0 < right x",
            front_z < rear_z && h[0].x < 0.0 && h[2].x < 0.0 && h[1].x > 0.0 && h[3].x > 0.0,
        );
        let mid = (front_z + rear_z) / 2.0;
        let cx = h.iter().map(|v| v.x).sum::<f32>() / 4.0;
        g.check(
            "origin",
            "origin at the wheelbase midpoint on the centreline",
            format!("axle midpoint z {mid:.3}, wheel centre x {cx:.3}"),
            format!("|z|, |x| ≤ {}", t.origin),
            mid.abs() <= t.origin && cx.abs() <= t.origin,
        );
        let mut worst = (0.0f32, "");
        for w in WHEELS {
            let i = model.find(&format!("wheel_{w}")).unwrap();
            let d = model.subtree_bounds(i).centre().distance(h_of(&h, w));
            if d >= worst.0 {
                worst = (d, w);
            }
        }
        g.check(
            "hub_origin",
            "wheel node origin at the centre of its geometry",
            format!("worst wheel_{}: {:.4} m", worst.1, worst.0),
            format!("≤ {}", t.hub_origin),
            worst.0 <= t.hub_origin,
        );
        let mut detail = Vec::new();
        let mut ok = true;
        for w in WHEELS {
            let expected = Vec3::from(spec.hub(w));
            let d = h_of(&h, w) - expected;
            ok &= d.abs().max_element() <= t.wheel_position;
            detail.push(format!("{w} Δ{}", v3(d)));
        }
        g.check(
            "wheel_positions",
            "wheel hubs vs car.toml wheelbase, tracks and tyre radius",
            detail.join("; "),
            format!("each axis ≤ {}", t.wheel_position),
            ok,
        );
        let mut worst = (0.0f32, String::new());
        let mut missing = false;
        for w in WHEELS {
            match model
                .nodes
                .iter()
                .find(|n| n.base == format!("socket_wheel_{w}"))
            {
                Some(s) => {
                    let d = s.translation().distance(h_of(&h, w));
                    if d >= worst.0 {
                        worst = (d, w.to_string());
                    }
                }
                None => missing = true,
            }
        }
        g.check(
            "socket_wheels",
            "socket_wheel_* at the wheel hubs",
            if missing {
                "socket missing".to_string()
            } else {
                format!("worst {}: {:.4} m", worst.1, worst.0)
            },
            format!("≤ {}", t.socket_wheel),
            !missing && worst.0 <= t.socket_wheel,
        );
    } else {
        g.check(
            "facing",
            "front wheels at −Z, left wheels at −X",
            "wheel nodes missing",
            "4 wheels",
            false,
        );
        g.check(
            "origin",
            "origin at the wheelbase midpoint on the centreline",
            "wheel nodes missing",
            "4 wheels",
            false,
        );
    }

    // Parts that must sit on a particular side of the car.
    let mut wrong = Vec::new();
    let mut side = |name: &str, want: &str, ok: fn(Vec3) -> bool, at: Option<Vec3>| {
        if let Some(p) = at
            && !ok(p)
        {
            wrong.push(format!("{name} at {} (expected {want})", v3(p)));
        }
    };
    let centre = |name: &str| {
        model
            .find(name)
            .map(|i| model.subtree_bounds(i))
            .filter(|b| !b.is_empty())
            .map(|b| b.centre())
    };
    let socket = |name: &str| {
        model
            .nodes
            .iter()
            .find(|n| n.base == name)
            .map(|n| n.translation())
    };
    side("mirror_l", "x < 0", |p| p.x < 0.0, centre("mirror_l"));
    side("mirror_r", "x > 0", |p| p.x > 0.0, centre("mirror_r"));
    side("door_l", "x < 0", |p| p.x < 0.0, centre("door_l"));
    side("door_r", "x > 0", |p| p.x > 0.0, centre("door_r"));
    side("bumper_front", "z < 0", |p| p.z < 0.0, centre("bumper_front"));
    side("bumper_rear", "z > 0", |p| p.z > 0.0, centre("bumper_rear"));
    side("lights_front", "z < 0", |p| p.z < 0.0, centre("lights_front"));
    side("lights_rear", "z > 0", |p| p.z > 0.0, centre("lights_rear"));
    side("socket_exhaust_l", "x < 0, z > 0", |p| p.x < 0.0 && p.z > 0.0, socket("socket_exhaust_l"));
    side("socket_exhaust_r", "x > 0, z > 0", |p| p.x > 0.0 && p.z > 0.0, socket("socket_exhaust_r"));
    for cam in ["socket_camera_hood", "socket_camera_cockpit"] {
        if let Some(p) = socket(cam)
            && !(p.cmpge(bounds.min).all() && p.cmple(bounds.max).all())
        {
            wrong.push(format!("{cam} at {} outside the car's bounds", v3(p)));
        }
    }
    g.check(
        "placement",
        "left/right and front/rear parts and sockets on the right side",
        if wrong.is_empty() {
            "all consistent".to_string()
        } else {
            wrong.join("; ")
        },
        "consistent with +Y up, −Z forward, +X right",
        wrong.is_empty(),
    );
    g
}

fn h_of(h: &[Vec3; 4], w: &str) -> Vec3 {
    h[WHEELS.iter().position(|x| *x == w).unwrap()]
}

fn rel(actual: f32, expected: f32) -> f32 {
    (actual - expected) / expected
}

pub fn dimensions(model: &Model, spec: &CarSpec, t: &Thresholds) -> Gate {
    let mut g = Gate::new(2, "Dimensions");
    let bounds = lod0_bounds(model);
    if bounds.is_empty() {
        g.check("geometry", "LOD0 geometry", "none", "present", false);
        return g;
    }
    let mut dim = |id: &str, what: &str, actual: f32, expected: f32| {
        let r = rel(actual, expected);
        g.check(
            id,
            what,
            format!("{actual:.3} m ({:+.2} %)", r * 100.0),
            format!("{expected:.3} m ± {:.1} %", t.dimension_rel * 100.0),
            r.abs() <= t.dimension_rel,
        );
    };
    let size = bounds.size();
    dim("length", "overall length (Z extent)", size.z, spec.length);
    // Published width excludes mirrors.
    let mirrors: Vec<usize> = ["mirror_l", "mirror_r"]
        .iter()
        .filter_map(|n| model.find(n))
        .flat_map(|i| model.subtree(i))
        .collect();
    let no_mirrors = model.bounds(model.render_nodes(0).filter(|i| !mirrors.contains(i)));
    dim("width", "width without mirrors (X extent)", no_mirrors.size().x, spec.width);
    dim("height", "height (ground to roof)", bounds.max.y.max(0.0) - bounds.min.y.min(0.0), spec.height);
    g.info("width_mirrors", "width with mirrors", m(size.x));

    let Some(h) = hubs(model) else {
        g.check("wheelbase", "wheelbase", "wheel nodes missing", "4 wheels", false);
        return g;
    };
    let mut axle = |id: &str, what: &str, actual: f32, expected: f32| {
        g.check(
            id,
            what,
            format!("{actual:.3} m ({:+.3})", actual - expected),
            format!("{expected:.3} ± {} m", t.axle_abs),
            (actual - expected).abs() <= t.axle_abs,
        );
    };
    let wheelbase = (h[2].z + h[3].z) / 2.0 - (h[0].z + h[1].z) / 2.0;
    axle("wheelbase", "wheelbase (hub Z distance)", wheelbase, spec.wheelbase);
    axle("track_front", "front track (hub X distance)", h[1].x - h[0].x, spec.track_front);
    axle("track_rear", "rear track (hub X distance)", h[3].x - h[2].x, spec.track_rear);

    for w in WHEELS {
        let i = model.find(&format!("wheel_{w}")).unwrap();
        let b = model.subtree_bounds(i);
        let tyre = spec.tyre(w);
        // Radius from the vertical and longitudinal extents (a wheel is round in
        // the side view); the smaller one is robust to protruding details.
        let radius = b.size().y.min(b.size().z) / 2.0;
        let r = rel(radius, tyre.radius);
        g.check(
            &format!("radius_{w}"),
            format!("wheel_{w} radius"),
            format!("{radius:.4} m ({:+.2} %)", r * 100.0),
            format!("{:.4} m ± {:.0} %", tyre.radius, t.wheel_radius_rel * 100.0),
            r.abs() <= t.wheel_radius_rel,
        );
        let r = rel(b.size().x, tyre.width);
        g.check(
            &format!("tyre_width_{w}"),
            format!("wheel_{w} width (X extent)"),
            format!("{:.3} m ({:+.1} %)", b.size().x, r * 100.0),
            format!("{:.3} m ± {:.0} %", tyre.width, t.tyre_width_rel * 100.0),
            r.abs() <= t.tyre_width_rel,
        );
    }
    g
}
