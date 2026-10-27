//! Track geometry from the node data: the centreline (straights joined by
//! circular fillets at the nodes) sampled every metre with its 3D frame,
//! the road, curb, apron and wall meshes, start grid, checkpoints and
//! greybox buildings.
//!
//! Frames follow Bevy: +Y up, -Z north. A sample's `right` lies in the road
//! surface (tilted by the banking), `up` is the road normal.

use glam::{Quat, Vec2, Vec3};

use crate::data::{Node, TrackFile};

/// Distance between centreline samples (m).
pub const SAMPLE_SPACING: f32 = 1.0;
/// Wall thickness (m).
pub const WALL_THICKNESS: f32 = 0.4;
/// Top of the flat ground the city stands on.
pub const GROUND_Y: f32 = -0.05;
/// Height of a curb's outer edge above the road.
const CURB_RISE: f32 = 0.05;
/// Curbs start and end this far before and after a node's arc (m).
const CURB_OVERRUN: f32 = 4.0;

/// Triangle mesh, counter-clockwise seen from the side its normal faces.
#[derive(Clone, Debug, Default)]
pub struct TriMesh {
    pub vertices: Vec<Vec3>,
    pub indices: Vec<[u32; 3]>,
}

impl TriMesh {
    /// Adds a ribbon between two point rows: quads (a_k, b_k, a_k+1, b_k+1)
    /// for every `k` with `keep(k)`, wrapping from the last row to the first.
    /// The face normal is (b - a) x (a_next - a).
    fn ribbon(&mut self, a: &[Vec3], b: &[Vec3], keep: impl Fn(usize) -> bool) {
        let n = a.len();
        for k in 0..n {
            let j = (k + 1) % n;
            if !keep(k) {
                continue;
            }
            let base = self.vertices.len() as u32;
            self.vertices.extend([a[k], b[k], a[j], b[j]]);
            self.indices
                .extend([[base, base + 1, base + 2], [base + 1, base + 3, base + 2]]);
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len()
    }
}

/// One centreline sample.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// Distance from the start line along the centreline (m).
    pub s: f32,
    /// Centreline point on the road surface.
    pub pos: Vec3,
    pub tangent: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    /// Plan curvature (1/m), positive turning right.
    pub curvature: f32,
    pub half_width: f32,
    /// Apron width left and right of the road (m).
    pub apron: [f32; 2],
    pub curbs: bool,
}

impl Sample {
    /// Lateral distance from the centreline to the wall face on `side`
    /// (0 left, 1 right).
    pub fn wall(&self, side: usize) -> f32 {
        self.half_width + self.apron[side]
    }

    /// Point at lateral offset `x` (positive right) and height `y` above the road.
    pub fn at(&self, x: f32, y: f32) -> Vec3 {
        self.pos + self.right * x + self.up * y
    }

    /// Rotation that maps the car frame (-Z forward, +Y up) onto this sample.
    pub fn rotation(&self) -> Quat {
        let back = -self.tangent;
        let up = self.up;
        let right = up.cross(back).normalize();
        Quat::from_mat3(&glam::Mat3::from_cols(right, up, right.cross(up)))
    }
}

/// A pose on the track: position on the road surface and heading.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub position: Vec3,
    pub rotation: Quat,
}

/// A greybox building beside the track (visual only).
#[derive(Clone, Copy, Debug)]
pub struct Building {
    /// Centre of the footprint at ground level.
    pub base: Vec3,
    /// Rotation about +Y.
    pub yaw: f32,
    /// Full size (x across, y height, z along).
    pub size: Vec3,
}

