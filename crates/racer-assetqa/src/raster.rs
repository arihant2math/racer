//! Orthographic rasterisation of triangle geometry into binary masks and
//! depth/facing buffers, plus the mask metrics (IoU, boundary distance).

use glam::{Vec2, Vec3};
use image::{GrayImage, Luma, Rgb, RgbImage};

/// Orthographic view directions, named from where the camera stands.
///
/// Every view maps a world point to `(u, v)` metres on the image plane (u to
/// the right, v up) and a depth along the viewing direction (smaller is
/// closer to the camera). Car convention: +Y up, front −Z, right +X.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    /// Camera on the car's left (−X) looking +X: front of the car on the left.
    Side,
    /// Camera on the car's right (+X) looking −X.
    SideRight,
    /// Camera ahead of the car (−Z) looking +Z.
    Front,
    /// Camera behind the car (+Z) looking −Z.
    Rear,
    /// Camera above looking down: front on the left, car's right side up.
    Top,
    /// Camera below looking up.
    Bottom,
}

impl View {
    pub const ALL: [View; 6] = [
        View::Side,
        View::SideRight,
        View::Front,
        View::Rear,
        View::Top,
        View::Bottom,
    ];

    pub fn name(self) -> &'static str {
        match self {
            View::Side => "side",
            View::SideRight => "side-right",
            View::Front => "front",
            View::Rear => "rear",
            View::Top => "top",
            View::Bottom => "bottom",
        }
    }

    pub fn parse(s: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.name() == s)
    }

    /// Direction the camera looks along.
    pub fn dir(self) -> Vec3 {
        match self {
            View::Side => Vec3::X,
            View::SideRight => Vec3::NEG_X,
            View::Front => Vec3::Z,
            View::Rear => Vec3::NEG_Z,
            View::Top => Vec3::NEG_Y,
            View::Bottom => Vec3::Y,
        }
    }

    /// `(u, v, depth)` of a world point.
    pub fn project(self, p: Vec3) -> Vec3 {
        match self {
            View::Side => Vec3::new(p.z, p.y, p.x),
            View::SideRight => Vec3::new(-p.z, p.y, -p.x),
            View::Front => Vec3::new(-p.x, p.y, p.z),
            View::Rear => Vec3::new(p.x, p.y, -p.z),
            View::Top => Vec3::new(p.z, p.x, -p.y),
            View::Bottom => Vec3::new(p.z, -p.x, p.y),
        }
    }
}

/// Maps view-plane metres to pixels: `px = ox ± u / mpp`, `py = oy − v / mpp`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub width: u32,
    pub height: u32,
    /// Pixel position (pixel corners, x right, y down) of `u = v = 0`.
    pub ox: f32,
    pub oy: f32,
    /// Metres per pixel.
    pub mpp: f32,
    /// Mirror u (the image shows the car's front on the other side).
    pub flip_u: bool,
}

impl Grid {
    /// A grid at `mpp` covering `min..max` (view-plane metres) plus a margin.
    pub fn fit(min: Vec2, max: Vec2, mpp: f32, margin_px: u32) -> Grid {
        let m = margin_px as f32;
        let width = ((max.x - min.x) / mpp).ceil().max(1.0) as u32 + 2 * margin_px;
        let height = ((max.y - min.y) / mpp).ceil().max(1.0) as u32 + 2 * margin_px;
        Grid {
            width,
            height,
            ox: m - min.x / mpp,
            oy: m + max.y / mpp,
            mpp,
            flip_u: false,
        }
    }

    pub fn to_px(&self, u: f32, v: f32) -> Vec2 {
        let du = if self.flip_u { -u } else { u };
        Vec2::new(self.ox + du / self.mpp, self.oy - v / self.mpp)
    }

    pub fn to_metric(&self, px: f32, py: f32) -> Vec2 {
        let du = (px - self.ox) * self.mpp;
        Vec2::new(if self.flip_u { -du } else { du }, (self.oy - py) * self.mpp)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<bool>,
}

impl Mask {
    pub fn new(width: u32, height: u32) -> Mask {
        Mask {
            width,
            height,
            data: vec![false; (width * height) as usize],
        }
    }

