//! Every number a gate compares against, with its rationale. Documented in
//! `docs/assets/qa/README.md`; keep the two in sync.

/// Gate thresholds. `Default` is the shipping configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct Thresholds {
    // 1. Structure
    /// Plausible overall length range (m); outside it the asset is in the wrong
    /// units (cm, mm, inches) or badly scaled.
    pub units_length: (f32, f32),
    /// Lowest point of the LOD0 geometry must be on the ground (|y| ≤ this, m).
    pub ground: f32,
    /// Wheelbase midpoint / centreline offset of the origin (m).
    pub origin: f32,
    /// Wheel node origin vs centre of its geometry's bounding box (m).
    pub hub_origin: f32,
    /// Wheel hub vs the position implied by car.toml (wheelbase, track,
    /// tyre radius) (m).
    pub wheel_position: f32,
    /// Wheel socket vs its wheel hub (m).
    pub socket_wheel: f32,
    /// Minimum number of `socket_light_*` empties (at least one front, one rear).
    pub min_light_sockets: usize,

    // 2. Dimensions
    /// Relative tolerance on length, width (without mirrors) and height.
    pub dimension_rel: f32,
    /// Absolute tolerance on wheelbase and tracks (m).
    pub axle_abs: f32,
    /// Relative tolerance on wheel (tyre) radius.
    pub wheel_radius_rel: f32,
    /// Relative tolerance on tyre width (the mesh's wheel width includes
    /// sidewall bulge and lettering, so this is loose).
    pub tyre_width_rel: f32,

    // 3. Mesh hygiene
    /// LOD0 triangle budget (inclusive).
    pub lod0_triangles: (usize, usize),
    /// Allowed triangle ratio LOD(n+1)/LOD(n).
    pub lod_ratio: (f32, f32),
    /// Minimum side-silhouette IoU of LOD1 and LOD2 against LOD0.
    pub lod_silhouette_iou: f32,
    /// Maximum fraction of LOD0 edges shared by more than two triangles.
    pub non_manifold_fraction: f32,
    /// Parts (base node names) that must be closed (watertight).
    pub closed_parts: Vec<String>,
    /// Maximum total open-boundary length on a closed part (m).
    pub closed_boundary_length: f32,
    /// Triangles below this area (m²) are degenerate.
    pub degenerate_area: f32,
    /// Maximum fraction of degenerate LOD0 triangles.
    pub degenerate_fraction: f32,
    /// Maximum area fraction of triangles whose winding disagrees with their
    /// stored vertex normals.
    pub normal_mismatch_fraction: f32,
    /// Maximum fraction of visible pixels showing a back face, per view
    /// (front, rear, both sides, top; the underside is reported only).
    pub backface_fraction: f32,
    /// Connected pieces with at most this many triangles count as loose.
    pub loose_max_triangles: usize,
    /// Maximum fraction of LOD0 triangles in loose pieces.
    pub loose_fraction: f32,
    /// Maximum area fraction with a texel density outside [median / r, median · r]
    /// of its material.
    pub texel_density_ratio: f32,
    pub texel_outlier_fraction: f32,
    /// Maximum area fraction of UV-mapped triangles whose anisotropic stretch
    /// (σmax / σmin of the surface→UV map) exceeds `uv_stretch`.
    pub uv_stretch: f32,
    pub uv_stretch_fraction: f32,
    /// Collision: maximum convex pieces, triangles per piece, and the distance
    /// a vertex may lie outside a face plane of its own piece (m).
    pub collision_max_pieces: usize,
    pub collision_max_triangles: usize,
    pub collision_convex_tolerance: f32,
    /// Collision bounding box vs LOD0 bounding box, per axis.
    pub collision_coverage: (f32, f32),

    // 4. Silhouettes
    /// Minimum IoU against the reference silhouette, per view.
    pub silhouette_iou: f32,
    /// Maximum mean and 95th-percentile outline distance (m).
    pub silhouette_mean_distance: f32,
    pub silhouette_p95_distance: f32,

    // 6. In-engine smoke
    /// Minimum fraction of the smoke-test frame covered by the car.
    pub smoke_min_coverage: f32,
}

impl Default for Thresholds {
    fn default() -> Thresholds {
        Thresholds {
            units_length: (2.5, 7.0),
            ground: 0.03,
            origin: 0.02,
            hub_origin: 0.01,
            wheel_position: 0.02,
            socket_wheel: 0.02,
            min_light_sockets: 2,

            dimension_rel: 0.015,
            axle_abs: 0.02,
            wheel_radius_rel: 0.02,
            tyre_width_rel: 0.15,

            lod0_triangles: (150_000, 500_000),
            lod_ratio: (0.1, 0.6),
            lod_silhouette_iou: 0.95,
            non_manifold_fraction: 1e-4,
            closed_parts: ["collision", "wheel_fl", "wheel_fr", "wheel_rl", "wheel_rr"]
                .map(String::from)
                .to_vec(),
            closed_boundary_length: 0.05,
            degenerate_area: 1e-8,
            degenerate_fraction: 1e-3,
            normal_mismatch_fraction: 0.01,
            backface_fraction: 0.02,
            loose_max_triangles: 4,
            loose_fraction: 5e-4,
            texel_density_ratio: 4.0,
            texel_outlier_fraction: 0.05,
            uv_stretch: 4.0,
            uv_stretch_fraction: 0.05,
            collision_max_pieces: 16,
            collision_max_triangles: 1024,
            collision_convex_tolerance: 0.005,
            collision_coverage: (0.85, 1.05),

            silhouette_iou: 0.90,
            silhouette_mean_distance: 0.02,
            silhouette_p95_distance: 0.06,

            smoke_min_coverage: 0.02,
        }
    }
}
