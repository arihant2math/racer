//! Car models from the asset pipeline (`assets/cars/<id>/<model>.glb`).
//!
//! A car whose model file exists gets the glTF scene instead of the greybox
//! body. Once the scene is spawned it is wired up:
//!
//! - node `wheel_XX` (rim + tyre) spins and steers, `brake_XX_mount` (disc +
//!   caliper) steers only; both follow suspension travel,
//! - the `car_paint` material is replaced by the car's own paint colour,
//! - `*_LOD1..3` meshes get [`VisibilityRange`]s, the collision hull and
//!   socket empties are hidden,
//! - the model is placed in the car's body frame (the sim's origin is the
//!   centre of mass; the model's is the ground under the wheelbase midpoint).

use std::path::PathBuf;

use bevy::{
    camera::visibility::VisibilityRange, gltf::GltfMaterialName, prelude::*,
    world_serialization::WorldInstanceReady,
};
use racer_sim::vehicle::{Car, CarTelemetry, wheel_center};

use crate::visuals::CarPaint;

/// Models shipped by the asset pipeline, by car id.
const MODELS: &[(&str, &str)] = &[("porsche-911-carrera-s-992", "porsche_992.glb")];

/// Wheel node names in the sim's wheel order (FL, FR, RL, RR).
const WHEELS: [&str; 4] = ["FL", "FR", "RL", "RR"];

/// LOD switch distances (m): LOD0 below the first, LOD3 beyond the last.
const LOD_DISTANCES: [f32; 3] = [18.0, 45.0, 110.0];

/// The `assets/` directory (see `racer_data::assets_dir` for where it is
/// found); Bevy's asset root is set to it in the game binary. Absolute,
/// because Bevy resolves a relative root against the executable's directory,
/// not the working directory.
pub fn assets_dir() -> PathBuf {
    let dir = racer_data::assets_dir();
    std::path::absolute(&dir).unwrap_or(dir)
}

/// Asset path of the car's model, if the pipeline has built it.
pub fn model_path(car_id: &str) -> Option<String> {
    let (_, file) = MODELS.iter().find(|(id, _)| *id == car_id)?;
    let rel = format!("cars/{car_id}/{file}");
    assets_dir().join(&rel).is_file().then_some(rel)
}

/// Marks the root of a car's glTF scene; `car` is the car entity.
#[derive(Component)]
pub struct CarModel {
    pub car: Entity,
}

/// A wheel node in a car model: `spin` true for rim/tyre, false for brake mounts.
#[derive(Component)]
pub struct ModelWheel {
    pub index: usize,
    pub spin: bool,
    pub angle: f32,
    /// Hub centre in the model frame (rest pose).
    pub rest: Vec3,
}

pub struct CarModelPlugin;

impl Plugin for CarModelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PreloadedModels>()
            .add_systems(Startup, preload_models)
            .add_systems(
                PostUpdate,
                animate_model_wheels.before(TransformSystems::Propagate),
            );
    }
}

/// Every built car model, requested at startup and kept alive so spawning a
/// car later reuses the loaded scene instead of parsing the GLB again.
#[derive(Resource, Default)]
pub struct PreloadedModels(pub Vec<Handle<WorldAsset>>);

impl PreloadedModels {
    /// True once every model (with its meshes and textures) has loaded or failed.
    pub fn ready(&self, asset_server: &AssetServer) -> bool {
        self.0.iter().all(|h| {
            asset_server.is_loaded_with_dependencies(h.id())
                || asset_server.load_state(h.id()).is_failed()
        })
    }
}

fn preload_models(asset_server: Res<AssetServer>, mut models: ResMut<PreloadedModels>) {
    models.0 = MODELS
        .iter()
        .filter_map(|(id, _)| model_path(id))
        .map(|path| asset_server.load(GltfAssetLabel::Scene(0).from_asset(path)))
        .collect();
}

/// Run condition: car models are loaded.
pub fn models_ready(asset_server: Res<AssetServer>, models: Res<PreloadedModels>) -> bool {
    models.ready(&asset_server)
}

/// Spawns the model under `car` (call instead of building the greybox body).
/// `cg_height` is the centre of mass above the ground at ride height.
pub fn spawn_model(
    commands: &mut Commands,
    asset_server: &AssetServer,
    car: Entity,
    path: String,
    cg_height: f32,
) {
    let scene = asset_server.load(GltfAssetLabel::Scene(0).from_asset(path));
    commands
        .entity(car)
        .insert(Visibility::default())
        .with_children(|p| {
            p.spawn((
                CarModel { car },
                WorldAssetRoot(scene),
                Transform::from_xyz(0.0, -cg_height, 0.0),
            ))
            .observe(on_model_ready);
        });
}

