//! The kit's asset catalogue: every generator instantiated as a named asset
//! with its LOD chain, sockets and triangle budget, for validation, export
//! and the handoff note's budget table.

use bevy::math::{Vec2, Vec3};

use crate::{
    building::{Lot, build},
    district::District,
    kit::{Asset, Parts, Slot, Socket},
    path::Path,
    props::{racing, street},
    road::{DecalRules, Gap, RoadProfile, Side, build_decals, build_road, scatter_decals},
    texture::RoadSignCell,
    validate::{Budget, Findings, check_asset},
};

fn budget(max_triangles: usize) -> Budget {
    Budget {
        max_triangles,
        lod_ratio: 0.8,
    }
}

fn asset(
    name: &str,
    lods: Vec<Parts>,
    lod_distances: Vec<f32>,
    sockets: Vec<Socket>,
    max: usize,
) -> Asset {
    Asset {
        name: name.into(),
        lods: lods.into_iter().map(Parts::into_parts).collect(),
        lod_distances,
        sockets,
        budget: budget(max),
    }
}

fn lods(f: impl Fn(usize) -> Parts) -> Vec<Parts> {
    vec![f(0), f(1)]
}

/// Every asset of the kit.
pub fn catalogue() -> Vec<Asset> {
    let mut out = Vec::new();
    let straight = Path::new(Vec2::ZERO, 0.0).straight(20.0);
    let corner = Path::new(Vec2::ZERO, 0.0).arc(30.0, 90.0);
    let p = RoadProfile::default();

    // Road sections (per 20 m / per 90° corner).
    for (name, path, gaps) in [
        ("road_avenue_straight_20m", &straight, vec![]),
        ("road_avenue_corner_r30", &corner, vec![]),
        (
            "road_avenue_junction",
            &straight,
            vec![Gap {
                side: Side::Left,
                s0: 6.3,
                s1: 13.7,
            }],
        ),
    ] {
        let r = build_road(path, &p, &gaps, 0.0, path.length());
        let mut parts = Parts::new();
        parts
            .add(Slot::Road, r.carriageway)
            .add(Slot::RoadPlain, r.mouths);
        parts
            .add_closed(Slot::Kerb, r.kerbs)
            .add_closed(Slot::Sidewalk, r.sidewalks);
        out.push(asset(name, vec![parts], vec![], vec![], 3000));
    }
    let decals = scatter_decals(
        &p,
        &DecalRules {
            braking_zones: vec![(10.0, 50.0)],
            ..DecalRules::default()
        },
        0.0,
        60.0,
    );
    let mut d = Parts::new();
    d.add(
        Slot::Decal,
        build_decals(&Path::new(Vec2::ZERO, 0.0).straight(60.0), &p, &decals),
    );
    out.push(asset("decal_road_set_60m", vec![d], vec![], vec![], 4000));

    // Racing furniture (linear pieces on a short straight).
    let run = Path::new(Vec2::ZERO, 0.0).straight(7.6);
    out.push(asset(
        "race_barrier_jersey",
        vec![racing::jersey_run(&run, 0.0, 7.6, 0.0, 0.0)],
        vec![],
        vec![],
        400,
    ));
    out.push(asset(
        "race_curb",
        vec![racing::curb_run(&run, 0.0, 7.6, 0.0, 1.0, 0.0)],
        vec![],
        vec![],
        200,
    ));
    out.push(asset(
        "race_armco",
        vec![racing::armco_run(&run, 0.0, 7.6, 0.0, 1.0, 0.0)],
        vec![],
        vec![],
        600,
    ));
    out.push(asset(
        "race_tirewall",
        lods(|l| racing::tirewall_run(&run, 0.0, 7.6, 0.0, 1.0, 0.0, l)),
        vec![70.0],
        vec![],
        12_000,
    ));
    out.push(asset(
        "race_catch_fence",
        lods(|l| racing::fence_run(&run, 0.0, 7.6, 0.0, 1.0, 0.81, 3.6, l)),
        vec![70.0],
        vec![],
        1500,
    ));
    let (g0, sockets) = racing::gantry(17.8, 0);
    let (g1, _) = racing::gantry(17.8, 1);
    out.push(asset(
        "race_gantry_start",
        vec![g0, g1],
        vec![70.0],
        sockets,
        30_000,
    ));
    out.push(asset(
        "race_board_sector",
        vec![racing::sector_board("S1", 0.0)],
        vec![],
        vec![],
        500,
    ));
    out.push(asset(
        "race_board_distance",
        vec![racing::distance_board("100", 0.0)],
        vec![],
        vec![],
        500,
    ));
    out.push(asset(
        "race_fence_banners",
        vec![racing::fence_banners(
            &Path::new(Vec2::ZERO, 0.0).straight(40.0),
            0.0,
            40.0,
            0.0,
            1.0,
            0.81,
            8.0,
            15.0,
            1,
        )],
        vec![],
        vec![],
        400,
    ));
    let mut crowd = Parts::new();
    for g in racing::crowd(36.0, 12, 0.7, 4, 9) {
        crowd.merge(&g, bevy::math::Affine3A::IDENTITY);
    }
    out.push(asset(
        "race_grandstand_crowd",
        vec![crowd],
        vec![],
        vec![],
        20_000,
    ));
    out.push(asset(
        "race_grandstand",
        lods(|l| racing::grandstand(36.0, 12, l)),
        vec![70.0],
        vec![],
        40_000,
    ));

    // Street furniture.
    for (name, arms) in [("furn_lamp_single", 1), ("furn_lamp_double", 2)] {
        let (l0, sockets) = street::street_lamp(arms, 0);
        let (l1, _) = street::street_lamp(arms, 1);
        out.push(asset(name, vec![l0, l1], vec![70.0], sockets, 1500));
    }
    let (s0, sockets) = street::traffic_signal(5.5, street::Aspect::Green, 0);
    let (s1, _) = street::traffic_signal(5.5, street::Aspect::Green, 1);
    out.push(asset(
        "furn_traffic_signal",
        vec![s0, s1],
        vec![70.0],
        sockets,
        3000,
    ));
    for cell in RoadSignCell::ALL {
        let name = format!("furn_sign_{}", format!("{cell:?}").to_lowercase());
        if cell.round() {
            out.push(asset(
                &name,
                lods(|l| street::road_sign(cell, l)),
                vec![70.0],
                vec![],
                300,
            ));
        } else {
            out.push(asset(
                &name,
                vec![street::road_sign(cell, 0)],
                vec![],
                vec![],
                300,
            ));
        }
    }
    out.push(asset(
        "furn_bollard",
        lods(street::bollard),
        vec![70.0],
        vec![],
        200,
    ));
    out.push(asset(
        "furn_bin",
        lods(street::bin),
        vec![70.0],
        vec![],
        300,
    ));
    out.push(asset(
        "furn_bench",
        vec![street::bench()],
        vec![],
        vec![],
        300,
    ));
    let (b0, sockets) = street::bus_shelter(0);
    let (b1, _) = street::bus_shelter(1);
    out.push(asset(
        "furn_bus_shelter",
        vec![b0, b1],
        vec![70.0],
        sockets,
        2000,
    ));
    out.push(asset(
        "furn_hydrant",
        lods(street::hydrant),
        vec![70.0],
        vec![],
        500,
    ));
    out.push(asset(
        "furn_parking_meter",
        vec![street::parking_meter()],
        vec![],
        vec![],
        200,
    ));
    out.push(asset(
        "furn_utility_cabinet",
        vec![street::utility_cabinet()],
        vec![],
        vec![],
        100,
    ));
    out.push(asset(
        "furn_tree",
        lods(|l| street::tree(1, l)),
        vec![70.0],
        vec![],
        2500,
    ));
    out.push(asset(
        "furn_car_parked",
        lods(street::parked_car),
        vec![70.0],
        vec![],
        800,
    ));

    // One building per archetype on a 30 × 25 m corner lot.
    let district = District::downtown();
    let lot = Lot {
        poly: vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(30.0, 0.0),
            Vec2::new(30.0, 25.0),
            Vec2::new(0.0, 25.0),
        ],
        street: vec![true, false, false, true],
    };
    for (i, arch) in district.archetypes.iter().enumerate() {
        let b = build(
            &lot,
            arch,
            &district,
            i as u64 + 1,
            format!("bldg_{}", arch.name),
        );
        let far = b.lod1();
        out.push(Asset {
            name: b.name.clone(),
            lods: vec![b.parts.into_parts(), far.into_parts()],
            lod_distances: vec![crate::batch::BUILDING_FAR],
            sockets: vec![],
            budget: budget(90_000),
        });
    }
    out
}