/// Everything generated from a track file.
#[derive(Clone, Debug)]
pub struct Geometry {
    pub id: String,
    pub name: String,
    /// Closed centreline length (m).
    pub length: f32,
    /// Samples every [`SAMPLE_SPACING`] (adjusted to close the loop), starting
    /// at the start line.
    pub samples: Vec<Sample>,
    pub road: TriMesh,
    /// Curbs in two alternating colours (red, white).
    pub curbs: [TriMesh; 2],
    pub aprons: TriMesh,
    pub walls: TriMesh,
    /// Painted start line and grid boxes (visual only).
    pub markings: TriMesh,
    pub buildings: Vec<Building>,
    /// Grid slots, pole first.
    pub grid: Vec<Pose>,
    /// Checkpoint distances in order, after the start line; the lap ends at
    /// the line itself (`length`).
    pub checkpoints: Vec<f32>,
    /// Index into `checkpoints` of the last checkpoint of each sector but the
    /// last (which ends at the line).
    pub sector_ends: Vec<usize>,
    /// Finish distance for sprints.
    pub sprint_finish: f32,
    /// Plan bounds (min, max) of everything, for the ground.
    pub bounds: (Vec2, Vec2),
}

/// A route element in plan.
#[derive(Clone, Copy, Debug)]
struct Fillet {
    /// Tangent length from the node to where the arc starts and ends.
    tangent: f32,
    /// Signed turn angle (positive right).
    turn: f32,
    start: Vec2,
    dir_in: Vec2,
    radius: f32,
}

fn right_of(d: Vec2) -> Vec2 {
    Vec2::new(-d.y, d.x)
}

fn fillets(nodes: &[Node]) -> Vec<Fillet> {
    let n = nodes.len();
    (0..n)
        .map(|i| {
            let (prev, here, next) = (
                nodes[(i + n - 1) % n].at,
                nodes[i].at,
                nodes[(i + 1) % n].at,
            );
            let (din, dout) = ((here - prev).normalize(), (next - here).normalize());
            let turn = din.perp_dot(dout).atan2(din.dot(dout));
            let radius = nodes[i].radius;
            let tangent = radius * (turn.abs() / 2.0).tan();
            Fillet {
                tangent,
                turn,
                start: here - din * tangent,
                dir_in: din,
                radius,
            }
        })
        .collect()
}

/// Plan position, heading and curvature at route distance `d` from the
/// start of node 0's arc, with the cumulative distance where each arc starts.
struct Route {
    nodes: Vec<Node>,
    fillets: Vec<Fillet>,
    /// Route distance where each node's arc starts, and the arc length.
    arc_at: Vec<(f32, f32)>,
    length: f32,
}

impl Route {
    fn new(nodes: Vec<Node>) -> Self {
        let fillets = fillets(&nodes);
        let n = nodes.len();
        let mut arc_at = Vec::with_capacity(n);
        let mut d = 0.0;
        for i in 0..n {
            let arc = fillets[i].radius * fillets[i].turn.abs();
            arc_at.push((d, arc));
            let j = (i + 1) % n;
            let straight =
                nodes[i].at.distance(nodes[j].at) - fillets[i].tangent - fillets[j].tangent;
            d += arc + straight.max(0.0);
        }
        Route {
            nodes,
            fillets,
            arc_at,
            length: d,
        }
    }

    /// (position, direction, curvature) at route distance `d`.
    fn at(&self, d: f32) -> (Vec2, Vec2, f32) {
        let d = d.rem_euclid(self.length);
        let i = self.arc_at.partition_point(|&(start, _)| start <= d) - 1;
        let (start, arc) = self.arc_at[i];
        let f = &self.fillets[i];
        let along = d - start;
        if along < arc && f.turn.abs() > 1e-6 {
            let phi = along / f.radius * f.turn.signum();
            let center = f.start + right_of(f.dir_in) * f.radius * f.turn.signum();
            let rot = Vec2::from_angle(phi);
            let pos = center + rot.rotate(f.start - center);
            (pos, rot.rotate(f.dir_in), f.turn.signum() / f.radius)
        } else {
            let dir = (f.dir_in.rotate(Vec2::from_angle(f.turn))).normalize();
            let arc_end = f.start + {
                let center_off = right_of(f.dir_in) * f.radius * f.turn.signum();
                Vec2::from_angle(f.turn).rotate(-center_off) + center_off
            };
            (arc_end + dir * (along - arc), dir, 0.0)
        }
    }

