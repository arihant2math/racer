//! Showcase street: a ~400 m Downtown street-circuit section (a straight,
//! a 90° right-hander, a second straight) built entirely from the kit:
//! road with markings and decals, side streets, racing furniture, street
//! furniture, generated city blocks, a grandstand plaza and a distant
//! skyline. Used by `racer-capture scene downtown-kit*`.

use bevy::{
    light::atmosphere::ScatteringMedium,
    math::{Affine3A, Vec2},
    pbr::DefaultOpaqueRendererMethod,
    prelude::*,
};
use racer_render::surface::{
    Crossing, SurfaceMaterial, SurfaceParams, SurfacePlugin, Wetness, flags,
};

use crate::{
    batch::{Band, BatchStats, Batcher, Variant, tint},
    building::{Lot, assemble_block, build},
    district::{District, Style},
    kit::{Parts, Slot, Socket, SocketKind, kelvin_to_rgb},
    lighting,
    materials::{KitMaterials, Mat, TextureSource, TimeOfDay},
    mesh::MeshData,
    noise::Rng,
    path::Path,
    props::{facing, racing, street},
    road::{
        DecalRules, Gap, RoadProfile, Side, build_decals, build_road, scatter_decals,
        solid_intervals,
    },
    texture::RoadSignCell,
};

/// Centreline arc length of the start/finish line.
pub const START_LINE: f32 = 72.0;
/// Corner entry and exit (arc of radius 30 m, 90° right).
pub const CORNER_IN: f32 = 230.0;
pub const CORNER_OUT: f32 = 230.0 + 30.0 * std::f32::consts::FRAC_PI_2;
/// Dressed section of the main road.
pub const DRESSED: (f32, f32) = (40.0, 440.0);

/// The main street: 230 m north, 90° right (R 30 m), 190 m east.
pub fn main_path() -> Path {
    Path::new(Vec2::new(0.0, 60.0), 0.0)
        .straight(CORNER_IN)
        .arc(30.0, 90.0)
        .straight(190.0)
}

/// Two-lane side street cross-section (14 m between building lines).
pub fn side_profile() -> RoadProfile {
    RoadProfile {
        lanes_per_side: 1,
        lane_width: 3.3,
        gutter: 0.4,
        sidewalk: 3.0,
        ..RoadProfile::default()
    }
}

/// Side streets: (centre arc length on the main road, side).
pub const SIDE_STREETS: [(f32, Side); 3] = [
    (123.7, Side::Left),
    (175.7, Side::Right),
    (323.7, Side::Left),
];

/// Lights requested by sockets, in world space.
#[derive(Clone, Debug)]
pub struct LightSpec {
    pub pos: Vec3,
    pub dir: Vec3,
    pub kind: SocketKind,
}

/// What the showcase contains, for budgets and the handoff note.
#[derive(Resource, Clone, Debug, Default)]
pub struct ShowcaseStats {
    pub batches: BatchStats,
    pub buildings: usize,
    pub lights: usize,
}

pub struct ShowcasePlugin {
    pub time: TimeOfDay,
    pub wet: bool,
    pub textures: TextureSource,
}

#[derive(Resource, Clone)]
struct Settings {
    time: TimeOfDay,
    wet: bool,
    textures: TextureSource,
}

impl Plugin for ShowcasePlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SurfacePlugin>() {
            app.add_plugins(SurfacePlugin);
        }
        let wetness = if self.wet { 0.9 } else { 0.0 };
        app.insert_resource(Wetness {
            wetness,
            puddles: 0.6,
        })
        .insert_resource(Settings {
            time: self.time,
            wet: self.wet,
            textures: self.textures.clone(),
        })
        .add_systems(Startup, setup)
        .add_systems(Last, report_frames);
        if self.wet {
            // Wet roads: deferred shading so screen-space reflections can
            // mirror lamps, windows and signs in the water film and puddles.
            app.insert_resource(DefaultOpaqueRendererMethod::deferred())
                .add_systems(PostStartup, lighting::enable_reflections);
        }
    }
}

