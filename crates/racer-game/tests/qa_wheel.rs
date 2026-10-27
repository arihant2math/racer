//! QA repro for the SDL3 wheel backend (`--features wheel`), with an SDL
//! virtual joystick standing in for hardware.
#![cfg(feature = "wheel")]

use std::time::Duration;

use sdl3_sys::{init, joystick};

/// QA-UI-11: the SDL backend opens the first joystick with two axes as the
/// wheel, so an ordinary gamepad becomes a "wheel": its resting sticks read
/// as half throttle and half brake, and the wheel reading overrides the
/// keyboard and Bevy's own gamepad input.
#[test]
#[ignore = "QA-UI-11"]
fn a_gamepad_is_not_taken_for_a_wheel() {
    // SAFETY: plain SDL calls on a descriptor that outlives them.
    unsafe {
        assert!(init::SDL_InitSubSystem(init::SDL_INIT_JOYSTICK));
        let desc = joystick::SDL_VirtualJoystickDesc {
            r#type: joystick::SDL_JOYSTICK_TYPE_GAMEPAD.0 as u16,
            naxes: 6,
            nbuttons: 15,
            name: c"QA virtual gamepad".as_ptr(),
            ..Default::default()
        };
        let id = joystick::SDL_AttachVirtualJoystick(&desc);
        assert_ne!(id.0, 0, "virtual joystick");
    }
    let mut wheel = racer_game::input::default_wheel();
    std::thread::sleep(Duration::from_millis(300));
    let reading = wheel.read();
    assert_eq!(
        reading, None,
        "a gamepad at rest was opened as a wheel: {reading:?}"
    );
}
