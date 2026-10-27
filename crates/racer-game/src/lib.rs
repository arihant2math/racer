//! Game client library: scenes and plugins shared by the game binary and
//! the capture harness.

pub mod driving;
pub mod greybox;
pub mod input;
#[cfg(feature = "wheel")]
mod sdl_wheel;
