//! The published dimensions a car asset is checked against, read from
//! `data/cars/<id>/car.toml`.

use std::path::Path;

use serde::Deserialize;

/// Tyre size from an ISO metric designation such as `245/35 ZR20`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tyre {
    /// Section width (m).
    pub width: f32,
    /// Unloaded outer radius (m): rim radius + sidewall height.
    pub radius: f32,
}

impl Tyre {
    /// Parses `245/35 ZR20`, `235/35 R19`, `305/30ZR21` and similar.
    pub fn parse(s: &str) -> Result<Tyre, String> {
        let err = || format!("cannot parse tyre size {s:?}");
        let (width, rest) = s.trim().split_once('/').ok_or_else(err)?;
        let width: f32 = width.trim().parse().map_err(|_| err())?;
        let aspect_end = rest.find(|c: char| !c.is_ascii_digit()).ok_or_else(err)?;
        let aspect: f32 = rest[..aspect_end].parse().map_err(|_| err())?;
        let rim_start = rest.rfind(|c: char| !c.is_ascii_digit()).ok_or_else(err)?;
        let rim: f32 = rest[rim_start + 1..].parse().map_err(|_| err())?;
        Ok(Tyre {
            width: width / 1000.0,
            radius: rim * 0.0254 / 2.0 + width * aspect / 100.0 / 1000.0,
        })
    }
}

/// Target dimensions of a car (metres).
#[derive(Clone, Debug, PartialEq)]
pub struct CarSpec {
    pub id: String,
    pub name: String,
    /// Overall length, width without mirrors and height.
    pub length: f32,
    pub width: f32,
    pub height: f32,
    pub wheelbase: f32,
    pub track_front: f32,
    pub track_rear: f32,
    pub tyre_front: Tyre,
    pub tyre_rear: Tyre,
}

#[derive(Deserialize)]
struct File {
    id: String,
    name: String,
    chassis: Chassis,
    tires: Tires,
}

#[derive(Deserialize)]
struct Chassis {
    length_m: f32,
    width_m: f32,
    height_m: f32,
    wheelbase_m: f32,
    track_front_m: f32,
    track_rear_m: f32,
}

#[derive(Deserialize)]
struct Tires {
    front: String,
    rear: String,
}

impl CarSpec {
    pub fn from_toml(text: &str) -> Result<CarSpec, String> {
        let f: File = toml::from_str(text).map_err(|e| e.to_string())?;
        Ok(CarSpec {
            id: f.id,
            name: f.name,
            length: f.chassis.length_m,
            width: f.chassis.width_m,
            height: f.chassis.height_m,
            wheelbase: f.chassis.wheelbase_m,
            track_front: f.chassis.track_front_m,
            track_rear: f.chassis.track_rear_m,
            tyre_front: Tyre::parse(&f.tires.front)?,
            tyre_rear: Tyre::parse(&f.tires.rear)?,
        })
    }

    pub fn load(path: &Path) -> Result<CarSpec, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        CarSpec::from_toml(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Expected hub centre of a wheel (`fl`, `fr`, `rl`, `rr`) in the asset
    /// convention: +Y up, front towards −Z, right towards +X, origin on the
    /// ground at the wheelbase midpoint.
    pub fn hub(&self, wheel: &str) -> [f32; 3] {
        let front = wheel.starts_with('f');
        let (track, tyre) = if front {
            (self.track_front, self.tyre_front)
        } else {
            (self.track_rear, self.tyre_rear)
        };
        let x = if wheel.ends_with('l') {
            -track / 2.0
        } else {
            track / 2.0
        };
        let z = if front {
            -self.wheelbase / 2.0
        } else {
            self.wheelbase / 2.0
        };
        [x, tyre.radius, z]
    }

    pub fn tyre(&self, wheel: &str) -> Tyre {
        if wheel.starts_with('f') {
            self.tyre_front
        } else {
            self.tyre_rear
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tyre_sizes() {
        let t = Tyre::parse("245/35 ZR20").unwrap();
        assert!((t.width - 0.245).abs() < 1e-6);
        assert!((t.radius - (0.254 + 0.08575)).abs() < 1e-5);
        let t = Tyre::parse("305/30ZR21").unwrap();
        assert!((t.radius - (0.2667 + 0.0915)).abs() < 1e-5);
        assert!(Tyre::parse("wide").is_err());
    }

    #[test]
    fn the_911_spec_loads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/cars/porsche-911-carrera-s-992/car.toml");
        let spec = CarSpec::load(&path).unwrap();
        assert_eq!(spec.wheelbase, 2.45);
        assert_eq!(
            spec.hub("fl"),
            [-1.589 / 2.0, spec.tyre_front.radius, -1.225]
        );
    }
}