    pub fn get(&self, x: i64, y: i64) -> bool {
        x >= 0
            && y >= 0
            && x < self.width as i64
            && y < self.height as i64
            && self.data[(y * self.width as i64 + x) as usize]
    }

    pub fn set(&mut self, x: u32, y: u32, v: bool) {
        self.data[(y * self.width + x) as usize] = v;
    }

    pub fn count(&self) -> usize {
        self.data.iter().filter(|&&b| b).count()
    }

    /// Pixel bounding box `(x0, y0, x1, y1)`, inclusive.
    pub fn bbox(&self) -> Option<(u32, u32, u32, u32)> {
        let mut b: Option<(u32, u32, u32, u32)> = None;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.data[(y * self.width + x) as usize] {
                    b = Some(match b {
                        None => (x, y, x, y),
                        Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                    });
                }
            }
        }
        b
    }

    pub fn to_image(&self) -> GrayImage {
        GrayImage::from_fn(self.width, self.height, |x, y| {
            Luma([if self.data[(y * self.width + x) as usize] { 0 } else { 255 }])
        })
    }

    /// Pixels in the mask with a 4-neighbour outside it.
    pub fn boundary(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                if !self.data[(y * self.width + x) as usize] {
                    continue;
                }
                let (xi, yi) = (x as i64, y as i64);
                if !self.get(xi - 1, yi)
                    || !self.get(xi + 1, yi)
                    || !self.get(xi, yi - 1)
                    || !self.get(xi, yi + 1)
                {
                    out.push((x, y));
                }
            }
        }
        out
    }
}

/// Calls `f(x, y, depth)` for every pixel whose centre lies inside the
/// projected triangle.
fn raster_triangle(grid: &Grid, view: View, tri: [Vec3; 3], mut f: impl FnMut(u32, u32, f32)) {
    let p = tri.map(|v| {
        let q = view.project(v);
        let px = grid.to_px(q.x, q.y);
        Vec3::new(px.x, px.y, q.z)
    });
    let area = (p[1].x - p[0].x) * (p[2].y - p[0].y) - (p[2].x - p[0].x) * (p[1].y - p[0].y);
    if area.abs() < 1e-12 {
        return;
    }
    let min_x = p.iter().map(|v| v.x).fold(f32::INFINITY, f32::min);
    let max_x = p.iter().map(|v| v.x).fold(f32::NEG_INFINITY, f32::max);
    let min_y = p.iter().map(|v| v.y).fold(f32::INFINITY, f32::min);
    let max_y = p.iter().map(|v| v.y).fold(f32::NEG_INFINITY, f32::max);
    let x0 = (min_x - 0.5).ceil().max(0.0) as i64;
    let x1 = ((max_x - 0.5).floor() as i64).min(grid.width as i64 - 1);
    let y0 = (min_y - 0.5).ceil().max(0.0) as i64;
    let y1 = ((max_y - 0.5).floor() as i64).min(grid.height as i64 - 1);
    let inv = 1.0 / area;
    for y in y0..=y1 {
        let cy = y as f32 + 0.5;
        for x in x0..=x1 {
            let cx = x as f32 + 0.5;
            let w0 = ((p[1].x - cx) * (p[2].y - cy) - (p[2].x - cx) * (p[1].y - cy)) * inv;
            let w1 = ((p[2].x - cx) * (p[0].y - cy) - (p[0].x - cx) * (p[2].y - cy)) * inv;
            let w2 = 1.0 - w0 - w1;
            if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                f(x as u32, y as u32, w0 * p[0].z + w1 * p[1].z + w2 * p[2].z);
            }
        }
    }
}

/// Silhouette of the triangles in `grid`.
pub fn silhouette(
    grid: &Grid,
    view: View,
    tris: impl IntoIterator<Item = [Vec3; 3]>,
) -> Mask {
    let mut mask = Mask::new(grid.width, grid.height);
    for t in tris {
        raster_triangle(grid, view, t, |x, y, _| mask.set(x, y, true));
    }
    mask
}

