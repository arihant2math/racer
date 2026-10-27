//! Rendering: materials and post-processing beyond Bevy's defaults.

pub mod surface;

use bevy::prelude::*;

/// Greybox car paint: a metallic base under a glossy clearcoat. Placeholder
/// for the custom flake + clearcoat material extension planned for M4.
pub fn car_paint(base_color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color,
        metallic: 0.6,
        perceptual_roughness: 0.45,
        clearcoat: 1.0,
        clearcoat_perceptual_roughness: 0.05,
        ..default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn car_paint_has_glossy_clearcoat_over_rougher_base() {
        let paint = car_paint(Color::srgb(0.8, 0.1, 0.1));
        assert_eq!(paint.clearcoat, 1.0);
        assert!(paint.clearcoat_perceptual_roughness < paint.perceptual_roughness);
    }
}