fn setup(
    mut commands: Commands,
    settings: Res<Settings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
    mut surf: ResMut<Assets<SurfaceMaterial>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
) {
    let t0 = std::time::Instant::now();
    let wetness = if settings.wet { 0.9 } else { 0.0 };
    let mut kit = KitMaterials::new(
        settings.time,
        wetness,
        &settings.textures,
        &mut images,
        &mut std_mats,
        &mut surf,
    );
    let t_tex = t0.elapsed();
    lighting::spawn_sky(&mut commands, &mut media, settings.time);
    if settings.time == TimeOfDay::Night {
        lighting::spawn_night_dome(&mut commands, &mut meshes, &mut images, &mut std_mats);
    }
    let district = District::downtown();
    let (batcher, lights, buildings) = build_showcase(&district, &kit, &mut surf);
    let t_gen = t0.elapsed();
    let batches = batcher.spawn(&mut commands, &mut meshes, &mut kit, &mut std_mats, None);
    let lit = settings.time != TimeOfDay::Day;
    let mut n_lights = 0;
    for l in &lights {
        match l.kind {
            SocketKind::SpotLight {
                outer_angle_deg,
                range,
                lumens,
                kelvin,
            } if lit => {
                let c = kelvin_to_rgb(kelvin);
                commands.spawn((
                    SpotLight {
                        color: Color::srgb(c.x, c.y, c.z),
                        intensity: spot_intensity(lumens, outer_angle_deg),
                        range,
                        radius: 0.25,
                        outer_angle: outer_angle_deg.to_radians(),
                        inner_angle: outer_angle_deg.to_radians() * 0.35,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(l.pos).looking_to(l.dir, Vec3::X),
                ));
                n_lights += 1;
            }
            SocketKind::PointLight {
                range,
                lumens,
                kelvin,
            } if lit => {
                let c = kelvin_to_rgb(kelvin);
                commands.spawn((
                    PointLight {
                        color: Color::srgb(c.x, c.y, c.z),
                        intensity: lumens,
                        range,
                        radius: 0.2,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(l.pos),
                ));
                n_lights += 1;
            }
            _ => {}
        }
    }
    info!(
        "downtown-kit: textures {:.2}s, generation {:.2}s; {} mesh entities, {} triangles near (LOD0), {} far, {} buildings, {} lights",
        t_tex.as_secs_f32(),
        (t_gen - t_tex).as_secs_f32(),
        batches.entities,
        batches.triangles_lod0,
        batches.triangles_far,
        buildings,
        n_lights
    );
    commands.insert_resource(ShowcaseStats {
        batches,
        buildings,
        lights: n_lights,
    });
}

/// Logs the mean frame time and the number of visible mesh entities (draws
/// after culling) every 60 frames, once the first 30 frames (pipeline
/// compilation) are over. On lavapipe the frame time includes software
/// rasterisation, so it is an upper bound for the CPU cost.
fn report_frames(
    time: Res<Time<Real>>,
    mut acc: Local<(u32, f32)>,
    visible: Query<&ViewVisibility, With<Mesh3d>>,
) {
    acc.0 += 1;
    if acc.0 <= 30 {
        return;
    }
    acc.1 += time.delta_secs();
    if acc.0 % 60 == 30 {
        let draws = visible.iter().filter(|v| v.get()).count();
        info!(
            "downtown-kit: mean frame {:.0} ms over 60 frames, {draws} visible mesh entities",
            acc.1 / 60.0 * 1000.0
        );
        acc.1 = 0.0;
    }
}

/// Bevy treats a spot light's intensity like a point light's (luminous flux
/// spread over the whole sphere, then masked by the cone). A luminaire puts
/// its flux into the cone, so scale by 4π / Ω (Ω = cone solid angle), with
/// 80 % of the flux inside the cut-off.
pub fn spot_intensity(lumens: f32, outer_angle_deg: f32) -> f32 {
    let omega = std::f32::consts::TAU * (1.0 - outer_angle_deg.to_radians().cos());
    lumens * 0.8 * 2.0 * std::f32::consts::TAU / omega
}

/// Splits `s0..s1` into chunks of about `len` metres.
fn chunks(s0: f32, s1: f32, len: f32) -> Vec<(f32, f32)> {
    let n = ((s1 - s0) / len).ceil().max(1.0) as usize;
    (0..n)
        .map(|k| {
            (
                s0 + (s1 - s0) * k as f32 / n as f32,
                s0 + (s1 - s0) * (k + 1) as f32 / n as f32,
            )
        })
        .collect()
}

/// Transform placing a prop at arc length `s`, offset `u`, height `y`,
/// facing `dir` (prop −Z).
fn place(path: &Path, s: f32, u: f32, y: f32, dir: Vec3) -> Affine3A {
    Affine3A::from_rotation_translation(facing(dir), path.point(s, u, y))
}

fn road_dir(path: &Path, s: f32, side: f32) -> Vec3 {
    let r = path.pose(s).right();
    -Vec3::new(r.x, 0.0, r.y) * side
}

fn fwd(path: &Path, s: f32) -> Vec3 {
    let f = path.pose(s).forward();
    Vec3::new(f.x, 0.0, f.y)
}

fn add_lights(lights: &mut Vec<LightSpec>, sockets: &[Socket], t: Affine3A) {
    for s in sockets {
        if !matches!(s.kind, SocketKind::Emissive) {
            lights.push(LightSpec {
                pos: t.transform_point3(s.pos),
                dir: t.transform_vector3(s.dir).normalize(),
                kind: s.kind,
            });
        }
    }
}

/// Builds everything; returns the batches, the lights and the building count.
pub fn build_showcase(
    district: &District,
    kit: &KitMaterials,
    surf: &mut Assets<SurfaceMaterial>,
) -> (Batcher, Vec<LightSpec>, usize) {
    let mut b = Batcher::new(60.0);
    let mut lights = Vec::new();
    let mut rng = Rng::new(district.seed ^ 0x5eed);
    let main = main_path();
    let p = RoadProfile::default();
    let sp = side_profile();
    let gaps: Vec<Gap> = SIDE_STREETS
        .iter()
        .map(|(c, side)| Gap {
            side: *side,
            s0: c - sp.road_half(),
            s1: c + sp.road_half(),
        })
        .collect();

    // --- Main road -------------------------------------------------------
    let mut params = SurfaceParams::road(p.lanes_per_side, p.lane_width, flags::ALL);
    params.seed = 3.0;
    params.add_crossing(
        Crossing::Chequered,
        START_LINE - 0.5,
        START_LINE + 0.5,
        p.carriage_half(),
    );
    for (c, _) in SIDE_STREETS {
        let (a, z) = (c - sp.road_half(), c + sp.road_half());
        if c < 150.0 {
            params.add_crossing(Crossing::Crosswalk, a - 4.5, a - 1.0, p.carriage_half());
        } else if c < 200.0 {
            params.add_crossing(Crossing::Crosswalk, z + 1.0, z + 4.5, p.carriage_half());
        } else {
            params.add_crossing(Crossing::Crosswalk, a - 4.5, a - 1.0, p.carriage_half());
        }
    }
    let road_mat = Mat::Surface(kit.road(params, surf));
    let len = main.length();
    for (s0, s1) in chunks(0.0, len, 60.0) {
        let r = build_road(&main, &p, &gaps, s0, s1);
        b.add_mesh(r.carriageway, Slot::Road, road_mat.clone(), Band::All);
        let mut parts = Parts::new();
        parts
            .add(Slot::RoadPlain, r.mouths)
            .add(Slot::Kerb, r.kerbs)
            .add(Slot::Sidewalk, r.sidewalks);
        b.add_world(&parts, Band::All, |_| Variant::Slot);
    }
    let rules = DecalRules {
        seed: 21,
        braking_zones: vec![(150.0, CORNER_IN - 2.0), (CORNER_OUT + 160.0, len - 5.0)],
        ..DecalRules::default()
    };
    for (s0, s1) in chunks(0.0, len, 60.0) {
        let mut r = rules.clone();
        r.seed = rules.seed ^ (s0 as u64);
        r.braking_zones.retain(|z| z.1 > s0 && z.0 < s1);
        for z in &mut r.braking_zones {
            *z = (z.0.max(s0), z.1.min(s1));
        }
        let decals = scatter_decals(&p, &r, s0, s1);
        let mut parts = Parts::new();
        parts.add(Slot::Decal, build_decals(&main, &p, &decals));
        b.add_world(&parts, Band::All, |_| Variant::Slot);
    }

    // --- Side streets ----------------------------------------------------
    let side_len = 70.0;
    let mut side_paths = Vec::new();
    for (i, (c, side)) in SIDE_STREETS.iter().enumerate() {
        let sg = side.sign();
        let pose = main.pose(*c);
        let start = pose.pos + pose.right() * sg * p.frontage();
        let mut path =
            Path::new(start, pose.heading + sg * std::f32::consts::FRAC_PI_2).straight(side_len);
        path.elevation = p.road_height(p.road_half());
        let mut sparams = SurfaceParams::road(1, sp.lane_width, flags::ALL);
        sparams.seed = 7.0 + i as f32;
        sparams.add_crossing(Crossing::Crosswalk, 0.8, 4.0, sp.carriage_half());
        sparams.add_crossing(Crossing::StopLine, 4.6, 5.0, sp.carriage_half());
        let mat = Mat::Surface(kit.road(sparams, surf));
        let r = build_road(&path, &sp, &[], 0.0, side_len);
        b.add_mesh(r.carriageway, Slot::Road, mat, Band::All);
        let mut parts = Parts::new();
        parts
            .add(Slot::Kerb, r.kerbs)
            .add(Slot::Sidewalk, r.sidewalks);
        let decals = scatter_decals(
            &sp,
            &DecalRules {
                seed: 40 + i as u64,
                patches: 4.0,
                ..DecalRules::default()
            },
            0.0,
            side_len,
        );
        parts.add(Slot::Decal, build_decals(&path, &sp, &decals));
        b.add_world(&parts, Band::All, |_| Variant::Slot);
        side_paths.push(path);
    }

    // --- Racing furniture -------------------------------------------------
    let bu = p.road_half() - 0.35;
    let by = p.height(bu);
    let (d0, d1) = DRESSED;
    for (s0, s1) in chunks(d0, d1, 40.0) {
        let mut near = Parts::new();
        let mut far = Parts::new();
        let mut all = Parts::new();
        all.merge(
            &racing::jersey_run(&main, s0, s1, -bu, by),
            Affine3A::IDENTITY,
        );
        near.merge(
            &racing::fence_run(&main, s0, s1, -bu, 1.0, by + 0.81, 3.6, 0),
            Affine3A::IDENTITY,
        );
        far.merge(
            &racing::fence_run(&main, s0, s1, -bu, 1.0, by + 0.81, 3.6, 1),
            Affine3A::IDENTITY,
        );
        let armco_from = CORNER_OUT + 40.0;
        let (j1, a0) = (s1.min(armco_from), s0.max(armco_from));
        if j1 > s0 {
            all.merge(
                &racing::jersey_run(&main, s0, j1, bu, by),
                Affine3A::IDENTITY,
            );
            near.merge(
                &racing::fence_run(&main, s0, j1, bu, -1.0, by + 0.81, 3.6, 0),
                Affine3A::IDENTITY,
            );
            far.merge(
                &racing::fence_run(&main, s0, j1, bu, -1.0, by + 0.81, 3.6, 1),
                Affine3A::IDENTITY,
            );
        }
        if s1 > a0 {
            all.merge(
                &racing::armco_run(&main, a0, s1, bu, -1.0, by),
                Affine3A::IDENTITY,
            );
        }
        all.merge(
            &racing::fence_banners(&main, s0, s1, -bu, 1.0, by + 0.81, 8.0, 22.0, s0 as u64),
            Affine3A::IDENTITY,
        );
        if j1 > s0 {
            all.merge(
                &racing::fence_banners(
                    &main,
                    s0,
                    j1,
                    bu,
                    -1.0,
                    by + 0.81,
                    8.0,
                    26.0,
                    s0 as u64 + 1,
                ),
                Affine3A::IDENTITY,
            );
        }
        b.add_world(&all, Band::All, |_| Variant::Slot);
        b.add_world(&near, Band::Near, |_| Variant::Slot);
        b.add_world(&far, Band::Far, |_| Variant::Slot);
    }
    // Tyre wall on the outside of the corner, curbs at entry, apex, exit.
    let face = bu - 0.31;
    for (s0, s1) in chunks(CORNER_IN - 8.0, CORNER_OUT + 12.0, 20.0) {
        b.add_world(
            &racing::tirewall_run(&main, s0, s1, -face, 1.0, by, 0),
            Band::Near,
            |_| Variant::Slot,
        );
        b.add_world(
            &racing::tirewall_run(&main, s0, s1, -face, 1.0, by, 1),
            Band::Far,
            |_| Variant::Slot,
        );
    }
    let ch = p.carriage_half();
    let curbs = [
        racing::curb_run(
            &main,
            CORNER_IN - 14.0,
            CORNER_IN + 4.0,
            -(ch - 1.6),
            -1.0,
            p.height(ch - 1.1),
        ),
        racing::curb_run(
            &main,
            CORNER_IN + 6.0,
            CORNER_OUT - 6.0,
            ch - 1.0,
            1.0,
            p.height(ch - 0.5),
        ),
        racing::curb_run(
            &main,
            CORNER_OUT - 4.0,
            CORNER_OUT + 22.0,
            -(ch - 1.0),
            -1.0,
            p.height(ch - 0.5),
        ),
    ];
    for c in &curbs {
        b.add_world(c, Band::All, |_| Variant::Slot);
    }
    // Start/finish gantry and boards.
    let span = 2.0 * (p.road_half() + 1.6);
    let gt = place(&main, START_LINE, 0.0, 0.0, fwd(&main, START_LINE));
    let (g0, _) = racing::gantry(span, 0);
    let (g1, _) = racing::gantry(span, 1);
    b.add(&g0, gt, Band::Near, |_| Variant::Slot);
    b.add(&g1, gt, Band::Far, |_| Variant::Slot);
    for (s, label) in [(CORNER_IN - 100.0, "100"), (CORNER_IN - 50.0, "50")] {
        b.add(
            &racing::distance_board(label, 0.0),
            place(&main, s, -bu + 0.1, by + 0.81, fwd(&main, s)),
            Band::All,
            |_| Variant::Slot,
        );
    }
    b.add(
        &racing::sector_board("S1", 0.0),
        place(&main, 400.0, -bu + 0.1, by + 0.81, fwd(&main, 400.0)),
        Band::All,
        |_| Variant::Slot,
    );

    // --- Plaza and grandstand at the outside of the corner ----------------
    let plaza_y = p.sidewalk_height(p.frontage());
    let mut plaza = MeshData::new();
    let st = main.stations(CORNER_IN - 6.0, CORNER_OUT + 6.0, 1.0);
    let frames = main.frames_centre_s(&st, 0.0);
    for w in frames.windows(2) {
        let q = |f: &crate::mesh::Frame, u: f32| f.at(Vec2::new(u, plaza_y));
        let (u0, u1) = (-p.frontage(), -p.frontage() - 34.0);
        let (a, c) = (q(&w[0], u0), q(&w[1], u0));
        let (bb, d) = (q(&w[0], u1), q(&w[1], u1));
        plaza.quad_n(
            [a, c, d, bb],
            [
                Vec2::new(a.x, a.z),
                Vec2::new(c.x, c.z),
                Vec2::new(d.x, d.z),
                Vec2::new(bb.x, bb.z),
            ],
            Vec3::Y,
        );
    }
    let mut pp = Parts::new();
    pp.add(Slot::Sidewalk, plaza);
    b.add_world(&pp, Band::All, |_| Variant::Slot);
    let mid = (CORNER_IN + CORNER_OUT) / 2.0;
    let gs = place(
        &main,
        mid,
        -p.frontage() - 13.0,
        plaza_y,
        road_dir(&main, mid, -1.0),
    );
    b.add(&racing::grandstand(36.0, 12, 0), gs, Band::Near, |_| {
        Variant::Slot
    });
    let shirts: [[f32; 3]; 8] = [
        [0.8, 0.1, 0.1],
        [0.1, 0.2, 0.6],
        [0.9, 0.9, 0.88],
        [0.08, 0.08, 0.09],
        [0.9, 0.7, 0.1],
        [0.2, 0.5, 0.25],
        [0.5, 0.5, 0.52],
        [0.6, 0.3, 0.5],
    ];
    for (g, people) in racing::crowd(36.0, 12, 0.7, shirts.len(), 9)
        .iter()
        .enumerate()
    {
        b.add(people, gs, Band::All, |_| tint(shirts[g]));
    }
    b.add(&racing::grandstand(36.0, 12, 1), gs, Band::Far, |_| {
        Variant::Slot
    });
    for k in 0..7 {
        let s = CORNER_IN - 4.0 + k as f32 * (CORNER_OUT - CORNER_IN + 8.0) / 6.0;
        let t = place(
            &main,
            s,
            -p.frontage() - 3.0,
            plaza_y,
            road_dir(&main, s, -1.0),
        );
        b.add(&street::tree(500 + k, 0), t, Band::Near, |_| Variant::Slot);
        b.add(&street::tree(500 + k, 1), t, Band::Far, |_| Variant::Slot);
    }

    // --- Street furniture -------------------------------------------------
    let furn = |path: &Path,
                prof: &RoadProfile,
                side: Side,
                s0: f32,
                s1: f32,
                gaps: &[Gap],
                b: &mut Batcher,
                lights: &mut Vec<LightSpec>,
                rng: &mut Rng,
                main_road: bool| {
        let sg = side.sign();
        let kerb_u = prof.road_half() + prof.kerb_width;
        let y = |u: f32| prof.sidewalk_height(u);
        let rules = &district.street;
        let lamp_off = if sg > 0.0 {
            0.0
        } else {
            rules.lamp_spacing / 2.0
        };
        for (a, z) in solid_intervals(s0, s1, side, gaps) {
            // Lamps.
            let mut lamp_s = Vec::new();
            let mut s = (a / rules.lamp_spacing).ceil() * rules.lamp_spacing + lamp_off;
            while s < z - 2.0 {
                if s > a + 2.0 {
                    let u = sg * (kerb_u + 0.6);
                    let t = place(path, s, u, y(u), road_dir(path, s, sg));
                    let (l0, sockets) = street::street_lamp(1, 0);
                    let (l1, _) = street::street_lamp(1, 1);
                    b.add(&l0, t, Band::Near, |_| Variant::Slot);
                    b.add(&l1, t, Band::Far, |_| Variant::Slot);
                    add_lights(lights, &sockets, t);
                    lamp_s.push(s);
                }
                s += rules.lamp_spacing;
            }
            // Trees in pits between the lamps.
            let mut s = a + rules.tree_spacing / 2.0;
            while s < z - 3.0 {
                if rng.chance(rules.tree_chance) && lamp_s.iter().all(|l| (l - s).abs() > 4.0) {
                    let u = sg * (kerb_u + 1.2);
                    let t = place(path, s, u, y(u), road_dir(path, s, sg));
                    let seed = rng.next_u32() as u64;
                    b.add(&street::tree(seed, 0), t, Band::Near, |_| Variant::Slot);
                    b.add(&street::tree(seed, 1), t, Band::Far, |_| Variant::Slot);
                }
                s += rules.tree_spacing;
            }
            // Small furniture near the kerb or the building line.
            let mut s = a + rng.range(5.0, rules.bin_spacing);
            while s < z - 3.0 {
                let u = sg * (kerb_u + 0.5);
                if lamp_s.iter().all(|l| (l - s).abs() > 2.0) {
                    b.add(
                        &street::bin(0),
                        place(path, s, u, y(u), road_dir(path, s, sg)),
                        Band::NearOnly,
                        |_| Variant::Slot,
                    );
                }
                if rng.chance(rules.bench_chance) {
                    let ub = sg * (prof.frontage() - 1.0);
                    b.add(
                        &street::bench(),
                        place(path, s + 3.0, ub, y(ub), -road_dir(path, s, sg)),
                        Band::NearOnly,
                        |_| Variant::Slot,
                    );
                }
                s += rules.bin_spacing * rng.range(0.7, 1.3);
            }
            let mut s = a + rng.range(8.0, rules.hydrant_spacing);
            while s < z - 2.0 {
                let u = sg * (kerb_u + 0.45);
                b.add(
                    &street::hydrant(0),
                    place(path, s, u, y(u), road_dir(path, s, sg)),
                    Band::NearOnly,
                    |_| Variant::Slot,
                );
                s += rules.hydrant_spacing * rng.range(0.8, 1.2);
            }
            if !main_road {
                // Side streets: parking meters and parked cars.
                let mut s = a + 6.0;
                while s < z - 3.0 {
                    let u = sg * (kerb_u + 0.35);
                    b.add(
                        &street::parking_meter(),
                        place(path, s, u, y(u), road_dir(path, s, sg)),
                        Band::NearOnly,
                        |_| Variant::Slot,
                    );
                    s += rules.meter_spacing;
                }
                let colours: [[f32; 3]; 7] = [
                    [0.9, 0.9, 0.9],
                    [0.05, 0.05, 0.06],
                    [0.55, 0.56, 0.58],
                    [0.08, 0.12, 0.3],
                    [0.5, 0.05, 0.04],
                    [0.25, 0.26, 0.27],
                    [0.75, 0.7, 0.6],
                ];
                let mut s = a + 9.0;
                while s < z - 3.0 {
                    if rng.chance(0.8) {
                        let u = sg * (prof.carriage_half() - 0.65);
                        let dir = fwd(path, s) * if sg > 0.0 { 1.0 } else { -1.0 };
                        let t = place(path, s, u, prof.road_height(u) + 0.0, dir);
                        let c = colours[rng.index(colours.len())];
                        b.add(&street::parked_car(0), t, Band::Near, |slot| {
                            if slot == Slot::CarPaint {
                                tint(c)
                            } else {
                                Variant::Slot
                            }
                        });
                        b.add(&street::parked_car(1), t, Band::Far, |slot| {
                            if slot == Slot::CarPaint {
                                tint(c)
                            } else {
                                Variant::Slot
                            }
                        });
                    }
                    s += rng.range(5.8, 7.5);
                }
            }
        }
    };
    for side in [Side::Left, Side::Right] {
        furn(
            &main,
            &p,
            side,
            d0,
            d1,
            &gaps,
            &mut b,
            &mut lights,
            &mut rng,
            true,
        );
    }
    for path in &side_paths {
        for side in [Side::Left, Side::Right] {
            furn(
                path,
                &sp,
                side,
                8.0,
                side_len,
                &[],
                &mut b,
                &mut lights,
                &mut rng,
                false,
            );
        }
    }
    // Signals, street-name signs and cabinets at each junction.
    for (i, (c, side)) in SIDE_STREETS.iter().enumerate() {
        let sg = side.sign();
        let kerb_u = p.road_half() + p.kerb_width;
        for (s, aspect) in [
            (c - sp.road_half() - 1.5, street::Aspect::Green),
            (c + sp.road_half() + 1.5, street::Aspect::Red),
        ] {
            let u = sg * (kerb_u + 0.4);
            let t = place(&main, s, u, p.sidewalk_height(u), road_dir(&main, s, sg));
            let (s0, _) = street::traffic_signal(5.5, aspect, 0);
            let (s1, _) = street::traffic_signal(5.5, aspect, 1);
            b.add(&s0, t, Band::Near, |_| Variant::Slot);
            b.add(&s1, t, Band::Far, |_| Variant::Slot);
        }
        let s = c + sp.road_half() + 3.0;
        let u = sg * (kerb_u + 1.0);
        b.add(
            &street::road_sign(RoadSignCell::StreetName, 0),
            place(&main, s, u, p.sidewalk_height(u), road_dir(&main, s, sg)),
            Band::Near,
            |_| Variant::Slot,
        );
        b.add(
            &street::utility_cabinet(),
            place(
                &main,
                c - sp.road_half() - 4.0,
                sg * (p.frontage() - 0.8),
                p.sidewalk_height(p.frontage()),
                -road_dir(&main, c - 6.0, sg),
            ),
            Band::NearOnly,
            |_| Variant::Slot,
        );
        // Side-street signals facing traffic coming out of the side street.
        let path = &side_paths[i];
        let t = place(
            path,
            1.5,
            sp.road_half() + sp.kerb_width + 0.4,
            sp.sidewalk_height(4.4),
            road_dir(path, 1.5, 1.0),
        );
        let (s0, _) = street::traffic_signal(3.5, street::Aspect::Red, 0);
        b.add(&s0, t, Band::Near, |_| Variant::Slot);
        let u = -(sp.road_half() + sp.kerb_width + 0.5);
        let sign = if i % 2 == 0 {
            RoadSignCell::OneWay
        } else {
            RoadSignCell::NoParking
        };
        b.add(
            &street::road_sign(sign, 0),
            place(
                path,
                12.0,
                u,
                sp.sidewalk_height(u),
                road_dir(path, 12.0, -1.0),
            ),
            Band::NearOnly,
            |_| Variant::Slot,
        );
        // Bollards across the closed side-street mouth (race day).
        for k in 0..5 {
            let u = -sp.carriage_half() + 0.5 + k as f32 * (2.0 * sp.carriage_half() - 1.0) / 4.0;
            b.add(
                &street::bollard(0),
                place(path, 9.0, u, sp.road_height(u), fwd(path, 9.0)),
                Band::NearOnly,
                |_| Variant::Slot,
            );
        }
    }
    {
        let s = START_LINE - 25.0;
        let u = p.road_half() + p.kerb_width + 1.0;
        b.add(
            &street::road_sign(RoadSignCell::SpeedLimit, 0),
            place(&main, s, u, p.sidewalk_height(u), road_dir(&main, s, 1.0)),
            Band::NearOnly,
            |_| Variant::Slot,
        );
        let s = CORNER_OUT + 120.0;
        let u = p.road_half() + p.kerb_width + 2.2;
        let t = place(&main, s, u, p.sidewalk_height(u), road_dir(&main, s, 1.0));
        let (shelter, sockets) = street::bus_shelter(0);
        b.add(&shelter, t, Band::Near, |_| Variant::Slot);
        add_lights(&mut lights, &sockets, t);
        let s = START_LINE + 40.0;
        let u = -(p.road_half() + p.kerb_width + 1.0);
        b.add(
            &street::road_sign(RoadSignCell::Pedestrian, 0),
            place(&main, s, u, p.sidewalk_height(u), road_dir(&main, s, -1.0)),
            Band::NearOnly,
            |_| Variant::Slot,
        );
    }

    // --- City blocks ------------------------------------------------------
    let f = p.frontage();
    let sf = sp.frontage();
    let rect = |x0: f32, x1: f32, z0: f32, z1: f32, street: [bool; 4]| Lot {
        // Counter-clockwise from above is irrelevant here; edges are
        // (x0,z0)-(x1,z0), (x1,z0)-(x1,z1), (x1,z1)-(x0,z1), (x0,z1)-(x0,z0).
        poly: vec![
            Vec2::new(x0, z0),
            Vec2::new(x1, z0),
            Vec2::new(x1, z1),
            Vec2::new(x0, z1),
        ],
        street: street.to_vec(),
    };
    let zc = |s: f32| 60.0 - s; // z on the first straight
    let xc = |s: f32| 30.0 + (s - CORNER_OUT); // x on the second straight
    let (l_c, r_c, n_c) = (SIDE_STREETS[0].0, SIDE_STREETS[1].0, SIDE_STREETS[2].0);
    let mut blocks = vec![
        // West of the first straight.
        rect(-48.0, -f, zc(l_c) + sf, 70.0, [true, true, false, false]),
        rect(
            -48.0,
            -f,
            zc(CORNER_IN) + 6.0,
            zc(l_c) - sf,
            [false, true, true, false],
        ),
        // East of the first straight, north of the right side street.
        rect(f, 48.0, zc(r_c) + sf, 70.0, [true, false, false, true]),
        // North of the second straight.
        rect(
            38.0,
            xc(n_c) - sf,
            -250.0,
            -200.0 - f,
            [false, true, true, false],
        ),
        rect(
            xc(n_c) + sf,
            260.0,
            -250.0,
            -200.0 - f,
            [false, false, true, true],
        ),
        // South of the second straight, east of the corner block.
        rect(84.0, 260.0, -200.0 + f, -140.0, [true, false, false, false]),
    ];
    // Inner-corner block: first straight, the corner arc, second straight.
    {
        let centre = Vec2::new(30.0, zc(CORNER_IN));
        let r = 30.0 - f;
        let mut poly = vec![Vec2::new(84.0, zc(r_c) - sf), Vec2::new(f, zc(r_c) - sf)];
        let mut street = vec![true, true];
        for k in 0..=9 {
            let a = std::f32::consts::PI + k as f32 / 9.0 * std::f32::consts::FRAC_PI_2;
            poly.push(centre + Vec2::new(a.cos(), a.sin()) * r);
            street.push(true);
        }
        poly.push(Vec2::new(84.0, -200.0 + f));
        street.pop();
        street.push(true);
        street.push(false);
        // Edge i runs from vertex i to i+1; the last edge closes to vertex 0.
        street.truncate(poly.len());
        blocks.push(Lot { poly, street });
    }
    let mut buildings = 0;
    for (i, block) in blocks.iter().enumerate() {
        for bld in assemble_block(block, district, 100 + i as u64, &format!("b{i}")) {
            add_building(&mut b, &bld);
            buildings += 1;
        }
    }
    // Landmark tower north of the corner plaza: closes the view down the
    // first straight without shading the second one from a westerly sun.
    {
        let tower = district
            .archetypes
            .iter()
            .find(|a| a.style == Style::Curtain)
            .expect("a curtain archetype");
        let c = Vec3::new(0.0, 0.0, -268.0);
        let d = Vec3::Z;
        let (dx, dz) = (Vec2::new(d.x, d.z), Vec2::new(-d.z, d.x));
        let cc = Vec2::new(c.x, c.z);
        let poly = vec![
            cc - dx * 16.0 - dz * 16.0,
            cc - dx * 16.0 + dz * 16.0,
            cc + dx * 16.0 + dz * 16.0,
            cc + dx * 16.0 - dz * 16.0,
        ];
        let lot = Lot {
            street: vec![true; 4],
            poly,
        };
        let mut arch = tower.clone();
        arch.floors = [38, 38];
        add_building(
            &mut b,
            &build(&lot, &arch, district, 77, "bldg_landmark".into()),
        );
        buildings += 1;
    }
    // Infill: the rest of the street grid as cheap massing (HLOD dressing
    // for everything beyond the corridor): 60 m blocks, 14 m streets.
    {
        let corridor = (Vec2::new(-52.0, -256.0), Vec2::new(266.0, 74.0));
        let mut sides = MeshData::new();
        let mut roofs = MeshData::new();
        let pitch = 74.0;
        for i in -5..=6 {
            for j in -7..=4 {
                let x0 = -60.0 + i as f32 * pitch;
                let z0 = -250.0 + j as f32 * pitch;
                let (lo, hi) = (Vec2::new(x0, z0), Vec2::new(x0 + 60.0, z0 + 60.0));
                if hi.x > corridor.0.x
                    && lo.x < corridor.1.x
                    && hi.y > corridor.0.y
                    && lo.y < corridor.1.y
                {
                    continue;
                }
                // Split each block into two to four masses of random height.
                let n = 2 + rng.index(3);
                for k in 0..n {
                    let (a, z) = (k as f32 / n as f32, (k + 1) as f32 / n as f32);
                    let (mx0, mx1) = (lo.x + 60.0 * a, lo.x + 60.0 * z);
                    let d = rng.range(25.0, 60.0);
                    let tall = rng.chance(0.25);
                    let h = if tall {
                        rng.range(60.0, 150.0)
                    } else {
                        rng.range(14.0, 45.0)
                    };
                    let m =
                        MeshData::cuboid(Vec3::new(mx0, -0.3, lo.y), Vec3::new(mx1, h, lo.y + d));
                    // Keep the four sides for the window texture, roofs apart.
                    let mut walls = MeshData::new();
                    let mut top = MeshData::new();
                    for q in m.indices.chunks(6) {
                        let n0 = m.normals[q[0] as usize];
                        let target = if n0.y.abs() > 0.5 {
                            &mut top
                        } else {
                            &mut walls
                        };
                        let base = target.positions.len() as u32;
                        let idx: Vec<u32> = q
                            .iter()
                            .map(|i| {
                                let i = *i as usize;
                                target.vertex(m.positions[i], m.normals[i], m.uvs[i])
                            })
                            .collect();
                        let _ = base;
                        target
                            .indices
                            .extend_from_slice(&[idx[0], idx[1], idx[2], idx[3], idx[4], idx[5]]);
                    }
                    sides.append(&walls);
                    roofs.append(&top);
                }
            }
        }
        let mut infill = Parts::new();
        infill.add(Slot::Skyline, sides).add(Slot::Roof, roofs);
        b.add_world(&infill, Band::All, |_| Variant::Slot);
    }
    // Distant skyline (HLOD boxes).
    let mut sky = MeshData::new();
    let centre = Vec3::new(60.0, 0.0, -150.0);
    let mut placed: Vec<Vec2> = Vec::new();
    while placed.len() < 70 {
        let a = rng.range(0.0, std::f32::consts::TAU);
        let r = rng.range(330.0, 1100.0);
        let q = Vec2::new(centre.x + a.cos() * r, centre.z + a.sin() * r);
        if placed.iter().any(|o| o.distance(q) < 60.0) {
            continue;
        }
        placed.push(q);
        let (w, d) = (rng.range(22.0, 48.0), rng.range(22.0, 48.0));
        let h = rng.range(45.0, 240.0) * if r < 600.0 { 1.0 } else { 1.2 };
        let m = MeshData::cuboid(
            Vec3::new(q.x - w / 2.0, -1.0, q.y - d / 2.0),
            Vec3::new(q.x + w / 2.0, h, q.y + d / 2.0),
        );
        sky.append(&m);
    }
    let mut sp_parts = Parts::new();
    sp_parts.add(Slot::Skyline, sky);
    b.add_world(&sp_parts, Band::All, |_| Variant::Slot);
    // Ground under everything.
    let mut ground = Parts::new();
    ground.add(
        Slot::RoadPlain,
        MeshData::cuboid(
            Vec3::new(-1500.0, -0.6, -1600.0),
            Vec3::new(1600.0, -0.25, 1500.0),
        ),
    );
    b.add_world(&ground, Band::All, |_| Variant::Slot);
    (b, lights, buildings)
}

fn add_building(b: &mut Batcher, bld: &crate::building::Building) {
    let t = bld.tints;
    b.add_world(&bld.parts, Band::BuildingNear, |slot| match slot {
        Slot::Brick | Slot::Plaster => tint(t.wall),
        Slot::Stone => tint(t.trim),
        Slot::CurtainGlass => tint(t.glass),
        Slot::Awning => tint(t.awning),
        _ => Variant::Slot,
    });
    // Far shell: window grid tinted towards the wall colour.
    let shell = if bld.style == Style::Curtain {
        [0.8, 0.9, 1.0]
    } else {
        t.wall.map(|c| (c * 1.6).min(1.0))
    };
    b.add_world(&bld.lod1(), Band::BuildingFar, |slot| {
        if slot == Slot::Skyline {
            tint(shell)
        } else {
            Variant::Slot
        }
    });
}

/// Scripted camera poses for the captures (name, eye, target).
pub fn camera_poses() -> Vec<(&'static str, Vec3, Vec3)> {
    let m = main_path();
    vec![
        (
            "start-straight",
            m.point(START_LINE - 22.0, 1.7, 1.25),
            m.point(CORNER_IN - 20.0, -1.0, 4.0),
        ),
        (
            "braking-zone",
            m.point(CORNER_IN - 70.0, -1.5, 3.2),
            m.point(CORNER_IN + 18.0, -3.0, 3.0),
        ),
        (
            "corner-apex",
            m.point(CORNER_IN + 10.0, 3.0, 1.3),
            m.point(CORNER_OUT + 10.0, -5.0, 3.0),
        ),
        (
            "sidewalk",
            m.point(150.0, 10.3, 1.65),
            m.point(190.0, 6.0, 1.5),
        ),
        (
            "exit-straight",
            m.point(CORNER_OUT + 25.0, -1.7, 1.25),
            m.point(CORNER_OUT + 160.0, 0.0, 6.0),
        ),
        (
            "drone",
            m.point(CORNER_IN + 22.0, -30.0, 42.0),
            m.point(CORNER_IN - 110.0, 2.0, 0.0),
        ),
        (
            "into-sun",
            m.point(CORNER_OUT + 120.0, 2.0, 1.4),
            m.point(CORNER_OUT - 10.0, 0.0, 5.0),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> (BatchStats, usize, usize) {
        let (mut images, mut std_mats, mut surf) = (
            Assets::<Image>::default(),
            Assets::<StandardMaterial>::default(),
            Assets::<SurfaceMaterial>::default(),
        );
        let kit = KitMaterials::new(
            TimeOfDay::Night,
            0.0,
            &TextureSource::Procedural,
            &mut images,
            &mut std_mats,
            &mut surf,
        );
        let (b, lights, buildings) = build_showcase(&District::downtown(), &kit, &mut surf);
        (b.stats(), lights.len(), buildings)
    }

    #[test]
    fn showcase_is_deterministic_and_within_budget() {
        let (a, lights, buildings) = layout();
        assert_eq!(layout(), (a, lights, buildings));
        // CPU-side budgets for the corridor (see docs/assets/track-kit.md).
        assert!(a.entities < 1_000, "{a:?}");
        assert!(a.triangles_lod0 < 900_000, "{a:?}");
        assert!(a.triangles_far < 700_000, "{a:?}");
        assert!((30..=60).contains(&lights), "{lights}");
        assert!(buildings >= 30, "{buildings}");
    }

    #[test]
    fn spot_intensity_concentrates_flux() {
        // A 180° cone (hemisphere) doubles the point-light equivalent.
        assert!((spot_intensity(1000.0, 90.0) - 1600.0).abs() < 1.0);
        assert!(spot_intensity(1000.0, 30.0) > spot_intensity(1000.0, 60.0));
    }

    #[test]
    fn side_streets_meet_the_main_frontage() {
        let main = main_path();
        let p = RoadProfile::default();
        for (c, side) in SIDE_STREETS {
            let pose = main.pose(c);
            let start = pose.pos + pose.right() * side.sign() * p.frontage();
            let q = main.point(c, side.sign() * p.frontage(), 0.0);
            assert!((Vec2::new(q.x, q.z) - start).length() < 1e-3);
        }
    }
}
