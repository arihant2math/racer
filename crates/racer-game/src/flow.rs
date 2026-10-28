//! Game flow: main menu, race setup, race, pause and results, as app
//! states with greybox bevy_ui menus driven by keyboard, gamepad or mouse.

use bevy::{
    input::gamepad::{Gamepad, GamepadConnectionEvent},
    prelude::*,
};
use racer_dynamics::Assists;
use racer_sim::{
    ai::Difficulty,
    race::{Collisions, Gap, RaceMode, Racer},
    rules::Status,
    vehicle::Car,
};

use crate::race_scene::{self, DriverAids, RaceEntity};

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    MainMenu,
    Setup,
    /// One frame between races: clears the old one so a restart re-enters `Race`.
    Loading,
    Race,
    Results,
}

/// The menus and race flow (the race scene itself is in [`race_scene`]).
pub struct FlowPlugin;

impl Plugin for FlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .init_resource::<RaceSetup>()
            .init_resource::<MenuCursor>()
            .init_resource::<Paused>()
            .add_systems(
                OnEnter(AppState::MainMenu),
                (race_scene::clear_race, spawn_main_menu),
            )
            .add_systems(
                OnEnter(AppState::Setup),
                (race_scene::clear_race, spawn_setup_menu),
            )
            .add_systems(
                OnEnter(AppState::Loading),
                (race_scene::clear_race, go_to_race).chain(),
            )
            .add_systems(OnEnter(AppState::Results), spawn_results)
            // Nothing outside a race is ever paused, however the race was left.
            .add_systems(OnExit(AppState::Race), resume_time)
            .add_systems(
                Update,
                (
                    toggle_pause.run_if(in_state(AppState::Race)),
                    menu_input,
                    draw_menu,
                    update_results.run_if(in_state(AppState::Results)),
                    fit_to_window,
                )
                    .chain(),
            );
    }
}

/// Opponent cars: all the player's model, or a mix of every model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpponentCars {
    Same,
    Mixed,
}

/// Everything chosen on the setup screen.
#[derive(Resource, Clone, Debug)]
pub struct RaceSetup {
    pub mode: RaceMode,
    /// Index into `racer_dynamics::spec::CARS`.
    pub car: usize,
    pub opponents: usize,
    pub opponent_cars: OpponentCars,
    /// `None` mixes all five difficulties.
    pub difficulty: Option<Difficulty>,
    pub laps: u32,
    pub collisions: Collisions,
    pub catch_up: bool,
    pub abs: bool,
    pub tcs: bool,
    pub esc: bool,
    pub steering_assist: bool,
    pub racing_line: bool,
    pub automatic: bool,
    /// Let the AI drive the player's car (demos and captures).
    pub autopilot: bool,
}

impl Default for RaceSetup {
    fn default() -> Self {
        RaceSetup {
            mode: RaceMode::Circuit,
            car: 0,
            opponents: 7,
            opponent_cars: OpponentCars::Same,
            difficulty: Some(Difficulty::Pro),
            laps: 3,
            collisions: Collisions::Full,
            catch_up: false,
            abs: true,
            tcs: true,
            esc: true,
            steering_assist: true,
            racing_line: false,
            automatic: true,
            autopilot: false,
        }
    }
}

/// Settings on the setup screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Mode,
    Track,
    Car,
    Opponents,
    OpponentCars,
    Difficulty,
    Laps,
    Collisions,
    CatchUp,
    Abs,
    Tcs,
    Esc,
    SteeringAssist,
    RacingLine,
    Gearbox,
}

fn on_off(on: bool) -> String {
    if on { "On".into() } else { "Off".into() }
}

fn cycle<T: Copy + PartialEq>(all: &[T], current: T, dir: i32) -> T {
    let i = all.iter().position(|x| *x == current).unwrap_or(0) as i32;
    all[(i + dir).rem_euclid(all.len() as i32) as usize]
}

