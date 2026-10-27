//! The shipped track builds, its generated data is current, and the racing
//! line and speed profiles behave.

use racer_trackc::{Limits, line, profile, tracks_dir};

fn dir() -> std::path::PathBuf {
    tracks_dir().join("downtown-loop")
}

#[test]
fn generated_data_matches_the_track() {
    let stored = racer_trackc::load_dir(&dir()).unwrap();
    let (line, cars) = racer_trackc::generate(&stored.geometry).unwrap();
    let worst = line
        .points
        .iter()
        .zip(&stored.line.points)
        .map(|(a, b)| (a.offset - b.offset).abs())
        .fold(0.0, f32::max);
    assert!(
        worst < 0.01,
        "racing line moved by {worst} m: run `racer-trackc compile`"
    );
    for car in &cars {
        let old = stored.car(&car.model.id).expect("car in summary.toml");
        let close = |a: f32, b: f32| (a - b).abs() <= 1e-3 * a.abs().max(1.0);
        let m = (&car.model, &old.model);
        assert!(
            close(m.0.lateral, m.1.lateral)
                && close(m.0.braking, m.1.braking)
                && close(m.0.top_speed, m.1.top_speed)
                && m.0.drive.iter().zip(&m.1.drive).all(|(a, b)| close(*a, *b)),
            "{}: car model changed: run `racer-trackc compile`",
            car.model.id
        );
        assert!(
            (car.lap_time - old.lap_time).abs() < 0.05,
            "{} {} {}",
            car.model.id,
            car.lap_time,
            old.lap_time
        );
    }
}

#[test]
fn geometry_is_closed_and_walled() {
    let g = racer_trackc::build(&dir()).unwrap();
    let n = g.samples.len();
    assert!((2000.0..2300.0).contains(&g.length), "{}", g.length);
    for k in 0..n {
        let (a, b) = (&g.samples[k], &g.samples[(k + 1) % n]);
        assert!(a.pos.distance(b.pos) < 1.2, "gap at {}", a.s);
        assert!(a.up.y > 0.98 && a.right.length() > 0.99);
        assert!(a.wall(0) > a.half_width && a.wall(1) > a.half_width);
    }
    // Every wall triangle stands beside the road, not on it.
    let reach = g
        .samples
        .iter()
        .map(|s| s.half_width)
        .fold(f32::MAX, f32::min);
    for v in &g.walls.vertices {
        let nearest = g
            .samples
            .iter()
            .map(|s| s.pos.with_y(0.0).distance(v.with_y(0.0)))
            .fold(f32::MAX, f32::min);
        assert!(nearest > reach, "wall vertex {v} on the road");
    }
    // Grid slots on the road, behind the line, in order; checkpoints ascending.
    for (i, slot) in g.grid.iter().enumerate() {
        let p = racer_trackc::geometry::interpolate(
            &g.samples,
            g.length,
            g.length - 4.0 - 7.0 * i as f32,
        );
        assert!(slot.position.distance(p.pos) < p.half_width);
    }
    assert!(g.checkpoints.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(g.sector_ends.len(), 2);
    // City character: 90-degree corners, a hairpin, a long straight, elevation.
    let tightest = g
        .samples
        .iter()
        .map(|s| s.curvature.abs())
        .fold(0.0, f32::max);
    assert!(tightest > 1.0 / 12.0, "hairpin");
    let climb = g.samples.iter().map(|s| s.pos.y).fold(0.0, f32::max);
    assert!(climb > 5.0, "overpass");
    let mut straight = 0.0f32;
    let mut run = 0.0;
    // Twice round: the main straight runs through the start line.
    for s in g.samples.iter().chain(&g.samples) {
        run = if s.curvature == 0.0 { run + 1.0 } else { 0.0 };
        straight = straight.max(run);
    }
    assert!(straight > 400.0, "longest straight {straight}");
}

#[test]
fn racing_line_stays_on_the_road_and_takes_the_hairpin_wide() {
    let t = racer_trackc::load_dir(&dir()).unwrap();
    for (p, s) in t.line.points.iter().zip(&t.geometry.samples) {
        let (lo, hi) = line::bounds(s, line::CAR_HALF_WIDTH);
        assert!(
            p.offset >= lo - 1e-3 && p.offset <= hi + 1e-3,
            "{} at {}",
            p.offset,
            s.s
        );
    }
    // Sharper than the centreline nowhere by much: the line opens corners up.
    let line_max = t
        .line
        .points
        .iter()
        .map(|p| p.curvature.abs())
        .fold(0.0, f32::max);
    let centre_max = t
        .geometry
        .samples
        .iter()
        .map(|s| s.curvature.abs())
        .fold(0.0, f32::max);
    assert!(
        line_max < centre_max,
        "line {line_max} centreline {centre_max}"
    );
    assert!(t.line.length < t.geometry.length);
}

#[test]
fn profiles_slow_for_corners_and_scale_with_the_limits() {
    let t = racer_trackc::load_dir(&dir()).unwrap();
    for car in &t.cars {
        let v = &car.speeds;
        let (min, max) = (
            v.iter().copied().fold(f32::MAX, f32::min),
            v.iter().copied().fold(0.0, f32::max),
        );
        assert!(min > 7.0 && min < 15.0, "{} hairpin {min}", car.model.id);
        assert!(
            max > 40.0 && max <= car.model.top_speed + 0.1,
            "{} {max}",
            car.model.id
        );
        // Lateral acceleration never above the model's grip.
        for (p, s) in t.line.points.iter().zip(v) {
            assert!(s * s * p.curvature.abs() <= car.model.lateral * 1.001);
        }
        let slower = profile::speed_profile(
            &t.line,
            &car.model,
            Limits {
                lateral: 0.8,
                braking: 0.8,
                drive: 0.8,
            },
        );
        assert!(profile::lap_time(&t.line, &slower) > car.lap_time * 1.08);
    }
    // Faster cars are faster.
    let lap = |id: &str| t.car(id).unwrap().lap_time;
    assert!(lap("porsche-911-carrera-s-992") < lap("bmw-m3-competition-g80"));
    assert!(lap("bmw-m3-competition-g80") < lap("vw-golf-gti-mk8"));
}
