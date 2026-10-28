//! QA repro for the SDL3 wheel backend (`--features wheel`), with SDL
//! virtual joysticks standing in for hardware.
#![cfg(feature = "wheel")]

use std::time::Duration;

use sdl3_sys::{init, joystick};

/// Attaches a virtual joystick of the given type and returns its id.
fn attach(kind: joystick::SDL_JoystickType, name: &std::ffi::CStr) -> joystick::SDL_JoystickID {
    // SAFETY: plain SDL calls on a descriptor that outlives them.
    unsafe {
        assert!(init::SDL_InitSubSystem(init::SDL_INIT_JOYSTICK));
        let desc = joystick::SDL_VirtualJoystickDesc {
            r#type: kind.0 as u16,
            naxes: 6,
            nbuttons: 15,
            name: name.as_ptr(),
            ..Default::default()
        };
        let id = joystick::SDL_AttachVirtualJoystick(&desc);
        assert_ne!(id.0, 0, "virtual joystick");
        id
    }
}

fn detach(id: joystick::SDL_JoystickID) {
    // SAFETY: `id` is a virtual joystick attached by `attach`.
    unsafe {
        joystick::SDL_DetachVirtualJoystick(id);
    }
}

/// QA-UI-11: the SDL backend opened the first joystick with two axes as the
/// wheel, so an ordinary gamepad became a "wheel": its resting sticks read
/// as half throttle and half brake, and the wheel reading overrode the
/// keyboard and Bevy's own gamepad input. A joystick SDL types as a wheel
/// must still be opened. (One test: the sdl3 crate initialises SDL on one
/// thread per process, so a second backend in this binary would not start.)
#[test]
fn a_gamepad_is_not_taken_for_a_wheel() {
    let pad = attach(joystick::SDL_JOYSTICK_TYPE_GAMEPAD, c"QA virtual gamepad");
    let mut wheel = racer_game::input::default_wheel();
    assert_eq!(wheel.name(), "sdl3");
    std::thread::sleep(Duration::from_millis(300));
    let reading = wheel.read();
    assert_eq!(
        reading, None,
        "a gamepad at rest was opened as a wheel: {reading:?}"
    );

    let id = attach(joystick::SDL_JOYSTICK_TYPE_WHEEL, c"QA virtual wheel");
    // SAFETY: `id` is attached; the handle is closed before the device goes.
    let handle = unsafe {
        let handle = joystick::SDL_OpenJoystick(id);
        assert!(!handle.is_null());
        // Steering a quarter right, pedals released (Logitech-style +32767).
        assert!(joystick::SDL_SetJoystickVirtualAxis(handle, 0, 8192));
        for pedal in 1..4 {
            assert!(joystick::SDL_SetJoystickVirtualAxis(handle, pedal, 32767));
        }
        handle
    };
    // The backend looks for a new wheel about once a second.
    std::thread::sleep(Duration::from_millis(1500));
    let reading = wheel.read();
    // SAFETY: opened above.
    unsafe { joystick::SDL_CloseJoystick(handle) };
    detach(id);
    detach(pad);
    let reading = reading.expect("a virtual wheel is opened");
    assert!((reading.steer - 0.25).abs() < 0.01, "{reading:?}");
    assert_eq!((reading.throttle, reading.brake), (0.0, 0.0), "{reading:?}");
}
