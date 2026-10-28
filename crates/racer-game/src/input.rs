//! Driver input: keyboard, gamepad (gilrs, through Bevy) and steering wheels
//! with force feedback behind the [`WheelDevice`] trait.

use bevy::{input::gamepad::Gamepad, prelude::*};
use racer_dynamics::{Assists, Controls};
use racer_sim::{
    ai::AiDriver,
    vehicle::{Car, CarControls, CarTelemetry, ControlSystems},
};

/// Reads devices every frame, applies them to the player's car every fixed step
/// and sends force feedback back to the wheel.
pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DriverInput>()
            .init_resource::<DriverSettings>()
            .add_systems(PreUpdate, read_devices.after(bevy::input::InputSystems))
            .add_systems(FixedUpdate, apply_to_car.in_set(ControlSystems))
            .add_systems(FixedPostUpdate, force_feedback)
            .add_systems(PostUpdate, release_force_while_paused);
        if !app.world().contains_resource::<Wheel>() {
            app.insert_resource(Wheel(default_wheel()));
        }
    }
}

/// Marks the car the local player drives.
#[derive(Component)]
pub struct Player;

/// Which kind of device produced the latest steering input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Device {
    #[default]
    Keyboard,
    Gamepad,
    Wheel,
}

/// Raw driver intent from the devices, before filtering.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct DriverInput {
    pub device: Device,
    /// Steering target -1 (left) ..= 1 (right).
    pub steer: f32,
    pub throttle: f32,
    pub brake: f32,
    pub handbrake: f32,
    pub clutch: f32,
    /// Shift presses waiting for the next fixed step.
    pub shift_up: bool,
    pub shift_down: bool,
}

/// Player settings that shape the controls.
#[derive(Resource, Clone, Copy, Debug)]
pub struct DriverSettings {
    pub assists: Assists,
    /// Speed-sensitive steering filter for keyboard and gamepad.
    pub steering_assist: bool,
    pub automatic: bool,
}

impl Default for DriverSettings {
    fn default() -> Self {
        DriverSettings {
            assists: Assists::default(),
            steering_assist: true,
            automatic: true,
        }
    }
}

/// Steering filter for digital and analog inputs. With the assist on, the
/// usable lock shrinks with speed (so a full key press asks for about the
/// lateral grip available instead of full lock) and the rate is limited.
/// Wheels bypass it.
pub fn filter_steering(
    current: f32,
    target: f32,
    speed: f32,
    device: Device,
    assist: bool,
    dt: f32,
) -> f32 {
    if device == Device::Wheel {
        return target;
    }
    let limit = if assist {
        1.0 / (1.0 + (speed.abs() / 22.0).powf(1.5))
    } else {
        1.0
    };
    let target = target.clamp(-1.0, 1.0) * limit;
    let rate = match device {
        Device::Keyboard if target.abs() < current.abs() || target * current < 0.0 => 4.0,
        Device::Keyboard => 2.5,
        _ => 6.0,
    } * if assist { limit.max(0.35) } else { 1.0 };
    current + (target - current).clamp(-rate * dt, rate * dt)
}