    /// Route distance of each node (the middle of its arc).
    fn node_distance(&self, i: usize) -> f32 {
        self.arc_at[i].0 + self.arc_at[i].1 / 2.0
    }

    /// Node attributes blended with smoothstep between the nodes around `d`.
    fn blend<T>(&self, d: f32, get: impl Fn(&Node) -> T, mix: impl Fn(T, T, f32) -> T) -> T {
        let n = self.nodes.len();
        let d = d.rem_euclid(self.length);
        // Node i owns [node_distance(i), node_distance(i + 1)).
        let mut i = n - 1;
        for k in 0..n {
            if self.node_distance(k) <= d {
                i = k;
            }
        }
        let j = (i + 1) % n;
        let (a, b) = (self.node_distance(i), self.node_distance(j));
        let span = (b - a).rem_euclid(self.length).max(1e-3);
        let t = ((d - a).rem_euclid(self.length) / span).clamp(0.0, 1.0);
        let w = t * t * (3.0 - 2.0 * t);
        mix(get(&self.nodes[i]), get(&self.nodes[j]), w)
    }

    fn curbs(&self, d: f32) -> bool {
        self.nodes.iter().enumerate().any(|(i, node)| {
            let (start, arc) = self.arc_at[i];
            let from = (d - start + CURB_OVERRUN).rem_euclid(self.length);
            node.curbs && arc > 0.0 && from <= arc + 2.0 * CURB_OVERRUN
        })
    }

