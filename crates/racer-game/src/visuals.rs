//! Greybox visuals shared by the driving and race scenes: meshes for track
//! pieces and a painted box body with four turning wheels for every car.

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use racer_sim::{
    track::{PieceKind, Shape, TrackPiece},
    vehicle::{Car, CarTelemetry, wheel_center},
};

pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(crate::car_model::CarModelPlugin)
            .add_systems(Update, (mesh_track_pieces, mesh_cars))
            .add_systems(
                PostUpdate,
                (animate_wheels, reverse_lights).before(TransformSystems::Propagate),
            );
    }
}

/// Body colour of a car; cars without it are not drawn.
#[derive(Component, Clone, Copy, Debug)]
pub struct CarPaint(pub Color);

/// Body colours for grids, in slot order.
pub const PAINT: [Color; 8] = [
    Color::srgb(0.75, 0.05, 0.04),
    Color::srgb(0.05, 0.35, 0.18),
    Color::srgb(0.95, 0.75, 0.05),
    Color::srgb(0.08, 0.2, 0.65),
    Color::srgb(0.9, 0.9, 0.92),
    Color::srgb(0.95, 0.4, 0.05),
    Color::srgb(0.45, 0.1, 0.55),
    Color::srgb(0.08, 0.08, 0.09),
];

/// Visual wheel `index` of its parent car, with its accumulated spin angle.
#[derive(Component)]
struct WheelVisual {
    index: usize,
    spin: f32,
}

/// A greybox body's reverse light, lit while its car is in reverse.
#[derive(Component)]
struct ReverseLight;

/// Triangle mesh for rendering, with smooth normals.
pub fn render_mesh(vertices: Vec<Vec3>, indices: Vec<u32>) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vertices)
    .with_inserted_indices(Indices::U32(indices))
    .with_computed_smooth_normals()
}

/// Adds greybox meshes to new track pieces.
fn mesh_track_pieces(
    mut commands: Commands,
    pieces: Query<(Entity, &TrackPiece), Added<TrackPiece>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<Vec<(PieceKind, Handle<StandardMaterial>)>>,
) {
    for (entity, piece) in &pieces {
        let material = match cache.iter().find(|(k, _)| *k == piece.kind) {
            Some((_, m)) => m.clone(),
            None => {
                let (color, roughness) = match piece.kind {
                    PieceKind::Grass => (Color::srgb(0.16, 0.24, 0.12), 0.95),
                    PieceKind::Road => (Color::srgb(0.22, 0.22, 0.23), 0.9),
                    PieceKind::Apron => (Color::srgb(0.42, 0.41, 0.39), 0.9),
                    PieceKind::Wall => (Color::srgb(0.62, 0.6, 0.56), 0.85),
                    PieceKind::Building => (Color::srgb(0.5, 0.52, 0.56), 0.8),
                    PieceKind::Curb(true) => (Color::srgb(0.7, 0.08, 0.06), 0.7),
                    PieceKind::Curb(false) | PieceKind::Marking => {
                        (Color::srgb(0.85, 0.85, 0.8), 0.7)
                    }
                };
                let m = materials.add(StandardMaterial {
                    base_color: color,
                    perceptual_roughness: roughness,
                    ..default()
                });
                cache.push((piece.kind, m.clone()));
                m
            }
        };
        let mesh = match &piece.shape {
            Shape::Box(size) => Cuboid::from_size(*size).into(),
            Shape::Mesh(m) => render_mesh(
                m.vertices.clone(),
                m.indices.iter().flatten().copied().collect(),
            ),
        };
        commands
            .entity(entity)
            .insert((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material)));
    }
}

/// Gives new painted cars their pipeline model if built, else a greybox body
/// (lower body, glasshouse) and wheels.
fn mesh_cars(
    mut commands: Commands,
    cars: Query<(Entity, &Car, &CarPaint), Added<CarPaint>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    for (entity, car, paint) in &cars {
        if let Some(path) = crate::car_model::model_path(&car.id) {
            let cg = car.vehicle.ride_height() as f32;
            crate::car_model::spawn_model(&mut commands, &asset_server, entity, path, cg);
            continue;
        }
        let Ok(spec) = racer_dynamics::load_car(&car.id) else {
            continue;
        };
        let c = &spec.chassis;
        let (len, wid, hgt) = (c.length_m as f32, c.width_m as f32, c.height_m as f32);
        let ride = car.vehicle.ride_height() as f32;
        let paint = materials.add(racer_render::car_paint(paint.0));
        let glass = materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.06, 0.08),
            perceptual_roughness: 0.1,
            ..default()
        });
        let tire = materials.add(StandardMaterial {
            base_color: Color::srgb(0.03, 0.03, 0.03),
            perceptual_roughness: 0.8,
            ..default()
        });
        let reverse_light = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            emissive: LinearRgba::rgb(4.0, 4.0, 4.0),
            unlit: true,
            ..default()
        });
        // Lower body above the wheel bottoms and narrower than the wheels, so they show.
        let (sill, belt) = (0.3, 0.55 * hgt);
        commands
            .entity(entity)
            .insert(Visibility::default())
            .with_children(|p| {
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.88 * wid, belt - sill, len))),
                    MeshMaterial3d(paint.clone()),
                    Transform::from_xyz(0.0, (sill + belt) / 2.0 - ride, 0.0),
                ));
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.8 * wid, hgt - belt, 0.5 * len))),
                    MeshMaterial3d(glass),
                    Transform::from_xyz(0.0, (belt + hgt) / 2.0 - ride, 0.08 * len),
                ));
                // Reverse lights in the tail (+Z is the back of the car).
                for side in [-1.0, 1.0] {
                    p.spawn((
                        ReverseLight,
                        Mesh3d(meshes.add(Cuboid::new(0.16 * wid, 0.06, 0.02))),
                        MeshMaterial3d(reverse_light.clone()),
                        Transform::from_xyz(side * 0.28 * wid, belt - 0.08 - ride, len / 2.0),
                        Visibility::Hidden,
                    ));
                }
                for index in 0..4 {
                    let r = car.vehicle.wheel_geometry(index).radius as f32;
                    p.spawn((
                        WheelVisual { index, spin: 0.0 },
                        Mesh3d(meshes.add(Cylinder::new(r, 0.24))),
                        MeshMaterial3d(tire.clone()),
                        Transform::default(),
                    ));
                }
            });
    }
}

fn reverse_lights(
    cars: Query<(&CarTelemetry, &Children)>,
    mut lights: Query<&mut Visibility, With<ReverseLight>>,
) {
    for (telemetry, children) in &cars {
        let on = if telemetry.telemetry.gear < 0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for child in children.iter() {
            if let Ok(mut v) = lights.get_mut(child) {
                v.set_if_neq(on);
            }
        }
    }
}

fn animate_wheels(
    time: Res<Time>,
    cars: Query<(&Car, &CarTelemetry, &Children)>,
    mut wheels: Query<(&mut WheelVisual, &mut Transform)>,
) {
    for (car, telemetry, children) in &cars {
        for child in children.iter() {
            let Ok((mut wheel, mut transform)) = wheels.get_mut(child) else {
                continue;
            };
            let w = &telemetry.telemetry.wheels[wheel.index];
            wheel.spin = (wheel.spin - w.omega as f32 * time.delta_secs()) % std::f32::consts::TAU;
            let g = car.vehicle.wheel_geometry(wheel.index);
            *transform = Transform::from_translation(wheel_center(&g, w.travel)).with_rotation(
                Quat::from_rotation_y(w.steer as f32)
                    * Quat::from_rotation_x(wheel.spin)
                    * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            );
        }
    }
}