fn read_devices(
    time: Res<Time<Virtual>>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut wheel: ResMut<Wheel>,
    mut input: ResMut<DriverInput>,
) {
    let key = |codes: &[KeyCode]| {
        if keys.any_pressed(codes.iter().copied()) {
            1.0
        } else {
            0.0
        }
    };
    let mut next = DriverInput {
        device: Device::Keyboard,
        steer: key(&[KeyCode::KeyD, KeyCode::ArrowRight])
            - key(&[KeyCode::KeyA, KeyCode::ArrowLeft]),
        throttle: key(&[KeyCode::KeyW, KeyCode::ArrowUp]),
        brake: key(&[KeyCode::KeyS, KeyCode::ArrowDown]),
        handbrake: key(&[KeyCode::Space]),
        clutch: key(&[KeyCode::KeyC]),
        shift_up: input.shift_up || keys.just_pressed(KeyCode::KeyE),
        shift_down: input.shift_down || keys.just_pressed(KeyCode::KeyQ),
    };
    for pad in &gamepads {
        // `Gamepad` holds raw values (Bevy's axis settings only filter its
        // change events), so this is the one deadzone.
        let stick = pad.left_stick().x;
        // Each trigger gets its own deadzone, rescaled: a worn trigger resting
        // just above it must not read as a touch of throttle while the other
        // is pressed (the automatic's reverse needs the throttle released).
        let trigger = |b| ((pad.get(b).unwrap_or(0.0) - 0.05) / 0.95).max(0.0);
        let (rt, lt) = (
            trigger(GamepadButton::RightTrigger2),
            trigger(GamepadButton::LeftTrigger2),
        );
        if stick.abs() > 0.08 || rt > 0.0 || lt > 0.0 {
            // Deadzone and a mild curve for fine control around centre. The
            // stronger steering wins, so keyboard steering works with pad pedals.
            let s = ((stick.abs() - 0.08) / 0.92).max(0.0);
            let steer = stick.signum() * s.powf(1.4);
            if steer.abs() > next.steer.abs() || next.steer == 0.0 {
                next.device = Device::Gamepad;
                next.steer = steer;
            }
            next.throttle = next.throttle.max(rt);
            next.brake = next.brake.max(lt);
        }
        next.handbrake = next.handbrake.max(if pad.pressed(GamepadButton::South) {
            1.0
        } else {
            0.0
        });
        next.shift_up |= pad.just_pressed(GamepadButton::RightTrigger);
        next.shift_down |= pad.just_pressed(GamepadButton::LeftTrigger);
    }
    if let Some(r) = wheel.0.read() {
        next.device = Device::Wheel;
        next.steer = r.steer;
        next.throttle = r.throttle;
        next.brake = r.brake;
        next.clutch = r.clutch;
        next.shift_up |= r.shift_up;
        next.shift_down |= r.shift_down;
    }
    // Shifts pressed while paused (time stands still) are dropped, not
    // latched for when the race resumes.
    if time.is_paused() {
        next.shift_up = false;
        next.shift_down = false;
    }
    *input = next;
}

/// The player's car while a human drives it (not on autopilot).
type HumanCar = (With<Player>, Without<AiDriver>);

fn apply_to_car(
    time: Res<Time>,
    settings: Res<DriverSettings>,
    mut input: ResMut<DriverInput>,
    mut cars: Query<(&mut CarControls, &CarTelemetry), HumanCar>,
) {
    for (mut controls, telemetry) in &mut cars {
        let c = &mut controls.0;
        let steer = filter_steering(
            c.steer as f32,
            input.steer,
            telemetry.telemetry.speed as f32,
            input.device,
            settings.steering_assist,
            time.delta_secs(),
        );
        *c = Controls {
            throttle: input.throttle as f64,
            brake: input.brake as f64,
            steer: steer as f64,
            handbrake: input.handbrake as f64,
            clutch: input.clutch as f64,
            shift_up: input.shift_up,
            shift_down: input.shift_down,
            automatic: settings.automatic,
            assists: settings.assists,
            hold: false,
        };
    }
    input.shift_up = false;
    input.shift_down = false;
}

/// Wheel rim torque (Nm) that maps to full force-feedback output.
const FFB_FULL_SCALE_NM: f64 = 8.0;

/// Force-feedback command, -1..=1 (positive pulls the rim right), from the
/// tire model's aligning torque at the front wheels through the steering ratio.
pub fn ffb_force(steering_torque: f64, steering_ratio: f64) -> f32 {
    // Positive aligning torque turns the road wheels left: the rim is pulled left.
    (-steering_torque / steering_ratio / FFB_FULL_SCALE_NM).clamp(-1.0, 1.0) as f32
}

fn force_feedback(mut wheel: ResMut<Wheel>, cars: Query<(&Car, &CarTelemetry), With<Player>>) {
    let force = cars.iter().next().map_or(0.0, |(car, t)| {
        ffb_force(t.telemetry.steering_torque, car.steering_ratio)
    });
    wheel.0.set_force(force);
}

/// Lets the rim go slack while time stands still (pause menu): the fixed
/// steps that send the race force do not run then, and the wheel would
/// otherwise keep playing the last one.
fn release_force_while_paused(time: Res<Time<Virtual>>, mut wheel: ResMut<Wheel>) {
    if time.is_paused() {
        wheel.0.set_force(0.0);
    }
}

/// One sample from a steering wheel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelReading {
    pub steer: f32,
    pub throttle: f32,
    pub brake: f32,
    pub clutch: f32,
    pub shift_up: bool,
    pub shift_down: bool,
}