impl Setting {
    pub const ALL: [Setting; 15] = [
        Setting::Mode,
        Setting::Track,
        Setting::Car,
        Setting::Opponents,
        Setting::OpponentCars,
        Setting::Difficulty,
        Setting::Laps,
        Setting::Collisions,
        Setting::CatchUp,
        Setting::Abs,
        Setting::Tcs,
        Setting::Esc,
        Setting::SteeringAssist,
        Setting::RacingLine,
        Setting::Gearbox,
    ];

    fn label(self) -> &'static str {
        match self {
            Setting::Mode => "Mode",
            Setting::Track => "Track",
            Setting::Car => "Car",
            Setting::Opponents => "AI opponents",
            Setting::OpponentCars => "AI cars",
            Setting::Difficulty => "AI difficulty",
            Setting::Laps => "Laps",
            Setting::Collisions => "Collisions",
            Setting::CatchUp => "Catch-up",
            Setting::Abs => "ABS",
            Setting::Tcs => "Traction control",
            Setting::Esc => "Stability control",
            Setting::SteeringAssist => "Steering assist",
            Setting::RacingLine => "Racing line",
            Setting::Gearbox => "Gearbox",
        }
    }

    fn value(self, s: &RaceSetup) -> String {
        match self {
            Setting::Mode => s.mode.name().into(),
            Setting::Track => "Downtown Loop".into(),
            Setting::Car => race_scene::car_name(s.car),
            Setting::Opponents if s.mode == RaceMode::TimeTrial => "none (time trial)".into(),
            Setting::Opponents => s.opponents.to_string(),
            Setting::OpponentCars => match s.opponent_cars {
                OpponentCars::Same => "Same as yours".into(),
                OpponentCars::Mixed => "Mixed".into(),
            },
            Setting::Difficulty => s.difficulty.map_or("Mixed".into(), |d| d.name().into()),
            Setting::Laps if s.mode == RaceMode::Sprint => "one run".into(),
            Setting::Laps => s.laps.to_string(),
            Setting::Collisions => s.collisions.name().into(),
            Setting::CatchUp => on_off(s.catch_up),
            Setting::Abs => on_off(s.abs),
            Setting::Tcs => on_off(s.tcs),
            Setting::Esc => on_off(s.esc),
            Setting::SteeringAssist => on_off(s.steering_assist),
            Setting::RacingLine => on_off(s.racing_line),
            Setting::Gearbox => if s.automatic { "Automatic" } else { "Manual" }.into(),
        }
    }

    fn change(self, s: &mut RaceSetup, dir: i32) {
        match self {
            Setting::Mode => s.mode = cycle(&RaceMode::ALL, s.mode, dir),
            Setting::Track => {}
            Setting::Car => {
                s.car = (s.car as i32 + dir).rem_euclid(racer_dynamics::spec::CARS.len() as i32)
                    as usize
            }
            Setting::Opponents => s.opponents = (s.opponents as i32 + dir).rem_euclid(8) as usize,
            Setting::OpponentCars => {
                s.opponent_cars = cycle(
                    &[OpponentCars::Same, OpponentCars::Mixed],
                    s.opponent_cars,
                    dir,
                )
            }
            Setting::Difficulty => {
                let all = [
                    None,
                    Some(Difficulty::Novice),
                    Some(Difficulty::Amateur),
                    Some(Difficulty::Pro),
                    Some(Difficulty::Expert),
                    Some(Difficulty::Alien),
                ];
                s.difficulty = cycle(&all, s.difficulty, dir);
            }
            Setting::Laps => s.laps = ((s.laps as i32 - 1 + dir).rem_euclid(10) + 1) as u32,
            Setting::Collisions => s.collisions = cycle(&Collisions::ALL, s.collisions, dir),
            Setting::CatchUp => s.catch_up = !s.catch_up,
            Setting::Abs => s.abs = !s.abs,
            Setting::Tcs => s.tcs = !s.tcs,
            Setting::Esc => s.esc = !s.esc,
            Setting::SteeringAssist => s.steering_assist = !s.steering_assist,
            Setting::RacingLine => s.racing_line = !s.racing_line,
            Setting::Gearbox => s.automatic = !s.automatic,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Race,
    Quit,
    Start,
    Back,
    Resume,
    Restart,
    Menu,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::Race => "Race",
            Action::Quit => "Quit",
            Action::Start => "Start race",
            Action::Back => "Back",
            Action::Resume => "Resume",
            Action::Restart => "Restart race",
            Action::Menu => "Main menu",
        }
    }
}