    /// Nearest route distance to a plan point (brute force at 0.5 m).
    fn project(&self, p: Vec2) -> (f32, f32) {
        let steps = (self.length / 0.5) as usize;
        (0..steps)
            .map(|k| {
                let d = k as f32 * 0.5;
                (d, self.at(d).0.distance(p))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((0.0, f32::MAX))
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Builds the geometry. Call [`validate`] first: this assumes valid data.
pub fn build(file: &TrackFile) -> Geometry {
    let route = Route::new(file.nodes());
    let (start, _) = route.project(file.race.start.into());
    let n = (route.length / SAMPLE_SPACING).round() as usize;
    let ds = route.length / n as f32;
    let plan: Vec<(Vec2, Vec2, f32, f32)> = (0..n)
        .map(|k| {
            let d = start + k as f32 * ds;
            let (p, dir, curvature) = route.at(d);
            (p, dir, curvature, d)
        })
        .collect();
    let mut samples: Vec<Sample> = plan
        .iter()
        .enumerate()
        .map(|(k, &(p, dir, curvature, d))| {
            let elevation = route.blend(d, |n| n.elevation, lerp);
            let bank = route.blend(d, |n| n.bank, lerp);
            let right_plan = right_of(dir);
            let right =
                Vec3::new(right_plan.x, 0.0, right_plan.y) * bank.cos() - Vec3::Y * bank.sin();
            Sample {
                s: k as f32 * ds,
                pos: Vec3::new(p.x, elevation, p.y),
                tangent: Vec3::new(dir.x, 0.0, dir.y),
                right,
                up: Vec3::Y,
                curvature,
                half_width: route.blend(d, |n| n.width, lerp) / 2.0,
                apron: route.blend(
                    d,
                    |n| n.apron,
                    |a, b, t| [lerp(a[0], b[0], t), lerp(a[1], b[1], t)],
                ),
                curbs: route.curbs(d),
            }
        })
        .collect();
    // 3D frames from the elevated centreline.
    for k in 0..n {
        let (prev, next) = (samples[(k + n - 1) % n].pos, samples[(k + 1) % n].pos);
        let s = &mut samples[k];
        s.tangent = (next - prev).normalize();
        s.right = (s.right - s.tangent * s.tangent.dot(s.right)).normalize();
        s.up = s.right.cross(s.tangent).normalize();
    }

    let mut road = TriMesh::default();
    let row = |f: &dyn Fn(&Sample) -> Vec3| samples.iter().map(f).collect::<Vec<_>>();
    road.ribbon(
        &row(&|s| s.at(-s.half_width, 0.0)),
        &row(&|s| s.at(s.half_width, 0.0)),
        |_| true,
    );

    let mut aprons = TriMesh::default();
    aprons.ribbon(
        &row(&|s| s.at(-s.wall(0), 0.0)),
        &row(&|s| s.at(-s.half_width, 0.0)),
        |_| true,
    );
    aprons.ribbon(
        &row(&|s| s.at(s.half_width, 0.0)),
        &row(&|s| s.at(s.wall(1), 0.0)),
        |_| true,
    );

    // Curbs lie on the apron, rising towards their outer edge; colour
    // alternates every 3 m.
    let cw = file.defaults.curb_width;
    let stripe = |k: usize| ((samples[k].s / 3.0) as usize) % 2;
    let curb_on = |k: usize, colour: usize| {
        samples[k].curbs && samples[(k + 1) % n].curbs && stripe(k) == colour
    };
    let mut curbs = [TriMesh::default(), TriMesh::default()];
    for (colour, mesh) in curbs.iter_mut().enumerate() {
        mesh.ribbon(
            &row(&|s| s.at(-s.half_width - cw, CURB_RISE)),
            &row(&|s| s.at(-s.half_width, 0.01)),
            |k| curb_on(k, colour),
        );
        mesh.ribbon(
            &row(&|s| s.at(s.half_width, 0.01)),
            &row(&|s| s.at(s.half_width + cw, CURB_RISE)),
            |k| curb_on(k, colour),
        );
    }

    // Walls: inner face from below the road to the wall height, a flat top,
    // and an outer face down to the ground.
    let h = file.defaults.wall_height;
    let mut walls = TriMesh::default();
    for side in [0usize, 1] {
        let sign = if side == 0 { -1.0 } else { 1.0 };
        let inner = |s: &Sample| sign * s.wall(side);
        let outer = |s: &Sample| sign * (s.wall(side) + WALL_THICKNESS);
        let inner_bottom = row(&|s| s.at(inner(s), -0.3));
        let inner_top = row(&|s| s.at(inner(s), h));
        let outer_top = row(&|s| s.at(outer(s), h));
        let outer_bottom = row(&|s| s.at(outer(s), 0.0).with_y(GROUND_Y - 0.5));
        if side == 0 {
            walls.ribbon(&inner_top, &inner_bottom, |_| true);
            walls.ribbon(&outer_top, &inner_top, |_| true);
            walls.ribbon(&outer_bottom, &outer_top, |_| true);
        } else {
            walls.ribbon(&inner_bottom, &inner_top, |_| true);
            walls.ribbon(&inner_top, &outer_top, |_| true);
            walls.ribbon(&outer_top, &outer_bottom, |_| true);
        }
    }

    let length = route.length;
    let pose_at = |s: f32, lateral: f32| {
        let sample = interpolate(&samples, length, s);
        Pose {
            position: sample.at(lateral, 0.0),
            rotation: sample.rotation(),
        }
    };
    let race = &file.race;
    let grid: Vec<Pose> = (0..race.grid_slots)
        .map(|i| {
            let back = 4.0 + i as f32 * race.grid_row_spacing;
            let side = if i % 2 == 0 { 1.0 } else { -1.0 };
            pose_at(length - back, side * race.grid_column_offset)
        })
        .collect();

    let mut markings = TriMesh::default();
    let mut paint_box = |pose: &Pose, half: Vec2| {
        let corner = |x: f32, z: f32| pose.position + pose.rotation * Vec3::new(x, 0.015, z);
        let base = markings.vertices.len() as u32;
        markings.vertices.extend([
            corner(-half.x, half.y),
            corner(half.x, half.y),
            corner(-half.x, -half.y),
            corner(half.x, -half.y),
        ]);
        markings
            .indices
            .extend([[base, base + 1, base + 2], [base + 1, base + 3, base + 2]]);
    };
    let line_width = samples[0].half_width;
    paint_box(&pose_at(0.0, 0.0), Vec2::new(line_width, 0.4));
    for slot in &grid {
        // A bar across the front of each slot.
        let front = Pose {
            position: slot.position + slot.rotation * Vec3::new(0.0, 0.0, -2.6),
            rotation: slot.rotation,
        };
        paint_box(&front, Vec2::new(1.2, 0.15));
    }

    // Checkpoints every `checkpoint_spacing` plus the sector ends.
    let to_race = |p: [f32; 2]| (route.project(p.into()).0 - start).rem_euclid(length);
    let sector_points: Vec<f32> = race.sectors.iter().map(|&p| to_race(p)).collect();
    let mut checkpoints: Vec<f32> = sector_points.clone();
    let spacing = race.checkpoint_spacing;
    let mut s = spacing;
    while s < length - spacing / 2.0 {
        if sector_points.iter().all(|p| (p - s).abs() > spacing / 3.0) {
            checkpoints.push(s);
        }
        s += spacing;
    }
    checkpoints.sort_by(f32::total_cmp);
    let sector_ends = sector_points
        .iter()
        .map(|p| checkpoints.iter().position(|c| c == p).unwrap_or(0))
        .collect();

    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for s in &samples {
        let p = Vec2::new(s.pos.x, s.pos.z);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    let buildings = buildings(&samples);
    Geometry {
        id: file.id.clone(),
        name: file.name.clone(),
        length,
        road,
        curbs,
        aprons,
        walls,
        markings,
        buildings,
        grid,
        checkpoints,
        sector_ends,
        sprint_finish: to_race(race.sprint_finish),
        bounds: (lo, hi),
        samples,
    }
}

/// Sample at distance `s` (wrapped), interpolated between neighbours.
pub fn interpolate(samples: &[Sample], length: f32, s: f32) -> Sample {
    let n = samples.len();
    let s = s.rem_euclid(length);
    let x = s / length * n as f32;
    let k = (x as usize).min(n - 1);
    let t = x - k as f32;
    let (a, b) = (&samples[k], &samples[(k + 1) % n]);
    Sample {
        s,
        pos: a.pos.lerp(b.pos, t),
        tangent: a.tangent.lerp(b.tangent, t).normalize(),
        right: a.right.lerp(b.right, t).normalize(),
        up: a.up.lerp(b.up, t).normalize(),
        curvature: lerp(a.curvature, b.curvature, t),
        half_width: lerp(a.half_width, b.half_width, t),
        apron: [
            lerp(a.apron[0], b.apron[0], t),
            lerp(a.apron[1], b.apron[1], t),
        ],
        curbs: a.curbs,
    }
}

/// Greybox blocks set back behind the walls, deterministic per track.
fn buildings(samples: &[Sample]) -> Vec<Building> {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 24) as f32
    };
    // Every corner, edge midpoint and the centre of a footprint must stay
    // clear of the walls (plus a 2 m pavement) of every part of the route.
    let clear_of_road = |points: &[Vec2]| {
        samples.iter().all(|s| {
            let reach = s.half_width + s.apron[0].max(s.apron[1]) + WALL_THICKNESS + 2.0;
            let c = Vec2::new(s.pos.x, s.pos.z);
            points.iter().all(|p| c.distance(*p) > reach)
        })
    };
    let mut out: Vec<Building> = Vec::new();
    let step = (26.0 / SAMPLE_SPACING) as usize;
    for side in [0usize, 1] {
        let sign = if side == 0 { -1.0 } else { 1.0 };
        for s in samples.iter().step_by(step) {
            let size = Vec3::new(
                14.0 + 12.0 * rand(),
                10.0 + 40.0 * rand().powi(2),
                18.0 + 6.0 * rand(),
            );
            let offset = s.wall(side) + WALL_THICKNESS + 3.0 + size.x / 2.0;
            let across = Vec2::new(s.right.x, s.right.z).normalize();
            let along = Vec2::new(s.tangent.x, s.tangent.z).normalize();
            let center = Vec2::new(s.pos.x, s.pos.z) + across * sign * offset;
            let footprint: Vec<Vec2> = [-1.0, 0.0, 1.0]
                .iter()
                .flat_map(|&u| [-1.0, 0.0, 1.0].map(|w| (u, w)))
                .map(|(u, w)| center + across * u * size.x / 2.0 + along * w * size.z / 2.0)
                .collect();
            let overlaps = out.iter().any(|b| {
                let d = Vec2::new(b.base.x, b.base.z).distance(center);
                d < 0.5 * (size.x.min(size.z) + b.size.x.min(b.size.z))
            });
            if overlaps || !clear_of_road(&footprint) {
                continue;
            }
            out.push(Building {
                base: Vec3::new(center.x, GROUND_Y, center.y),
                yaw: (-s.tangent.x).atan2(-s.tangent.z),
                size,
            });
        }
    }
    out
}

/// Checks the data and the geometry built from it; returns every problem found.
pub fn validate(file: &TrackFile) -> Vec<String> {
    let mut errors = Vec::new();
    let nodes = file.nodes();
    let d = &file.defaults;
    if nodes.len() < 3 {
        errors.push(format!("need at least 3 nodes, got {}", nodes.len()));
        return errors;
    }
    if !(0.5..=3.0).contains(&d.wall_height) {
        errors.push(format!("wall_height {} outside 0.5..=3 m", d.wall_height));
    }
    let n = nodes.len();
    for (i, node) in nodes.iter().enumerate() {
        let name = format!("node {i} at {:?}", node.at.to_array());
        let values = [
            node.radius,
            node.width,
            node.apron[0],
            node.apron[1],
            node.elevation,
            node.bank,
        ];
        if !values.iter().chain(node.at.as_ref()).all(|v| v.is_finite()) {
            errors.push(format!("{name}: non-finite value"));
            continue;
        }
        if !(6.0..=25.0).contains(&node.width) {
            errors.push(format!("{name}: width {} outside 6..=25 m", node.width));
        }
        if node.apron.iter().any(|a| !(0.0..=30.0).contains(a)) {
            errors.push(format!("{name}: apron {:?} outside 0..=30 m", node.apron));
        }
        if node.curbs && node.apron.iter().any(|&a| a < d.curb_width + 0.3) {
            errors.push(format!(
                "{name}: curbs need aprons of at least curb_width + 0.3 m"
            ));
        }
        if node.radius <= 0.0 {
            errors.push(format!("{name}: radius must be positive"));
        }
        if node.bank.abs() > 10f32.to_radians() {
            errors.push(format!("{name}: banking over 10 degrees"));
        }
        if node.at.distance(nodes[(i + 1) % n].at) < 1.0 {
            errors.push(format!("{name}: coincides with the next node"));
        }
    }
    if !errors.is_empty() {
        return errors;
    }
    let f = fillets(&nodes);
    for i in 0..n {
        let j = (i + 1) % n;
        let room = nodes[i].at.distance(nodes[j].at);
        if f[i].tangent + f[j].tangent > room + 1e-3 {
            errors.push(format!(
                "nodes {i} and {j}: corner radii need {:.1} m of the {room:.1} m between them",
                f[i].tangent + f[j].tangent
            ));
        }
        if f[i].turn.abs() > 179f32.to_radians() {
            errors.push(format!("node {i}: turns back on itself"));
        }
    }
    if !errors.is_empty() {
        return errors;
    }

    let g = build(file);
    let samples = &g.samples;
    let m = samples.len();
    for (k, s) in samples.iter().enumerate() {
        let next = &samples[(k + 1) % m];
        let run = Vec2::new(next.pos.x - s.pos.x, next.pos.z - s.pos.z).length();
        let grade = (next.pos.y - s.pos.y).abs() / run.max(1e-3);
        if grade > 0.12 {
            errors.push(format!(
                "s = {:.0} m: grade {:.1} % over 12 %",
                s.s,
                grade * 100.0
            ));
            break;
        }
        // The inner wall must not fold over itself in a corner.
        if s.curvature != 0.0 {
            let inner = if s.curvature > 0.0 { 1 } else { 0 };
            if s.wall(inner) + WALL_THICKNESS + 0.5 > 1.0 / s.curvature.abs() {
                errors.push(format!(
                    "s = {:.0} m: corner radius {:.1} m is too tight for the road and apron ({:.1} m to the inner wall)",
                    s.s,
                    1.0 / s.curvature.abs(),
                    s.wall(inner)
                ));
                break;
            }
        }
    }
    // Parts of the route far apart along the road must not overlap in plan.
    let reach = |s: &Sample| s.half_width + s.apron[0].max(s.apron[1]) + WALL_THICKNESS;
    'outer: for a in 0..m {
        for b in a + 1..m {
            let along = (samples[b].s - samples[a].s).min(g.length - (samples[b].s - samples[a].s));
            if along < 60.0 {
                continue;
            }
            let (pa, pb) = (samples[a].pos, samples[b].pos);
            let gap = Vec2::new(pa.x - pb.x, pa.z - pb.z).length();
            if gap < reach(&samples[a]) + reach(&samples[b]) + 1.0 {
                errors.push(format!(
                    "route overlaps itself near s = {:.0} m and s = {:.0} m ({gap:.1} m apart)",
                    samples[a].s, samples[b].s
                ));
                break 'outer;
            }
        }
    }
    // Grid slots on a straight, on the road.
    let race = &file.race;
    if race.grid_slots == 0 || race.grid_slots > 16 {
        errors.push(format!("grid_slots {} outside 1..=16", race.grid_slots));
    }
    let grid_length = 4.0 + race.grid_slots as f32 * race.grid_row_spacing + 3.0;
    if samples
        .iter()
        .rev()
        .take(grid_length as usize)
        .chain(samples.iter().take(5))
        .any(|s| s.curvature != 0.0)
    {
        errors.push(format!(
            "the {grid_length:.0} m behind the start line must be straight for the grid"
        ));
    }
    if race.grid_column_offset + 1.2 > samples[0].half_width {
        errors.push("grid columns do not fit on the road".into());
    }
    if race.checkpoint_spacing < 20.0 {
        errors.push("checkpoint_spacing must be at least 20 m".into());
    }
    let route = Route::new(nodes);
    for (what, p) in race.sectors.iter().map(|p| ("sector end", p)).chain([
        ("start", &race.start),
        ("sprint finish", &race.sprint_finish),
    ]) {
        let (_, off) = route.project((*p).into());
        if off > 3.0 {
            errors.push(format!("{what} {p:?} is {off:.1} m off the centreline"));
        }
    }
    let mut ends: Vec<f32> = g.sector_ends.iter().map(|&i| g.checkpoints[i]).collect();
    ends.push(g.length);
    if ends.windows(2).any(|w| w[1] - w[0] < 100.0) || ends[0] < 100.0 {
        errors.push(format!(
            "sectors must be at least 100 m long and in order: ends at {ends:?}"
        ));
    }
    if g.sprint_finish < g.length - 200.0 || g.sprint_finish > g.length - grid_length {
        errors.push(format!(
            "sprint finish at {:.0} m should lie just before the grid (lap {:.0} m)",
            g.sprint_finish, g.length
        ));
    }
    errors
}
