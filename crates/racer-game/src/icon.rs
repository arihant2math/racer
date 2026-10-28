//! The window icon (`assets/icon/`, made by `tools/make_icon.py`), embedded
//! in the binary so it never depends on where the assets are.
//!
//! winit sets it on Windows and X11; on Wayland and macOS winit ignores it:
//! Wayland compositors take the icon from the `.desktop` file that matches
//! the window's app id ([`APP_ID`]), and macOS from the app bundle.

use bevy::{ecs::system::NonSendMarker, prelude::*, window::PrimaryWindow, winit::WINIT_WINDOWS};

/// The window's app id (Wayland) and class (X11): `racer.desktop` matches it.
pub const APP_ID: &str = "racer";

const ICON_PNG: &[u8] = include_bytes!("../../../assets/icon/icon-64.png");

/// The icon as RGBA pixels: (pixels, width, height).
pub fn icon_rgba() -> Result<(Vec<u8>, u32, u32), String> {
    let image = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let (w, h) = image.dimensions();
    Ok((image.into_raw(), w, h))
}

/// Sets the primary window's icon once winit has made the window.
pub struct WindowIconPlugin;

impl Plugin for WindowIconPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IconSet>()
            .add_systems(Update, set_window_icon.run_if(not(icon_set)));
    }
}

#[derive(Resource, Default)]
struct IconSet(bool);

fn icon_set(set: Res<IconSet>) -> bool {
    set.0
}

fn set_window_icon(
    _main_thread: NonSendMarker,
    window: Query<Entity, With<PrimaryWindow>>,
    mut set: ResMut<IconSet>,
) {
    let Ok(window) = window.single() else {
        return;
    };
    WINIT_WINDOWS.with_borrow(|windows| {
        // The winit window is made a frame or so after the entity.
        let Some(winit_window) = windows.get_window(window) else {
            return;
        };
        set.0 = true;
        match icon_rgba().and_then(|(rgba, w, h)| {
            winit::window::Icon::from_rgba(rgba, w, h).map_err(|e| e.to_string())
        }) {
            Ok(icon) => winit_window.set_window_icon(Some(icon)),
            Err(e) => warn!("window icon: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_icon_decodes() {
        let (rgba, w, h) = super::icon_rgba().unwrap();
        assert_eq!((w, h), (64, 64));
        assert_eq!(rgba.len(), 64 * 64 * 4);
        // Opaque in the middle, white-ish letter or orange tile.
        assert_eq!(rgba[(32 * 64 + 32) * 4 + 3], 255);
    }
}