/// A selectable menu row, in display order.
#[derive(Component, Clone, Copy, Debug)]
pub enum MenuItem {
    Setting(Setting),
    Action(Action),
}

#[derive(Component)]
struct MenuIndex(usize);

/// Root of a menu (despawned when the menu closes).
#[derive(Component)]
pub struct MenuRoot;

/// Root of the pause overlay.
#[derive(Component)]
struct PauseMenu;

#[derive(Resource, Default)]
pub struct MenuCursor(pub usize);

#[derive(Resource, Default)]
pub struct Paused(pub bool);

const PANEL: Color = Color::srgba(0.05, 0.06, 0.08, 0.86);
const HIGHLIGHT: Color = Color::srgba(0.85, 0.35, 0.08, 0.9);

fn screen(
    commands: &mut Commands,
    title: &str,
    subtitle: &str,
    items: &[MenuItem],
    extra: impl Bundle,
) -> Entity {
    commands
        .spawn((MenuRoot, extra, menu_root()))
        .with_children(|root| {
            root.spawn((
                FitToWindow,
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.0,
                    padding: UiRect::all(px(20)),
                    row_gap: px(2),
                    min_width: px(460),
                    ..default()
                },
                BackgroundColor(PANEL),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new(title),
                    TextFont::from_font_size(36.0),
                    TextColor(Color::WHITE),
                ));
                panel.spawn((
                    Text::new(subtitle),
                    TextFont::from_font_size(15.0),
                    TextColor(Color::srgb(0.7, 0.7, 0.72)),
                    Node {
                        margin: UiRect::bottom(px(12)),
                        ..default()
                    },
                ));
                for (i, item) in items.iter().enumerate() {
                    panel
                        .spawn((
                            *item,
                            MenuIndex(i),
                            Button,
                            Node {
                                padding: UiRect::axes(px(10), px(2)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_child((
                            Text::new(""),
                            TextFont::from_font_size(20.0),
                            TextColor(Color::WHITE),
                        ));
                }
            });
        })
        .id()
}

/// A full-window node that centres one menu panel, with a margin the panel
/// is fitted into.
fn menu_root() -> Node {
    Node {
        width: percent(100),
        height: percent(100),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        padding: UiRect::all(px(16)),
        ..default()
    }
}

/// A menu panel that is scaled down, as a whole, when it is larger than the
/// window (less its parent's margin), so every row stays on screen at any
/// window size. It is never scaled up.
#[derive(Component)]
#[require(UiTransform)]
pub struct FitToWindow;

fn fit_to_window(
    mut panels: Query<(&mut UiTransform, &ComputedNode, &ChildOf), With<FitToWindow>>,
    parents: Query<&ComputedNode>,
) {
    for (mut transform, panel, child_of) in &mut panels {
        let Ok(parent) = parents.get(child_of.parent()) else {
            continue;
        };
        // Layout sizes ignore the transform, so this does not feed back.
        let room = parent.content_box().size();
        if panel.size.min_element() <= 0.0 || room.min_element() <= 0.0 {
            continue;
        }
        let scale = (room / panel.size).min_element().min(1.0);
        if (transform.scale.x - scale).abs() > 1e-3 {
            transform.scale = Vec2::splat(scale);
        }
    }
}

fn spawn_main_menu(mut commands: Commands, mut cursor: ResMut<MenuCursor>) {
    cursor.0 = 0;
    screen(
        &mut commands,
        "RACER",
        "Street racing in a greybox city",
        &[
            MenuItem::Action(Action::Race),
            MenuItem::Action(Action::Quit),
        ],
        DespawnOnExit(AppState::MainMenu),
    );
}

fn spawn_setup_menu(mut commands: Commands, mut cursor: ResMut<MenuCursor>) {
    let mut items: Vec<MenuItem> = Setting::ALL.iter().map(|s| MenuItem::Setting(*s)).collect();
    items.push(MenuItem::Action(Action::Start));
    items.push(MenuItem::Action(Action::Back));
    cursor.0 = items.len() - 2;
    screen(
        &mut commands,
        "Race setup",
        "Up/Down select, Left/Right change, Enter start, Esc back",
        &items,
        DespawnOnExit(AppState::Setup),
    );
}

fn go_to_race(mut next: ResMut<NextState<AppState>>) {
    next.set(AppState::Race);
}

fn set_paused(paused: &mut Paused, time: &mut Time<Virtual>, on: bool) {
    paused.0 = on;
    if on {
        time.pause();
    } else {
        time.unpause();
    }
}

fn resume_time(mut paused: ResMut<Paused>, mut time: ResMut<Time<Virtual>>) {
    set_paused(&mut paused, &mut time, false);
}

/// Opens the pause menu on Esc or gamepad Start, and when a gamepad is
/// unplugged mid-race.
#[allow(clippy::too_many_arguments)]
fn toggle_pause(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut connections: MessageReader<GamepadConnectionEvent>,
    mut paused: ResMut<Paused>,
    mut time: ResMut<Time<Virtual>>,
    mut cursor: ResMut<MenuCursor>,
    menus: Query<Entity, With<PauseMenu>>,
) {
    let unplugged = connections.read().any(|c| c.disconnected());
    let pressed = keys.just_pressed(KeyCode::Escape)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::Start))
        || unplugged;
    if !pressed || paused.0 {
        return;
    }
    set_paused(&mut paused, &mut time, true);
    cursor.0 = 0;
    for e in &menus {
        commands.entity(e).despawn();
    }
    screen(
        &mut commands,
        "Paused",
        "Esc to resume",
        &[
            MenuItem::Action(Action::Resume),
            MenuItem::Action(Action::Restart),
            MenuItem::Action(Action::Menu),
        ],
        (PauseMenu, DespawnOnExit(AppState::Race)),
    );
}

