//! Race HUD: position and lap, lap/sector times and the delta to the best
//! lap, speed, gear and rpm, a minimap with every car, the start countdown
//! and warnings (wrong way, reset).

use bevy::prelude::*;
use racer_sim::{
    race::{Race, RaceConfig, RaceMode, RacePhase, RaceTrack, Racer},
    rules::{RESET_AFTER, Status},
    vehicle::{Car, CarTelemetry},
};

use crate::{
    flow::{AppState, lap_time},
    input::Player,
    visuals::CarPaint,
};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Race), spawn_hud)
            .add_systems(
                Update,
                (update_hud, update_minimap).run_if(in_state(AppState::Race)),
            );
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum HudText {
    Position,
    Lap,
    Times,
    Delta,
    Speed,
    Gear,
    Center,
    Warning,
}

#[derive(Component)]
struct RpmBar;

#[derive(Component)]
struct Minimap;

/// A car's dot on the minimap.
#[derive(Component)]
struct MinimapDot(Entity);

const PANEL: Color = Color::srgba(0.0, 0.0, 0.0, 0.5);
const MAP_SIZE: f32 = 210.0;

fn text(kind: HudText, size: f32) -> impl Bundle {
    (
        kind,
        Text::new(""),
        TextFont::from_font_size(size),
        TextColor(Color::WHITE),
    )
}

fn panel(node: Node) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(10)),
            ..node
        },
        BackgroundColor(PANEL),
    )
}

fn spawn_hud(mut commands: Commands, track: Res<RaceTrack>) {
    let root = (
        DespawnOnExit(AppState::Race),
        Node {
            width: percent(100),
            height: percent(100),
            ..default()
        },
    );
    commands.spawn(root).with_children(|root| {
        root.spawn(panel(Node {
            top: px(12),
            left: px(12),
            ..default()
        }))
        .with_children(|p| {
            p.spawn(text(HudText::Position, 44.0));
            p.spawn(text(HudText::Lap, 22.0));
        });
        root.spawn(panel(Node {
            top: px(12),
            right: px(12),
            min_width: px(250),
            ..default()
        }))
        .with_children(|p| {
            p.spawn(text(HudText::Times, 18.0));
            p.spawn(text(HudText::Delta, 26.0));
        });
        root.spawn(panel(Node {
            bottom: px(12),
            right: px(12),
            min_width: px(230),
            ..default()
        }))
        .with_children(|p| {
            p.spawn(text(HudText::Speed, 44.0));
            p.spawn(text(HudText::Gear, 20.0));
            p.spawn((
                Node {
                    width: percent(100),
                    height: px(10),
                    margin: UiRect::top(px(6)),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.15)),
            ))
            .with_child((
                RpmBar,
                Node {
                    width: percent(0),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.95, 0.55, 0.1)),
            ));
        });
        // Centre: countdown and warnings.
        root.spawn(Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            top: percent(28),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn(text(HudText::Center, 96.0));
            p.spawn((
                HudText::Warning,
                Text::new(""),
                TextFont::from_font_size(40.0),
                TextColor(Color::srgb(1.0, 0.2, 0.15)),
            ));
        });
        // Minimap: the centreline as dots, cars on top.
        root.spawn((
            Minimap,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(12),
                left: px(12),
                width: px(MAP_SIZE + 20.0),
                height: px(MAP_SIZE + 20.0),
                ..default()
            },
            BackgroundColor(PANEL),
        ))
        .with_children(|map| {
            let g = &track.0.geometry;
            for s in g.samples.iter().step_by(6) {
                let p = map_point(g.bounds, s.pos);
                map.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(p.x - 1.5),
                        top: px(p.y - 1.5),
                        width: px(3),
                        height: px(3),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.85, 0.85, 0.85, 0.8)),
                ));
            }
        });
    });
}

/// Minimap pixel position of a world point (north up).
fn map_point(bounds: (Vec2, Vec2), p: Vec3) -> Vec2 {
    let (lo, hi) = bounds;
    let span = (hi - lo).max_element().max(1.0);
    let offset = (Vec2::splat(span) - (hi - lo)) / 2.0;
    (Vec2::new(p.x, p.z) - lo + offset) / span * MAP_SIZE + Vec2::splat(10.0)
}