/// Front-most surface per pixel: `Some(true)` if it faces the camera (by its
/// winding), `Some(false)` if it is a back face, `None` if empty.
pub fn facing(
    grid: &Grid,
    view: View,
    tris: impl IntoIterator<Item = [Vec3; 3]>,
) -> Vec<Option<bool>> {
    let n = (grid.width * grid.height) as usize;
    let mut depth = vec![f32::INFINITY; n];
    let mut front = vec![None; n];
    let dir = view.dir();
    for t in tris {
        let normal = (t[1] - t[0]).cross(t[2] - t[0]);
        if normal.length_squared() < 1e-20 {
            continue;
        }
        let faces = normal.dot(dir) < 0.0;
        raster_triangle(grid, view, t, |x, y, d| {
            let i = (y * grid.width + x) as usize;
            if d < depth[i] {
                depth[i] = d;
                front[i] = Some(faces);
            }
        });
    }
    front
}

/// Intersection over union.
pub fn iou(a: &Mask, b: &Mask) -> f32 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let (mut inter, mut union) = (0usize, 0usize);
    for (&x, &y) in a.data.iter().zip(&b.data) {
        inter += (x && y) as usize;
        union += (x || y) as usize;
    }
    if union == 0 {
        1.0
    } else {
        inter as f32 / union as f32
    }
}

/// Squared Euclidean distance transform (Felzenszwalb & Huttenlocher) of the
/// given seed pixels.
fn edt(width: u32, height: u32, seeds: &[(u32, u32)]) -> Vec<f32> {
    let (w, h) = (width as usize, height as usize);
    const INF: f32 = 1e20;
    let mut grid = vec![INF; w * h];
    for &(x, y) in seeds {
        grid[y as usize * w + x as usize] = 0.0;
    }
    let mut f = vec![0.0f32; w.max(h)];
    let mut d = vec![0.0f32; w.max(h)];
    let mut v = vec![0usize; w.max(h)];
    let mut z = vec![0.0f32; w.max(h) + 1];
    let mut pass = |len: usize, get: &dyn Fn(usize) -> usize, grid: &mut Vec<f32>| {
        for (q, fq) in f.iter_mut().enumerate().take(len) {
            *fq = grid[get(q)];
        }
        let mut k = 0usize;
        v[0] = 0;
        z[0] = -INF;
        z[1] = INF;
        for q in 1..len {
            let parabola = |p: usize| {
                ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32))
                    / (2.0 * q as f32 - 2.0 * p as f32)
            };
            // z[0] is -INF, so this stops at k = 0 at the latest.
            let mut s = parabola(v[k]);
            while s <= z[k] {
                k -= 1;
                s = parabola(v[k]);
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = INF;
        }
        k = 0;
        for (q, dq) in d.iter_mut().enumerate().take(len) {
            while z[k + 1] < q as f32 {
                k += 1;
            }
            let p = v[k];
            *dq = (q as f32 - p as f32).powi(2) + f[p];
        }
        for (q, dq) in d.iter().enumerate().take(len) {
            grid[get(q)] = *dq;
        }
    };
    for x in 0..w {
        pass(h, &|q| q * w + x, &mut grid);
    }
    for y in 0..h {
        pass(w, &|q| y * w + q, &mut grid);
    }
    grid
}

/// Distances between the outlines of two masks, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundaryDistance {
    pub mean: f32,
    pub p95: f32,
    pub max: f32,
}