/// A steering wheel with force feedback.
pub trait WheelDevice: Send + Sync {
    fn name(&self) -> &str;
    /// Latest state, or `None` when no wheel is connected.
    fn read(&mut self) -> Option<WheelReading>;
    /// Constant force -1..=1 (positive pulls the rim right).
    fn set_force(&mut self, force: f32);
}

/// The active wheel backend.
#[derive(Resource)]
pub struct Wheel(pub Box<dyn WheelDevice>);

/// No wheel: reads nothing, ignores forces.
pub struct NoWheel;

impl WheelDevice for NoWheel {
    fn name(&self) -> &str {
        "none"
    }
    fn read(&mut self) -> Option<WheelReading> {
        None
    }
    fn set_force(&mut self, _force: f32) {}
}

/// Scripted wheel for tests: replays a reading and records every force.
#[derive(Default)]
pub struct MockWheel {
    pub reading: Option<WheelReading>,
    pub forces: std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
}

impl WheelDevice for MockWheel {
    fn name(&self) -> &str {
        "mock"
    }
    fn read(&mut self) -> Option<WheelReading> {
        self.reading
    }
    fn set_force(&mut self, force: f32) {
        self.forces.lock().unwrap().push(force);
    }
}

/// SDL3 wheel when built with the `wheel` feature and SDL initialises; otherwise none.
pub fn default_wheel() -> Box<dyn WheelDevice> {
    #[cfg(feature = "wheel")]
    match crate::sdl_wheel::SdlWheel::new() {
        Ok(w) => return Box::new(w),
        Err(e) => warn!("SDL3 wheel backend unavailable: {e}"),
    }
    Box::new(NoWheel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steering_assist_reduces_lock_with_speed_and_wheels_bypass_it() {
        let settle = |speed: f32, device, assist| {
            let mut s = 0.0;
            for _ in 0..1000 {
                s = filter_steering(s, 1.0, speed, device, assist, 1.0 / 240.0);
            }
            s
        };
        assert!((settle(0.0, Device::Keyboard, true) - 1.0).abs() < 1e-3);
        let fast = settle(40.0, Device::Keyboard, true);
        assert!(fast < 0.4 && fast > 0.1, "{fast}");
        assert_eq!(settle(40.0, Device::Keyboard, false), 1.0);
        assert_eq!(
            filter_steering(0.0, 0.7, 40.0, Device::Wheel, true, 1.0 / 240.0),
            0.7
        );
        // Rate limited: one step moves a keyboard press only a little.
        assert!(filter_steering(0.0, 1.0, 0.0, Device::Keyboard, true, 1.0 / 240.0) < 0.02);
    }

    #[test]
    fn force_feedback_pulls_the_rim_towards_centre() {
        // Turning right, the front tires slide left relative to the road and
        // their aligning torque turns the wheels back left: the rim pulls left.
        assert!(ffb_force(40.0, 15.0) < 0.0);
        assert_eq!(ffb_force(1e6, 15.0), -1.0);
    }

    #[test]
    fn mock_wheel_drives_the_player_car_and_receives_force_feedback() {
        use bevy::time::TimeUpdateStrategy;
        use racer_sim::{
            SimPlugin, tick_duration,
            track::{spawn_track, test_track},
            vehicle::{VehiclePlugin, spawn_car},
        };

        let forces = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let wheel = MockWheel {
            reading: Some(WheelReading {
                steer: 0.3,
                throttle: 0.5,
                ..default()
            }),
            forces: forces.clone(),
        };
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            bevy::input::InputPlugin,
            SimPlugin,
            VehiclePlugin,
        ))
        .insert_resource(Wheel(Box::new(wheel)))
        .add_plugins(InputPlugin)
        .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
        app.finish();
        app.cleanup();
        let spec = racer_dynamics::load_car("vw-golf-gti-mk8").unwrap();
        let car = {
            let world = app.world_mut();
            let mut commands = world.commands();
            spawn_track(&mut commands, &test_track());
            let car = spawn_car(&mut commands, &spec, Transform::default(), Player).unwrap();
            world.flush();
            car
        };
        // 2.5 s: up to speed while still on the road (it drifts right).
        for _ in 0..600 {
            app.update();
        }
        let controls = app.world().get::<CarControls>(car).unwrap().0;
        assert_eq!(
            controls.steer, 0.3f32 as f64,
            "wheel steering is not filtered"
        );
        let t = app.world().get::<CarTelemetry>(car).unwrap().telemetry;
        assert!(t.speed > 5.0, "{}", t.speed);
        let forces = forces.lock().unwrap();
        assert!(forces.len() > 500);
        // Steering right at speed: the rim is pulled back left.
        let recent = &forces[forces.len() - 60..];
        let mean = recent.iter().sum::<f32>() / recent.len() as f32;
        assert!(mean < -0.05, "{mean}");
    }
    /// A wheel whose reading the test changes while the app runs; shift
    /// presses are edges, reported once, as the SDL backend does.
    #[derive(Clone, Default)]
    struct SharedWheel(std::sync::Arc<std::sync::Mutex<WheelReading>>);

    impl WheelDevice for SharedWheel {
        fn name(&self) -> &str {
            "shared"
        }
        fn read(&mut self) -> Option<WheelReading> {
            let mut r = self.0.lock().unwrap();
            let reading = *r;
            r.shift_up = false;
            r.shift_down = false;
            Some(reading)
        }
        fn set_force(&mut self, _force: f32) {}
    }

    /// The player's Golf on the test track with the input plugin, one fixed
    /// step per update.
    fn player_app(wheel: Box<dyn WheelDevice>, automatic: bool) -> (App, Entity) {
        use bevy::time::TimeUpdateStrategy;
        use racer_sim::{
            SimPlugin, tick_duration,
            track::{spawn_track, test_track},
            vehicle::{VehiclePlugin, spawn_car},
        };
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            bevy::input::InputPlugin,
            SimPlugin,
            VehiclePlugin,
        ))
        .insert_resource(Wheel(wheel))
        .add_plugins(InputPlugin)
        .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
        app.world_mut().resource_mut::<DriverSettings>().automatic = automatic;
        app.finish();
        app.cleanup();
        let spec = racer_dynamics::load_car("vw-golf-gti-mk8").unwrap();
        let car = {
            let world = app.world_mut();
            let mut commands = world.commands();
            spawn_track(&mut commands, &test_track());
            let car = spawn_car(&mut commands, &spec, Transform::default(), Player).unwrap();
            world.flush();
            car
        };
        (app, car)
    }

    fn run(app: &mut App, seconds: f32) {
        for _ in 0..(seconds * 240.0) as usize {
            app.update();
        }
    }

    fn telemetry(app: &App, car: Entity) -> racer_dynamics::Telemetry {
        app.world().get::<CarTelemetry>(car).unwrap().telemetry
    }

    fn keys(app: &mut App) -> Mut<'_, ButtonInput<KeyCode>> {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>()
    }

    #[test]
    fn keyboard_brake_held_at_a_standstill_reverses_and_throttle_goes_forward_again() {
        let (mut app, car) = player_app(Box::new(NoWheel), true);
        run(&mut app, 1.0);
        keys(&mut app).press(KeyCode::KeyS);
        // A short press keeps 1st.
        run(&mut app, 0.4);
        assert_eq!(telemetry(&app, car).gear, 1);
        run(&mut app, 2.6);
        let t = telemetry(&app, car);
        assert_eq!(t.gear, -1);
        assert!(t.speed < -2.0, "{}", t.speed);
        // Throttle: brakes the reversing car, then 1st and forwards.
        keys(&mut app).release(KeyCode::KeyS);
        keys(&mut app).press(KeyCode::KeyW);
        run(&mut app, 4.0);
        let t = telemetry(&app, car);
        assert!(t.gear >= 1);
        assert!(t.speed > 3.0, "{}", t.speed);
    }

    #[test]
    fn manual_gearbox_q_twice_at_a_standstill_selects_reverse_and_w_drives_it() {
        let (mut app, car) = player_app(Box::new(NoWheel), false);
        run(&mut app, 1.0);
        // Through keyboard events: the input plugin clears `just_pressed`
        // before it reads them.
        let key = |app: &mut App, state| {
            use bevy::input::keyboard::KeyboardInput;
            app.world_mut()
                .resource_mut::<Messages<KeyboardInput>>()
                .write(KeyboardInput {
                    key_code: KeyCode::KeyQ,
                    logical_key: bevy::input::keyboard::Key::Character("q".into()),
                    state,
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
            app.update();
        };
        for _ in 0..2 {
            key(&mut app, bevy::input::ButtonState::Pressed);
            key(&mut app, bevy::input::ButtonState::Released);
            run(&mut app, 0.5);
        }
        assert_eq!(telemetry(&app, car).gear, -1);
        keys(&mut app).press(KeyCode::KeyW);
        run(&mut app, 2.0);
        let t = telemetry(&app, car);
        assert!(t.speed < -2.0, "{}", t.speed);
        assert_eq!(t.gear, -1);
    }

    fn gamepad(app: &mut App) -> Entity {
        use bevy::input::gamepad::{GamepadConnection, GamepadConnectionEvent};
        let pad = app.world_mut().spawn_empty().id();
        app.world_mut()
            .resource_mut::<Messages<GamepadConnectionEvent>>()
            .write(GamepadConnectionEvent::new(
                pad,
                GamepadConnection::Connected {
                    name: "Test pad".into(),
                    vendor_id: None,
                    product_id: None,
                },
            ));
        app.update();
        pad
    }

    fn pad_button(app: &mut App, pad: Entity, button: GamepadButton, value: f32) {
        use bevy::input::gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent};
        app.world_mut()
            .resource_mut::<Messages<RawGamepadEvent>>()
            .write(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                pad, button, value,
            )));
    }

    #[test]
    fn gamepad_left_trigger_reverses_in_proportion() {
        let reverse_speed = |trigger: f32| {
            let (mut app, car) = player_app(Box::new(NoWheel), true);
            let pad = gamepad(&mut app);
            run(&mut app, 1.0);
            pad_button(&mut app, pad, GamepadButton::LeftTrigger2, trigger);
            run(&mut app, 2.5);
            let t = telemetry(&app, car);
            assert_eq!(t.gear, -1, "trigger {trigger}");
            -t.speed
        };
        let (light, full) = (reverse_speed(0.35), reverse_speed(1.0));
        assert!(0.3 < light && light < full - 1.0, "{light} {full}");
    }

    #[test]
    fn a_right_trigger_resting_above_zero_does_not_block_reverse() {
        let (mut app, car) = player_app(Box::new(NoWheel), true);
        let pad = gamepad(&mut app);
        run(&mut app, 1.0);
        // A worn trigger that never quite returns.
        pad_button(&mut app, pad, GamepadButton::RightTrigger2, 0.08);
        pad_button(&mut app, pad, GamepadButton::LeftTrigger2, 1.0);
        run(&mut app, 2.5);
        let t = telemetry(&app, car);
        assert_eq!(t.gear, -1);
        assert!(t.speed < -1.0, "{}", t.speed);
    }

    #[test]
    fn gamepad_bumpers_and_wheel_paddles_select_reverse_in_the_manual() {
        let (mut app, car) = player_app(Box::new(NoWheel), false);
        let pad = gamepad(&mut app);
        run(&mut app, 1.0);
        for _ in 0..2 {
            pad_button(&mut app, pad, GamepadButton::LeftTrigger, 1.0);
            app.update();
            pad_button(&mut app, pad, GamepadButton::LeftTrigger, 0.0);
            run(&mut app, 0.5);
        }
        assert_eq!(telemetry(&app, car).gear, -1);
        pad_button(&mut app, pad, GamepadButton::RightTrigger2, 0.6);
        run(&mut app, 2.0);
        assert!(telemetry(&app, car).speed < -1.5);

        let wheel = SharedWheel::default();
        let (mut app, car) = player_app(Box::new(wheel.clone()), false);
        run(&mut app, 1.0);
        for _ in 0..2 {
            wheel.0.lock().unwrap().shift_down = true;
            run(&mut app, 0.5);
        }
        assert_eq!(telemetry(&app, car).gear, -1);
        wheel.0.lock().unwrap().throttle = 0.6;
        run(&mut app, 2.0);
        assert!(telemetry(&app, car).speed < -1.5);
    }

    #[test]
    fn wheel_brake_pedal_reverses_in_proportion_in_the_automatic() {
        let reverse_speed = |pedal: f32| {
            let wheel = SharedWheel::default();
            let (mut app, car) = player_app(Box::new(wheel.clone()), true);
            run(&mut app, 1.0);
            wheel.0.lock().unwrap().brake = pedal;
            run(&mut app, 2.5);
            let t = telemetry(&app, car);
            assert_eq!(t.gear, -1, "pedal {pedal}");
            -t.speed
        };
        let (light, full) = (reverse_speed(0.3), reverse_speed(1.0));
        assert!(0.3 < light && light < full - 1.0, "{light} {full}");
    }
}
