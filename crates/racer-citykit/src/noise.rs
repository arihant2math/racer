//! Deterministic, tileable noise for procedural textures and placement.
//!
//! Everything here is a pure function of its inputs and a `u32` seed, so the
//! generated textures and layouts are bit-identical across runs and platforms.

/// 32-bit integer hash (a variant of the "lowbias32" finaliser).
pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// Hash of a lattice point and a seed.
pub fn hash3(x: i32, y: i32, seed: u32) -> u32 {
    hash((x as u32).wrapping_mul(0x9e37_79b9) ^ hash((y as u32) ^ hash(seed)))
}

/// Uniform float in `[0, 1)` from a hash.
pub fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn wrap(i: i32, period: i32) -> i32 {
    if period > 0 { i.rem_euclid(period) } else { i }
}

/// Value noise in `[0, 1)` on an integer lattice, periodic with `period`
/// lattice cells in both axes when `period > 0`.
pub fn value(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (smooth(x - xi as f32), smooth(y - yi as f32));
    let v = |dx: i32, dy: i32| unit(hash3(wrap(xi + dx, period), wrap(yi + dy, period), seed));
    let a = v(0, 0) + (v(1, 0) - v(0, 0)) * fx;
    let b = v(0, 1) + (v(1, 1) - v(0, 1)) * fx;
    a + (b - a) * fy
}

/// Fractal value noise in `[0, 1)`, periodic over `period` base cells.
pub fn fbm(x: f32, y: f32, period: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut freq) = (0.0, 1.0, 0.0, 1);
    for o in 0..octaves {
        let f = freq as f32;
        sum += amp * value(x * f, y * f, period * freq, seed.wrapping_add(o * 7919));
        norm += amp;
        amp *= 0.5;
        freq *= 2;
    }
    sum / norm
}

/// Cellular (Worley) noise: distance to the nearest and second-nearest
/// feature point, plus the hash of the nearest cell. Periodic like [`value`].
pub fn worley(x: f32, y: f32, period: i32, seed: u32) -> (f32, f32, u32) {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (mut d1, mut d2, mut id) = (f32::MAX, f32::MAX, 0);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (xi + dx, yi + dy);
            let h = hash3(wrap(cx, period), wrap(cy, period), seed);
            let px = cx as f32 + unit(h);
            let py = cy as f32 + unit(hash(h));
            let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
                id = h;
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2, id)
}

/// Small deterministic random number generator (PCG-like) for placement.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x853c_49e6_748f_ea9b)
    }

    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let x = (((self.0 >> 18) ^ self.0) >> 27) as u32;
        x.rotate_right((self.0 >> 59) as u32)
    }

    /// Uniform in `[0, 1)`.
    pub fn f32(&mut self) -> f32 {
        unit(self.next_u32())
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    /// Uniform index in `0..n` (`n > 0`).
    pub fn index(&mut self, n: usize) -> usize {
        (self.next_u32() as usize) % n.max(1)
    }

    /// Picks an index with the given (non-negative) weights.
    pub fn weighted(&mut self, weights: &[f32]) -> usize {
        let total: f32 = weights.iter().sum();
        let mut r = self.f32() * total;
        for (i, w) in weights.iter().enumerate() {
            if r < *w {
                return i;
            }
            r -= w;
        }
        weights.len().saturating_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_noise_is_periodic_and_bounded() {
        for i in 0..200 {
            let (x, y) = (i as f32 * 0.37, i as f32 * 0.91);
            let a = value(x, y, 8, 3);
            assert!((0.0..1.0).contains(&a));
            assert!((a - value(x + 8.0, y - 16.0, 8, 3)).abs() < 1e-5);
            let f = fbm(x, y, 4, 5, 9);
            assert!((f - fbm(x + 4.0, y + 4.0, 4, 5, 9)).abs() < 1e-5);
        }
    }

    #[test]
    fn worley_is_periodic() {
        let a = worley(1.3, 2.7, 5, 1);
        let b = worley(6.3, -2.3, 5, 1);
        assert!((a.0 - b.0).abs() < 1e-4 && a.2 == b.2);
    }

    #[test]
    fn rng_is_deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        let xs: Vec<u32> = (0..10).map(|_| a.next_u32()).collect();
        let ys: Vec<u32> = (0..10).map(|_| b.next_u32()).collect();
        assert_eq!(xs, ys);
        assert_ne!(Rng::new(43).next_u32(), xs[0]);
    }
}