#[allow(clippy::too_many_arguments)]
fn on_model_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    models: Query<&CarModel>,
    cars: Query<(&Car, Option<&CarPaint>)>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    names: Query<(&Name, &Transform)>,
    mesh_materials: Query<(&MeshMaterial3d<StandardMaterial>, Option<&GltfMaterialName>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let root = ready.event().entity;
    let Ok(model) = models.get(root) else {
        return;
    };
    let Ok((car, paint)) = cars.get(model.car) else {
        return;
    };
    // The model is centred on the wheelbase; the sim body frame on the centre
    // of mass, which sits behind the midpoint for a rear-heavy car.
    let front_z = car.vehicle.wheel_geometry(0).hardpoint.z as f32;
    let rear_z = car.vehicle.wheel_geometry(2).hardpoint.z as f32;
    let shift = 0.5 * (front_z + rear_z);
    commands
        .entity(root)
        .entry::<Transform>()
        .and_modify(move |mut t| t.translation.z = shift);

    let paint_material = paint.map(|p| {
        let mut m = racer_render::car_paint(p.0);
        m.perceptual_roughness = 0.3;
        materials.add(m)
    });

    for entity in children.iter_descendants(root) {
        if let Ok((name, transform)) = names.get(entity) {
            let name = name.as_str();
            for (index, key) in WHEELS.iter().enumerate() {
                let spin = name == format!("wheel_{key}");
                if spin || name == format!("brake_{key}_mount") {
                    commands.entity(entity).insert(ModelWheel {
                        index,
                        spin,
                        angle: 0.0,
                        rest: transform.translation,
                    });
                }
            }
            if name.starts_with("COL_") || name.starts_with("socket_") {
                commands.entity(entity).insert(Visibility::Hidden);
            }
        }
        // Mesh primitives: LOD from the suffix of their node (the parent) or their own name.
        if mesh_materials.get(entity).is_ok() {
            let lod_of = |e: Entity| {
                names
                    .get(e)
                    .ok()
                    .and_then(|(n, _)| (1..=3).find(|k| n.as_str().ends_with(&format!("_LOD{k}"))))
            };
            let lod = lod_of(entity)
                .or_else(|| parents.get(entity).ok().and_then(|p| lod_of(p.parent())))
                .unwrap_or(0);
            let start = if lod == 0 {
                0.0
            } else {
                LOD_DISTANCES[lod - 1]
            };
            let end = if lod == 3 {
                f32::MAX
            } else {
                LOD_DISTANCES[lod]
            };
            commands.entity(entity).insert(VisibilityRange {
                start_margin: start..start,
                end_margin: end..end,
                use_aabb: false,
            });
        }
        if let (Some(paint), Ok((_, Some(mat_name)))) =
            (&paint_material, mesh_materials.get(entity))
            && mat_name.0 == "car_paint"
        {
            commands
                .entity(entity)
                .insert(MeshMaterial3d(paint.clone()));
        }
    }
}

/// Spins and steers model wheels from the car's telemetry.
fn animate_model_wheels(
    time: Res<Time>,
    models: Query<(&CarModel, &Transform), Without<ModelWheel>>,
    cars: Query<(&Car, &CarTelemetry)>,
    parents: Query<&ChildOf>,
    mut wheels: Query<(Entity, &mut ModelWheel, &mut Transform)>,
) {
    for (entity, mut wheel, mut transform) in &mut wheels {
        let Some((model, model_tf)) = parents
            .iter_ancestors(entity)
            .find_map(|a| models.get(a).ok())
        else {
            continue;
        };
        let Ok((car, telemetry)) = cars.get(model.car) else {
            continue;
        };
        let w = &telemetry.telemetry.wheels[wheel.index];
        let g = car.vehicle.wheel_geometry(wheel.index);
        // Suspension: the sim's wheel centre in the body frame, mapped into the model frame.
        let hub = wheel_center(&g, w.travel) - model_tf.translation;
        let rest = wheel.rest;
        let translation = Vec3::new(rest.x, hub.y, rest.z);
        let mut rotation = Quat::from_rotation_y(w.steer as f32);
        if wheel.spin {
            wheel.angle =
                (wheel.angle - w.omega as f32 * time.delta_secs()) % std::f32::consts::TAU;
            rotation *= Quat::from_rotation_x(wheel.angle);
        }
        *transform = Transform::from_translation(translation).with_rotation(rotation);
    }
}