#[allow(clippy::too_many_arguments)]
fn menu_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    items: Query<(&MenuItem, &MenuIndex, Ref<Interaction>)>,
    pause_menus: Query<Entity, With<PauseMenu>>,
    just_paused: Query<(), Added<PauseMenu>>,
    mut cursor: ResMut<MenuCursor>,
    mut setup: ResMut<RaceSetup>,
    mut next: ResMut<NextState<AppState>>,
    state: Res<State<AppState>>,
    mut paused: ResMut<Paused>,
    mut time: ResMut<Time<Virtual>>,
    mut exit: MessageWriter<AppExit>,
    real: Res<Time<Real>>,
    mut entered: Local<f64>,
) {
    if state.is_changed() {
        *entered = real.elapsed_secs_f64();
    }
    let count = items.iter().count();
    // The key that opened the pause menu must not close it again.
    if count == 0 || !just_paused.is_empty() {
        return;
    }
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let key = |k: &[KeyCode]| keys.any_just_pressed(k.iter().copied());
    let mut activate =
        key(&[KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) || pad(GamepadButton::South);
    let mut back = key(&[KeyCode::Escape, KeyCode::Backspace])
        || pad(GamepadButton::East)
        || (*state.get() == AppState::Race && pad(GamepadButton::Start));
    // The results come up while the player may still be driving: a
    // handbrake (Space, South) or pause press must not leave them at once.
    if *state.get() == AppState::Results && real.elapsed_secs_f64() - *entered < 1.0 {
        activate = false;
        back = false;
    }
    if key(&[KeyCode::ArrowUp]) || pad(GamepadButton::DPadUp) {
        cursor.0 = (cursor.0 + count - 1) % count;
    }
    if key(&[KeyCode::ArrowDown]) || pad(GamepadButton::DPadDown) {
        cursor.0 = (cursor.0 + 1) % count;
    }
    let mut dir = 0;
    if key(&[KeyCode::ArrowLeft]) || pad(GamepadButton::DPadLeft) {
        dir = -1;
    }
    if key(&[KeyCode::ArrowRight]) || pad(GamepadButton::DPadRight) {
        dir = 1;
    }
    // A click selects and activates; the highlight otherwise follows the keys.
    for (_, index, interaction) in &items {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            cursor.0 = index.0;
            activate = true;
        }
    }
    let Some((item, _, _)) = items.iter().find(|(_, i, _)| i.0 == cursor.0) else {
        return;
    };
    let action = match *item {
        MenuItem::Setting(s) => {
            if dir != 0 || activate {
                s.change(&mut setup, if dir == 0 { 1 } else { dir });
            }
            None
        }
        MenuItem::Action(a) if activate => Some(a),
        MenuItem::Action(_) => None,
    };
    // "Back" on the main menu does nothing: only the Quit item quits.
    let action = action.or(back
        .then(|| match state.get() {
            AppState::MainMenu => None,
            AppState::Race => Some(Action::Resume),
            AppState::Results => Some(Action::Menu),
            _ => Some(Action::Back),
        })
        .flatten());
    let Some(action) = action else { return };
    let unpause = |commands: &mut Commands, paused: &mut Paused, time: &mut Time<Virtual>| {
        set_paused(paused, time, false);
        for e in &pause_menus {
            commands.entity(e).despawn();
        }
    };
    match action {
        Action::Race => next.set(AppState::Setup),
        Action::Quit => {
            exit.write(AppExit::Success);
        }
        Action::Start | Action::Restart => {
            unpause(&mut commands, &mut paused, &mut time);
            next.set(AppState::Loading);
        }
        Action::Back => next.set(AppState::MainMenu),
        Action::Resume => unpause(&mut commands, &mut paused, &mut time),
        Action::Menu => {
            unpause(&mut commands, &mut paused, &mut time);
            next.set(AppState::MainMenu);
        }
    }
}

