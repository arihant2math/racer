//! Time-of-day presets for the kit scenes: sun/moon, sky, exposure,
//! tonemapping, bloom and colour grading (PLAN.md section 6, "Lighting").

use bevy::{
    camera::Exposure,
    camera::Hdr,
    core_pipeline::tonemapping::Tonemapping,
    light::{
        Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder,
        atmosphere::ScatteringMedium, light_consts::lux,
    },
    pbr::{AtmosphereSettings, DistanceFog, FogFalloff},
    post_process::bloom::Bloom,
    prelude::*,
    render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection},
};

use crate::materials::TimeOfDay;

/// Sun (or moon) direction: unit vector from the scene towards the light.
pub fn sun_direction(time: TimeOfDay) -> Vec3 {
    let (elev, azim): (f32, f32) = match time {
        // Azimuth measured from −Z (north) clockwise towards +X (east).
        TimeOfDay::Day => (52.0, 150.0),
        TimeOfDay::Golden => (7.0, 285.0),
        TimeOfDay::Night => (35.0, 200.0),
    };
    let (e, a) = (elev.to_radians(), azim.to_radians());
    Vec3::new(a.sin() * e.cos(), e.sin(), -a.cos() * e.cos())
}

/// Sky, sun/moon and global ambient for `time`.
pub fn spawn_sky(commands: &mut Commands, media: &mut Assets<ScatteringMedium>, time: TimeOfDay) {
    commands.spawn(Atmosphere::earth(
        media.add(ScatteringMedium::earth(256, 256)),
    ));
    let dir = sun_direction(time);
    let (illuminance, color, shadows) = match time {
        TimeOfDay::Day => (lux::RAW_SUNLIGHT, Color::WHITE, true),
        TimeOfDay::Golden => (lux::RAW_SUNLIGHT * 0.75, Color::srgb(1.0, 0.8, 0.58), true),
        // Moonlight (a bright full moon is ~0.3 lux; games push it for
        // readability), cool; the atmosphere then renders a dark sky.
        TimeOfDay::Night => (2.0, Color::srgb(0.7, 0.8, 1.0), false),
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
        TimeOfDay::Night => 3.1,
    };
    let grading = match time {
        TimeOfDay::Day => ColorGrading {
            global: ColorGradingGlobal {
                post_saturation: 1.05,
                ..default()
            },
            ..default()
        },
        TimeOfDay::Golden => ColorGrading {
            global: ColorGradingGlobal {
                temperature: 0.1,
                post_saturation: 1.15,
                ..default()
            },
            shadows: ColorGradingSection {
                saturation: 1.05,
                ..default()
            },
            ..default()
        },
        // Teal shadows, warm highlights (night street look).
        TimeOfDay::Night => ColorGrading {
            global: ColorGradingGlobal {
                post_saturation: 0.95,
                temperature: -0.04,
                ..default()
            },
            shadows: ColorGradingSection {
                saturation: 1.05,
                contrast: 1.08,
                ..default()
            },
            ..default()
        },
    };
    let fog = match time {
        // Fog colour is display-relative (applied after exposure); it matches
        // the horizon of the light-pollution dome.
        TimeOfDay::Night => DistanceFog {
            color: Color::srgb(0.045, 0.04, 0.042),
            falloff: FogFalloff::Exponential {
                density: if wet { 0.006 } else { 0.004 },
            },
            ..default()
        },
        _ => DistanceFog {
            color: Color::srgba(0.6, 0.65, 0.72, 0.0),
            falloff: FogFalloff::Linear {
                start: 5000.0,
                end: 6000.0,
            },
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
        Bloom {
            intensity: if time == TimeOfDay::Night { 0.22 } else { 0.08 },
            ..Bloom::NATURAL
        },
        grading,
        fog,
        Projection::Perspective(PerspectiveProjection {
            fov: 55f32.to_radians(),
            near: 0.1,
            far: 3000.0,
            ..default()
        }),
    )
}

/// Night sky glow: an emissive dome (not fogged) with the orange-grey
/// light-pollution gradient of a city sky, brightest at the horizon. The
/// physical atmosphere renders a black sky at night, which reads as a void.
pub fn spawn_night_dome(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    use crate::{
        mesh::MeshData,
        texture::{Encoding, Tex},
    };
    let gradient = Tex::from_fn(64, |_, v| {
        // v = 0 at the zenith, 1 at the horizon (and below).
        let t = v.clamp(0.0, 1.0).powf(3.0);
        let zenith = Vec3::new(0.10, 0.13, 0.22);
        let horizon = Vec3::new(1.0, 0.72, 0.5);
        let c = zenith.lerp(horizon, t);
        [c.x, c.y, c.z, 1.0]
    });
    let mut dome = MeshData::new();
    let (rings, seg, r) = (24, 48, 2800.0);
    for i in 0..=rings {
        let el = std::f32::consts::FRAC_PI_2 * (1.0 - i as f32 / rings as f32)
            - 0.15 * (i == rings) as i32 as f32;
        for j in 0..=seg {
            let az = j as f32 / seg as f32 * std::f32::consts::TAU;
            let d = Vec3::new(el.cos() * az.cos(), el.sin(), el.cos() * az.sin());
            dome.vertex(
                d * r,
                -d,
                Vec2::new(j as f32 / seg as f32, i as f32 / rings as f32),
            );
        }
    }
    for i in 0..rings as u32 {
        for j in 0..seg as u32 {
            let a = i * (seg as u32 + 1) + j;
            let b = a + seg as u32 + 1;
            dome.tri(a, b, b + 1);
            dome.tri(a, b + 1, a + 1);
        }
    }
    let tex = images.add(gradient.to_image(Encoding::Srgb));
    commands.spawn((
        Mesh3d(meshes.add(dome.to_bevy())),
        MeshMaterial3d(materials.add(StandardMaterial {
            // Always forward: the dome must not go through the deferred
            // G-buffer (which would light and fog it).
            opaque_render_method: bevy::material::OpaqueRendererMethod::Forward,
            base_color: Color::BLACK,
            emissive: LinearRgba::gray(0.35),
            emissive_texture: Some(tex),
            emissive_exposure_weight: 1.0,
            fog_enabled: false,
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            ..default()
        })),
        bevy::light::NotShadowCaster,
        bevy::light::NotShadowReceiver,
    ));
}

/// Adds screen-space reflections (and TAA, since deferred shading cannot use
/// MSAA and a wet road's specular highlights alias badly without temporal
/// filtering) to every 3D camera. Used by the wet presets.
pub fn enable_reflections(mut commands: Commands, cameras: Query<Entity, With<Camera3d>>) {
    use bevy::{anti_alias::taa::TemporalAntiAliasing, pbr::ScreenSpaceReflections};
    for cam in &cameras {
        commands.entity(cam).insert((
            ScreenSpaceReflections {
                min_perceptual_roughness: 0.0..0.01,
                max_perceptual_roughness: 0.32..0.45,
                thickness: 0.4,
                linear_steps: 24,
                linear_march_exponent: 1.0,
                edge_fadeout: 0.0..0.05,
                bisection_steps: 6,
                use_secant: true,
            },
            Msaa::Off,
            TemporalAntiAliasing::default(),
        ));
    }
}
