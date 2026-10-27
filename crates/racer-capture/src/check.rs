//! Image sanity checks and golden-image comparison.

use image::RgbaImage;

/// Basic statistics of a captured frame.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageStats {
    pub width: u32,
    pub height: u32,
    /// Mean Rec. 709 luma, 0-255.
    pub mean_luma: f64,
    /// Standard deviation of luma, 0-255.
    pub luma_stddev: f64,
    /// Fraction of pixels sharing the single most common colour.
    pub dominant_fraction: f64,
}

/// Computes [`ImageStats`] for an image.
pub fn stats(img: &RgbaImage) -> ImageStats {
    let n = f64::from(img.width()) * f64::from(img.height());
    let (mut sum, mut sum_sq) = (0.0, 0.0);
    let mut counts = std::collections::HashMap::<[u8; 3], u64>::new();
    for p in img.pixels() {
        let [r, g, b, _] = p.0;
        let y = 0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b);
        sum += y;
        sum_sq += y * y;
        *counts.entry([r, g, b]).or_default() += 1;
    }
    let mean = sum / n;
    let dominant = counts.values().copied().max().unwrap_or(0);
    ImageStats {
        width: img.width(),
        height: img.height(),
        mean_luma: mean,
        luma_stddev: (sum_sq / n - mean * mean).max(0.0).sqrt(),
        dominant_fraction: dominant as f64 / n,
    }
}

/// Rejects frames that are blank, (nearly) a single colour, or crushed to
/// black or white: the symptoms of capturing before pipelines are ready.
pub fn check(img: &RgbaImage) -> Result<ImageStats, String> {
    if img.width() == 0 || img.height() == 0 {
        return Err("image is empty".into());
    }
    let s = stats(img);
    if s.dominant_fraction > 0.9 {
        return Err(format!(
            "{:.1}% of pixels are one colour",
            s.dominant_fraction * 100.0
        ));
    }
    if s.luma_stddev < 4.0 {
        return Err(format!(
            "almost no contrast (luma stddev {:.2})",
            s.luma_stddev
        ));
    }
    if !(8.0..=247.0).contains(&s.mean_luma) {
        return Err(format!(
            "mean luma {:.1} is crushed to black or white",
            s.mean_luma
        ));
    }
    Ok(s)
}

/// Tolerances for [`diff`].
#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    /// A pixel differs if any channel differs by more than this (0-255).
    pub pixel: u8,
    /// Maximum allowed fraction of differing pixels.
    pub max_differing_fraction: f64,
    /// Maximum allowed mean absolute channel error (0-255).
    pub max_mean_error: f64,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            pixel: 24,
            max_differing_fraction: 0.01,
            max_mean_error: 2.0,
        }
    }
}

/// Result of comparing an image against a golden.
#[derive(Clone, Debug)]
pub struct DiffReport {
    pub mean_error: f64,
    pub differing_fraction: f64,
    pub passed: bool,
}

/// Compares `actual` with `golden`. Images of different sizes are an error.
pub fn diff(actual: &RgbaImage, golden: &RgbaImage, tol: Tolerance) -> Result<DiffReport, String> {
    if actual.dimensions() != golden.dimensions() {
        return Err(format!(
            "size mismatch: actual {:?}, golden {:?}",
            actual.dimensions(),
            golden.dimensions()
        ));
    }
    let (mut total, mut differing) = (0u64, 0u64);
    for (a, g) in actual.pixels().zip(golden.pixels()) {
        let mut worst = 0;
        for c in 0..3 {
            let d = a.0[c].abs_diff(g.0[c]);
            total += u64::from(d);
            worst = worst.max(d);
        }
        if worst > tol.pixel {
            differing += 1;
        }
    }
    let n = f64::from(actual.width()) * f64::from(actual.height());
    let mean_error = total as f64 / (n * 3.0);
    let differing_fraction = differing as f64 / n;
    Ok(DiffReport {
        mean_error,
        differing_fraction,
        passed: mean_error <= tol.max_mean_error
            && differing_fraction <= tol.max_differing_fraction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn gradient(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 128, 255])
        })
    }

    #[test]
    fn uniform_image_fails_check() {
        let img = RgbaImage::from_pixel(64, 64, Rgba([90, 120, 200, 255]));
        assert!(check(&img).unwrap_err().contains("one colour"));
    }

    #[test]
    fn black_image_fails_check() {
        let mut img = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 255]));
        for x in 0..64 {
            img.put_pixel(x, x, Rgba([x as u8 % 3, 0, 0, 255]));
        }
        assert!(check(&img).is_err());
    }

    #[test]
    fn gradient_passes_check() {
        let s = check(&gradient(64, 64)).unwrap();
        assert!(s.luma_stddev > 10.0);
        assert!(s.dominant_fraction < 0.01);
    }

    #[test]
    fn identical_images_pass_diff() {
        let img = gradient(32, 32);
        let r = diff(&img, &img, Tolerance::default()).unwrap();
        assert_eq!(r.mean_error, 0.0);
        assert!(r.passed);
    }

    #[test]
    fn small_noise_is_tolerated_but_a_changed_region_is_not() {
        let golden = gradient(100, 100);
        let mut noisy = golden.clone();
        for p in noisy.pixels_mut() {
            p.0[0] = p.0[0].saturating_add(3);
        }
        assert!(diff(&noisy, &golden, Tolerance::default()).unwrap().passed);

        let mut broken = golden.clone();
        for y in 0..20 {
            for x in 0..20 {
                broken.put_pixel(x, y, Rgba([255, 0, 255, 255]));
            }
        }
        let r = diff(&broken, &golden, Tolerance::default()).unwrap();
        assert!(!r.passed);
        assert!((r.differing_fraction - 0.04).abs() < 1e-9);
    }

    #[test]
    fn size_mismatch_is_an_error() {
        assert!(diff(&gradient(8, 8), &gradient(8, 9), Tolerance::default()).is_err());
    }
}