fn draw_menu(
    cursor: Res<MenuCursor>,
    setup: Res<RaceSetup>,
    mut rows: Query<(&MenuItem, &MenuIndex, &Children, &mut BackgroundColor)>,
    mut texts: Query<&mut Text>,
) {
    for (item, index, children, mut background) in &mut rows {
        let selected = index.0 == cursor.0;
        background.0 = if selected { HIGHLIGHT } else { Color::NONE };
        let label = match item {
            MenuItem::Setting(s) => format!("{:<20} <  {}  >", s.label(), s.value(&setup)),
            MenuItem::Action(a) => a.label().to_string(),
        };
        for child in children.iter() {
            if let Ok(mut text) = texts.get_mut(child)
                && text.0 != label
            {
                text.0 = label.clone();
            }
        }
    }
}

/// A cell of the results table: (row, column).
#[derive(Component)]
struct ResultCell(usize, usize);

const RESULT_COLUMNS: [(&str, f32); 8] = [
    ("Pos", 44.0),
    ("Driver", 170.0),
    ("Car", 290.0),
    ("Time", 100.0),
    ("Gap", 100.0),
    ("Best lap", 100.0),
    ("Resets", 70.0),
    ("Assists", 250.0),
];

fn spawn_results(mut commands: Commands, mut cursor: ResMut<MenuCursor>, racers: Query<&Racer>) {
    cursor.0 = 0;
    let rows = racers.iter().count();
    commands
        .spawn((
            MenuRoot,
            RaceEntity,
            DespawnOnExit(AppState::Results),
            menu_root(),
        ))
        .with_children(|root| {
            root.spawn((
                FitToWindow,
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.0,
                    padding: UiRect::all(px(20)),
                    row_gap: px(3),
                    ..default()
                },
                BackgroundColor(PANEL),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Results"),
                    TextFont::from_font_size(36.0),
                    TextColor(Color::WHITE),
                ));
                for row in 0..=rows {
                    panel
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            height: px(26),
                            ..default()
                        })
                        .with_children(|line| {
                            for (col, (name, width)) in RESULT_COLUMNS.iter().enumerate() {
                                line.spawn((
                                    ResultCell(row, col),
                                    Text::new(if row == 0 { *name } else { "" }),
                                    TextLayout::no_wrap(),
                                    TextFont::from_font_size(16.0),
                                    TextColor(if row == 0 {
                                        Color::srgb(0.7, 0.7, 0.72)
                                    } else {
                                        Color::WHITE
                                    }),
                                    Node {
                                        width: px(*width),
                                        ..default()
                                    },
                                ));
                            }
                        });
                }
                for (i, a) in [Action::Restart, Action::Menu].into_iter().enumerate() {
                    panel
                        .spawn((
                            MenuItem::Action(a),
                            MenuIndex(i),
                            Button,
                            Node {
                                padding: UiRect::axes(px(10), px(4)),
                                margin: UiRect::top(px(if i == 0 { 14.0 } else { 0.0 })),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_child((
                            Text::new(""),
                            TextFont::from_font_size(20.0),
                            TextColor(Color::WHITE),
                        ));
                }
            });
        });
}

