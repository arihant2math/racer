//! Time-of-day presets for the kit scenes: sun/moon, sky, exposure,
//! tonemapping, bloom and colour grading (PLAN.md section 6, "Lighting").

use bevy::{
    camera::Exposure,
    core_pipeline::tonemapping::Tonemapping,
    light::{
        Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder,
        atmosphere::ScatteringMedium, light_consts::lux,
    },
    pbr::{AtmosphereSettings, DistanceFog, FogFalloff},
    post_process::bloom::Bloom,
    prelude::*,
    camera::Hdr,
    render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection},
};

use crate::materials::TimeOfDay;

/// Sun (or moon) direction: unit vector from the scene towards the light.
pub fn sun_direction(time: TimeOfDay) -> Vec3 {
    let (elev, azim): (f32, f32) = match time {
        // Azimuth measured from −Z (north) clockwise towards +X (east).
        TimeOfDay::Day => (52.0, 150.0),
        TimeOfDay::Golden => (9.0, 266.0),
        TimeOfDay::Night => (35.0, 200.0),
    };
    let (e, a) = (elev.to_radians(), azim.to_radians());
    Vec3::new(a.sin() * e.cos(), e.sin(), -a.cos() * e.cos())
}

/// Sky, sun/moon and global ambient for `time`.
pub fn spawn_sky(commands: &mut Commands, media: &mut Assets<ScatteringMedium>, time: TimeOfDay) {
    commands.spawn(Atmosphere::earth(media.add(ScatteringMedium::earth(256, 256))));
    let dir = sun_direction(time);
    let (illuminance, color, shadows) = match time {
        TimeOfDay::Day => (lux::RAW_SUNLIGHT, Color::WHITE, true),
        TimeOfDay::Golden => (lux::RAW_SUNLIGHT * 0.8, Color::srgb(1.0, 0.9, 0.78), true),
        // Moonlight: ~0.25 lux, cool; the atmosphere then renders a dark sky.
        TimeOfDay::Night => (0.6, Color::srgb(0.7, 0.8, 1.0), false),
    };
    commands.spawn((
        DirectionalLight {
            illuminance,
            color,
            shadow_maps_enabled: shadows,
            ..default()
        },
        Transform::from_translation(dir * 100.0).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            maximum_distance: 260.0,
            first_cascade_far_bound: 30.0,
            num_cascades: 4,
            ..default()
        }
        .build(),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: match time {
            TimeOfDay::Night => Color::srgb(0.55, 0.5, 0.6),
            _ => Color::WHITE,
        },
        // Night: sodium/LED sky glow bounced off the street canyon.
        brightness: match time {
            TimeOfDay::Day => 0.0,
            TimeOfDay::Golden => 0.0,
            TimeOfDay::Night => 2.0,
        },
        affects_lightmapped_meshes: true,
    });
}

/// Camera components for `time` (add a `Transform` and any marker).
pub fn camera_bundle(time: TimeOfDay, wet: bool) -> impl Bundle {
    let ev100 = match time {
        TimeOfDay::Day => 13.6,
        TimeOfDay::Golden => 12.2,
        TimeOfDay::Night => 2.2,
    };
    let grading = match time {
        TimeOfDay::Day => ColorGrading {
            global: ColorGradingGlobal { post_saturation: 1.05, ..default() },
            ..default()
        },
        TimeOfDay::Golden => ColorGrading {
            global: ColorGradingGlobal { temperature: 0.04, post_saturation: 1.05, ..default() },
            shadows: ColorGradingSection { saturation: 1.05, ..default() },
            ..default()
        },
        // Teal shadows, warm highlights (night street look).
        TimeOfDay::Night => ColorGrading {
            global: ColorGradingGlobal { post_saturation: 1.1, ..default() },
            shadows: ColorGradingSection { saturation: 1.1, contrast: 1.05, ..default() },
            ..default()
        },
    };
    let fog = match time {
        TimeOfDay::Night => DistanceFog {
            color: Color::srgb(0.035, 0.03, 0.03),
            falloff: FogFalloff::Exponential { density: if wet { 0.006 } else { 0.004 } },
            ..default()
        },
        _ => DistanceFog {
            color: Color::srgba(0.6, 0.65, 0.72, 0.0),
            falloff: FogFalloff::Linear { start: 5000.0, end: 6000.0 },
            ..default()
        },
    };
    (
        Camera3d::default(),
        Hdr,
        AtmosphereSettings::default(),
        AtmosphereEnvironmentMapLight::default(),
        Exposure { ev100 },
        Tonemapping::AgX,
        Bloom { intensity: if time == TimeOfDay::Night { 0.22 } else { 0.08 }, ..Bloom::NATURAL },
        grading,
        fog,
        Projection::Perspective(PerspectiveProjection { fov: 55f32.to_radians(), near: 0.1, far: 3000.0, ..default() }),
    )
}
