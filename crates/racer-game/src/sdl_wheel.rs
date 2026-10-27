//! Steering wheel input and constant-force feedback through SDL3.
//!
//! SDL handles are not `Send`, so a dedicated thread owns SDL, polls the
//! first wheel-like joystick and plays the force the game last requested.
//! Axis mapping follows Logitech-style wheels: axis 0 steering, axes 1-3
//! throttle/brake/clutch (released = +32767), buttons 4/5 the paddles.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use sdl3::sys::haptic as h;

use crate::input::{WheelDevice, WheelReading};

pub struct SdlWheel {
    name: Arc<Mutex<String>>,
    reading: Arc<Mutex<Option<WheelReading>>>,
    /// Requested force as f32 bits.
    force: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
}

impl SdlWheel {
    /// Starts the SDL thread. Fails if SDL's joystick or haptic subsystem
    /// cannot initialise; succeeds with no wheel connected.
    pub fn new() -> Result<Self, String> {
        let wheel = SdlWheel {
            name: Arc::new(Mutex::new("SDL3 (no wheel)".into())),
            reading: Arc::default(),
            force: Arc::default(),
            stop: Arc::default(),
        };
        let (ready, started) = mpsc::channel();
        let (name, reading, force, stop) = (
            wheel.name.clone(),
            wheel.reading.clone(),
            wheel.force.clone(),
            wheel.stop.clone(),
        );
        thread::Builder::new()
            .name("sdl-wheel".into())
            .spawn(move || run(ready, name, reading, force, stop))
            .map_err(|e| e.to_string())?;
        started.recv().map_err(|e| e.to_string())??;
        Ok(wheel)
    }
}

impl Drop for SdlWheel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl WheelDevice for SdlWheel {
    fn name(&self) -> &str {
        "sdl3"
    }

    fn read(&mut self) -> Option<WheelReading> {
        *self.reading.lock().unwrap()
    }

    fn set_force(&mut self, force: f32) {
        self.force
            .store(force.clamp(-1.0, 1.0).to_bits(), Ordering::Relaxed);
    }
}

/// A connected wheel and, if it has one, its constant-force effect.
struct Open {
    joystick: sdl3::joystick::Joystick,
    haptic: Option<(sdl3::haptic::Haptic, h::SDL_HapticEffectID)>,
}

fn constant_effect(level: f32) -> h::SDL_HapticEffect {
    h::SDL_HapticEffect {
        constant: h::SDL_HapticConstant {
            r#type: h::SDL_HAPTIC_CONSTANT,
            direction: h::SDL_HapticDirection {
                r#type: h::SDL_HAPTIC_STEERING_AXIS,
                dir: [0; 3],
            },
            length: h::SDL_HAPTIC_INFINITY,
            level: (level * 32767.0) as i16,
            ..Default::default()
        },
    }
}

fn open(joysticks: &sdl3::JoystickSubsystem, haptics: &sdl3::HapticSubsystem) -> Option<Open> {
    let id = joysticks.joysticks().ok()?.into_iter().next()?;
    let joystick = joysticks.open(id).ok()?;
    if joystick.num_axes() < 2 {
        return None;
    }
    // SAFETY: `joystick` is open.
    let instance = unsafe { sdl3::sys::joystick::SDL_GetJoystickID(joystick.raw()) };
    let haptic = haptics
        .open_from_joystick_id(instance.0)
        .ok()
        .and_then(|haptic| {
            // SAFETY: `haptic` is a valid open device; the effect lives as long as it.
            let effect = unsafe {
                if h::SDL_GetHapticFeatures(haptic.raw()) & u32::from(h::SDL_HAPTIC_CONSTANT.0) == 0
                {
                    return None;
                }
                h::SDL_SetHapticAutocenter(haptic.raw(), 0);
                let id = h::SDL_CreateHapticEffect(haptic.raw(), &constant_effect(0.0));
                if id.0 < 0 || !h::SDL_RunHapticEffect(haptic.raw(), id, h::SDL_HAPTIC_INFINITY) {
                    return None;
                }
                id
            };
            Some((haptic, effect))
        });
    Some(Open { joystick, haptic })
}

fn run(
    ready: mpsc::Sender<Result<(), String>>,
    name: Arc<Mutex<String>>,
    reading: Arc<Mutex<Option<WheelReading>>>,
    force: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
) {
    let init = || -> Result<_, String> {
        let sdl = sdl3::init().map_err(|e| e.to_string())?;
        let joysticks = sdl.joystick().map_err(|e| e.to_string())?;
        let haptics = sdl.haptic().map_err(|e| e.to_string())?;
        Ok((sdl, joysticks, haptics))
    };
    let (_sdl, joysticks, haptics) = match init() {
        Ok(s) => {
            let _ = ready.send(Ok(()));
            s
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let mut wheel: Option<Open> = None;
    let mut buttons = (false, false);
    let mut tick = 0u32;
    while !stop.load(Ordering::Relaxed) {
        joysticks.update();
        if wheel.as_ref().is_some_and(|w| !w.joystick.connected()) {
            wheel = None;
            *reading.lock().unwrap() = None;
        }
        // Look for a newly connected wheel about once a second.
        if wheel.is_none() && tick.is_multiple_of(250) {
            wheel = open(&joysticks, &haptics);
            if let Some(w) = &wheel {
                *name.lock().unwrap() = w.joystick.name();
            }
        }
        if let Some(w) = &wheel {
            let axis = |i| w.joystick.axis(i).map_or(0.0, |v| f32::from(v) / 32767.0);
            let pedal = |i| ((1.0 - axis(i)) / 2.0).clamp(0.0, 1.0);
            let pressed = |b| w.joystick.button(b).unwrap_or(false);
            let now = (pressed(4), pressed(5));
            *reading.lock().unwrap() = Some(WheelReading {
                steer: axis(0),
                throttle: pedal(1),
                brake: pedal(2),
                clutch: if w.joystick.num_axes() > 3 {
                    pedal(3)
                } else {
                    0.0
                },
                shift_up: now.0 && !buttons.0,
                shift_down: now.1 && !buttons.1,
            });
            buttons = now;
            if let Some((haptic, effect)) = &w.haptic {
                let level = f32::from_bits(force.load(Ordering::Relaxed));
                // SAFETY: the effect was created on this device, which is still open.
                unsafe {
                    h::SDL_UpdateHapticEffect(haptic.raw(), *effect, &constant_effect(level));
                }
            }
        }
        tick = tick.wrapping_add(1);
        thread::sleep(Duration::from_millis(4));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No wheel is attached here: SDL must still initialise and read nothing.
    #[test]
    fn initialises_without_a_device() {
        let mut wheel = SdlWheel::new().expect("SDL3 joystick and haptic subsystems initialise");
        thread::sleep(Duration::from_millis(50));
        assert_eq!(wheel.read(), None);
        wheel.set_force(0.5);
    }
}