/// Formats seconds as m:ss.000.
pub fn lap_time(t: f32) -> String {
    // Round once, to whole milliseconds, so 59.9996 s carries into 1:00.000.
    let ms = (f64::from(t) * 1000.0).round().max(0.0) as u64;
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

/// The assists column: the car's ABS/TCS/ESC and, for the player, the
/// game's own aids (steering assist, racing line, automatic gearbox).
pub fn assists_label(assists: Assists, aids: Option<&DriverAids>) -> String {
    let aids = aids.copied().unwrap_or(DriverAids {
        steering_assist: false,
        racing_line: false,
        automatic: false,
    });
    let on: Vec<&str> = [
        (assists.abs, "ABS"),
        (assists.tcs, "TCS"),
        (assists.esc, "ESC"),
        (aids.steering_assist, "Steer"),
        (aids.racing_line, "Line"),
        (aids.automatic, "Auto"),
    ]
    .iter()
    .filter(|x| x.0)
    .map(|x| x.1)
    .collect();
    if on.is_empty() {
        "none".into()
    } else {
        on.join(" ")
    }
}

fn update_results(
    racers: Query<(&Racer, &Car, Option<&DriverAids>)>,
    mut cells: Query<(&ResultCell, &mut Text)>,
) {
    let list: Vec<(&Racer, &Car)> = racers.iter().map(|(r, c, _)| (r, c)).collect();
    let rows = racer_sim::race::results(&list);
    // Result rows carry the racer's name (unique on the grid), not its entity.
    let aids = |name: &str| {
        racers
            .iter()
            .find(|(r, _, _)| r.name == name)
            .and_then(|(_, _, a)| a)
    };
    for (cell, mut text) in &mut cells {
        if cell.0 == 0 {
            continue;
        }
        let Some(r) = rows.get(cell.0 - 1) else {
            continue;
        };
        let value = match cell.1 {
            0 => r.position.to_string(),
            1 => r.name.clone(),
            2 => r.car.clone(),
            3 => match r.status {
                Status::Finished(t) => lap_time(t),
                Status::Dnf => "DNF".into(),
                Status::Racing => "running".into(),
            },
            4 => match r.gap {
                Some(Gap::Seconds(s)) if s > 0.0 => format!("+{s:.3}"),
                Some(Gap::Laps(n)) => format!("+{n} lap{}", if n > 1 { "s" } else { "" }),
                _ => String::new(),
            },
            5 => r.best_lap.map(lap_time).unwrap_or_else(|| "-".into()),
            6 => r.resets.to_string(),
            _ => assists_label(r.assists, aids(&r.name)),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}