/// Symmetric outline distance: for every boundary pixel of either mask, the
/// distance to the nearest boundary pixel of the other.
pub fn boundary_distance(a: &Mask, b: &Mask) -> BoundaryDistance {
    let (ba, bb) = (a.boundary(), b.boundary());
    if ba.is_empty() || bb.is_empty() {
        let inf = f32::INFINITY;
        return BoundaryDistance {
            mean: inf,
            p95: inf,
            max: inf,
        };
    }
    let (da, db) = (edt(a.width, a.height, &ba), edt(b.width, b.height, &bb));
    let mut d: Vec<f32> = ba
        .iter()
        .map(|&(x, y)| db[(y * b.width + x) as usize].sqrt())
        .chain(bb.iter().map(|&(x, y)| da[(y * a.width + x) as usize].sqrt()))
        .collect();
    d.sort_by(f32::total_cmp);
    BoundaryDistance {
        mean: d.iter().sum::<f32>() / d.len() as f32,
        p95: d[((d.len() - 1) as f32 * 0.95) as usize],
        max: d[d.len() - 1],
    }
}

/// Overlay of a model mask on a reference mask: both = dark grey, model only
/// = red, reference only = blue.
pub fn overlay(model: &Mask, reference: &Mask) -> RgbImage {
    RgbImage::from_fn(model.width, model.height, |x, y| {
        let i = (y * model.width + x) as usize;
        Rgb(match (model.data[i], reference.data[i]) {
            (true, true) => [90, 90, 90],
            (true, false) => [230, 40, 40],
            (false, true) => [40, 90, 230],
            (false, false) => [255, 255, 255],
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(grid: &Grid, view: View, min: Vec3, max: Vec3) -> Mask {
        let a = Vec3::new(min.x, min.y, min.z);
        let b = Vec3::new(min.x, max.y, min.z);
        let c = Vec3::new(min.x, max.y, max.z);
        let d = Vec3::new(min.x, min.y, max.z);
        silhouette(grid, view, [[a, b, c], [a, c, d]])
    }

    #[test]
    fn side_view_square_covers_expected_pixels() {
        let grid = Grid::fit(Vec2::new(-1.0, 0.0), Vec2::new(1.0, 1.0), 0.01, 5);
        let m = square(
            &grid,
            View::Side,
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, 1.0, 1.0),
        );
        assert_eq!(m.count(), 200 * 100);
        assert_eq!(m.bbox(), Some((5, 5, 204, 104)));
        let p = grid.to_metric(5.0, 105.0);
        assert!((p - Vec2::new(-1.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn iou_and_boundary_distance() {
        let grid = Grid::fit(Vec2::new(-1.0, 0.0), Vec2::new(1.0, 1.0), 0.01, 5);
        let a = square(&grid, View::Side, Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 1.0, 1.0));
        let b = square(&grid, View::Side, Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 0.9, 1.0));
        assert!((iou(&a, &b) - 0.9).abs() < 1e-3);
        assert_eq!(iou(&a, &a), 1.0);
        let d = boundary_distance(&a, &a);
        assert_eq!((d.mean, d.max), (0.0, 0.0));
        let d = boundary_distance(&a, &b);
        // Only the top edges differ, by 10 px.
        assert!((d.max - 10.0).abs() < 1e-3, "{d:?}");
        assert!(d.mean > 0.5 && d.mean < 5.0, "{d:?}");
    }

    #[test]
    fn edt_is_exact() {
        let d = edt(7, 5, &[(1, 1), (6, 4)]);
        assert_eq!(d[0], 2.0);
        assert_eq!(d[3 * 7 + 4], 5.0);
        assert_eq!(d[4 * 7 + 6], 0.0);
        assert_eq!(d[4 * 7 + 1], 9.0);
    }

    #[test]
    fn facing_detects_back_faces() {
        let grid = Grid::fit(Vec2::new(-1.0, 0.0), Vec2::new(1.0, 1.0), 0.05, 0);
        let a = Vec3::new(0.0, 0.0, -1.0);
        let b = Vec3::new(0.0, 1.0, -1.0);
        let c = Vec3::new(0.0, 1.0, 1.0);
        // Normal of (a, b, c) is (b - a) x (c - a) = +Y x ... = (2, 0, 0)·sign.
        let n = (b - a).cross(c - a);
        let f = facing(&grid, View::Side, [[a, b, c]]);
        let any = f.iter().flatten().next().copied().unwrap();
        assert_eq!(any, n.dot(View::Side.dir()) < 0.0);
    }
}