/// Validates one asset including its sockets.
pub fn check(a: &Asset) -> Findings {
    let mut f = check_asset(&a.name, &a.lods, a.budget);
    if a.lods.len() > 1 && a.lod_distances.len() + 1 != a.lods.len() {
        f.errors.push(format!(
            "{}: {} LODs but {} switch distances",
            a.name,
            a.lods.len(),
            a.lod_distances.len()
        ));
    }
    for s in &a.sockets {
        if !s.pos.is_finite()
            || (s.dir.length() - 1.0).abs() > 1e-3
            || !s
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            f.errors
                .push(format!("{}: bad socket {:?}", a.name, s.name));
        }
    }
    f
}

/// Bounds of an asset's LOD0 (for the docs table).
pub fn bounds(a: &Asset) -> Option<(Vec3, Vec3)> {
    a.lods
        .first()?
        .iter()
        .filter_map(|p| p.mesh.bounds())
        .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))
}

/// Merges an asset's LOD into one `Parts` (for export).
pub fn lod_parts(a: &Asset, lod: usize) -> Parts {
    let mut p = Parts::new();
    for part in a.lods.get(lod).into_iter().flatten() {
        p.add(part.material, part.mesh.clone());
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_asset_passes_validation() {
        let all = catalogue();
        assert!(all.len() > 35, "{}", all.len());
        let mut errors = Vec::new();
        for a in &all {
            errors.extend(check(a).errors);
        }
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }

    #[test]
    fn names_are_unique() {
        let all = catalogue();
        let mut names: Vec<&str> = all.iter().map(|a| a.name.as_str()).collect();
        names.sort();
        let n = names.len();
        names.dedup();
        assert_eq!(n, names.len());
    }

    #[test]
    fn catalogue_is_deterministic() {
        let (a, b) = (catalogue(), catalogue());
        for (x, y) in a.iter().zip(&b) {
            for (p, q) in x.lods.iter().flatten().zip(y.lods.iter().flatten()) {
                assert_eq!(p.mesh.content_hash(), q.mesh.content_hash(), "{}", x.name);
            }
        }
    }

    #[test]
    fn lamps_have_downward_light_sockets() {
        let all = catalogue();
        let lamp = all.iter().find(|a| a.name == "furn_lamp_double").unwrap();
        assert_eq!(lamp.sockets.len(), 2);
        assert!(lamp.sockets.iter().all(|s| s.dir.y < -0.9 && s.pos.y > 8.0));
    }
}