fn update_minimap(
    mut commands: Commands,
    track: Res<RaceTrack>,
    maps: Query<Entity, With<Minimap>>,
    cars: Query<(Entity, &Transform, &CarPaint, Has<Player>), With<Racer>>,
    mut dots: Query<(&MinimapDot, &mut Node)>,
) {
    let Ok(map) = maps.single() else { return };
    let bounds = track.0.geometry.bounds;
    for (entity, transform, paint, player) in &cars {
        let p = map_point(bounds, transform.translation);
        let size = if player { 11.0 } else { 8.0 };
        if let Some((_, mut node)) = dots.iter_mut().find(|(d, _)| d.0 == entity) {
            node.left = px(p.x - size / 2.0);
            node.top = px(p.y - size / 2.0);
        } else {
            let dot = commands
                .spawn((
                    MinimapDot(entity),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(p.x - size / 2.0),
                        top: px(p.y - size / 2.0),
                        width: px(size),
                        height: px(size),
                        border: UiRect::all(px(if player { 2.0 } else { 1.0 })),
                        ..default()
                    },
                    BackgroundColor(paint.0),
                    BorderColor::all(if player { Color::WHITE } else { Color::BLACK }),
                ))
                .id();
            commands.entity(map).add_child(dot);
        }
    }
}

fn update_hud(
    race: Option<Res<Race>>,
    config: Option<Res<RaceConfig>>,
    track: Res<RaceTrack>,
    racers: Query<&Racer>,
    players: Query<(&Racer, &Car, &CarTelemetry), With<Player>>,
    mut texts: Query<(&HudText, &mut Text, &mut TextColor)>,
    mut bars: Query<&mut Node, With<RpmBar>>,
) {
    let (Some(race), Some(config)) = (race, config) else {
        return;
    };
    let Ok((me, car, tel)) = players.single() else {
        return;
    };
    let g = &track.0.geometry;
    let count = racers.iter().count();
    let now = race.time.max(0.0);
    let t = &me.timing;
    let lap_now = match me.status {
        Status::Finished(_) | Status::Dnf => None,
        Status::Racing if race.phase == RacePhase::Running => Some(t.current_lap(now)),
        Status::Racing => None,
    };
    let sectors: Vec<String> = (0..=g.sector_ends.len())
        .map(|i| {
            let current = t.sectors.get(i).or(if t.sectors.is_empty() {
                t.last_sectors.get(i)
            } else {
                None
            });
            format!(
                "S{} {}",
                i + 1,
                current.map_or("--.---".into(), |s| format!("{s:.3}"))
            )
        })
        .collect();
    for (kind, mut text, mut color) in &mut texts {
        let value = match kind {
            HudText::Position => {
                if config.mode == RaceMode::TimeTrial {
                    "Time trial".into()
                } else {
                    format!("P{} / {count}", me.position)
                }
            }
            HudText::Lap => match config.mode {
                RaceMode::Sprint => {
                    let done = (me.progress.distance / g.sprint_finish).clamp(0.0, 1.0);
                    format!("Sprint {:.0} %", done * 100.0)
                }
                _ => format!(
                    "Lap {} / {}",
                    (me.progress.laps + 1).min(config.laps),
                    config.laps
                ),
            },
            HudText::Times => format!(
                "Lap   {}\nLast  {}\nBest  {}\n{}",
                lap_now.map_or("-:--.---".into(), lap_time),
                t.laps.last().map_or("-:--.---".into(), |x| lap_time(*x)),
                t.best_lap.map_or("-:--.---".into(), lap_time),
                sectors.join("  ")
            ),
            HudText::Delta => {
                color.0 = match t.delta {
                    Some(d) if d < 0.0 => Color::srgb(0.3, 1.0, 0.35),
                    Some(_) => Color::srgb(1.0, 0.35, 0.3),
                    None => Color::WHITE,
                };
                t.delta
                    .map_or("Delta  --".into(), |d| format!("Delta {d:+.2}"))
            }
            HudText::Speed => format!("{:3.0} km/h", tel.telemetry.speed.abs() * 3.6),
            HudText::Gear => {
                let gear = match tel.telemetry.gear {
                    g if g < 0 => "R".to_string(),
                    0 => "N".to_string(),
                    g => g.to_string(),
                };
                format!("Gear {gear}   {:5.0} rpm", tel.telemetry.rpm)
            }
            HudText::Center => match (race.phase, me.status) {
                (RacePhase::Countdown, _) => format!("{}", race.time.abs().ceil() as i32),
                (RacePhase::Running, _) if race.time < 1.0 => "GO!".into(),
                (_, Status::Finished(_)) => format!("Finished  P{}", me.position),
                (_, Status::Dnf) => "DNF".into(),
                _ => String::new(),
            },
            HudText::Warning => {
                if me.progress.wrong_way {
                    "WRONG WAY".into()
                } else if !me.progress.on_track {
                    format!("Off track: reset in {RESET_AFTER:.0} s")
                } else if me.ghosted && race.phase == RacePhase::Running && me.ghost > 0.0 {
                    "Reset".into()
                } else {
                    String::new()
                }
            }
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    let limiter = car.vehicle.drivetrain.engine.limiter / racer_dynamics::drivetrain::RPM_TO_RAD_S;
    for mut node in &mut bars {
        node.width = percent((tel.telemetry.rpm / limiter * 100.0).clamp(0.0, 100.0) as f32);
    }
}
