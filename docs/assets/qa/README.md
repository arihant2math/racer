# Car asset QA (`racer-assetqa`)

The automated accuracy gate from PLAN.md section 7: every generated car runs through it
before it ships, and a car that fails is regenerated or fixed by the pipeline. It is a Rust
CLI (`crates/racer-assetqa`) plus one headless Blender script
(`pipelines/blender/qa/review_render.py`).

```sh
tools/setup.sh --with-blender                      # Blender for gate 5, lavapipe + Xvfb for gate 6
cargo run -p racer-assetqa -- check porsche-911-carrera-s-992
# options: --glb PATH (default assets/cars/<id>/car.glb)  --out DIR (default target/assetqa/<id>)
#          --samples N (Cycles samples, default 48)  --no-review  --reuse-review  --no-smoke
#          --smoke-timeout SECS (default 900)
cargo run -p racer-assetqa -- refs porsche-911-carrera-s-992     # reference masks + sheet agreement
cargo run -p racer-assetqa -- fixture /tmp/fixtures              # the synthetic calibration cars
cargo test -p racer-assetqa                                      # calibration tests (~20 s)
```

`check` writes `report.md`, `report.json`, silhouette overlays, the review pack
(`review/`) and the smoke frame into the output directory. It exits 1 if any gate fails,
0 otherwise; the verdict is **PASS**, **FAIL**, or **REVIEW** (gates 1–4 and 6 pass and
the review pack waits for its reviewer). Gates 1–4 are pure Rust and deterministic; they
need no GPU, Blender or display.

## Asset contract

Units metres, +Y up, car facing −Z (right side +X), origin on the ground at the
wheelbase midpoint. Named LOD0 nodes `body`, `hood`, `bumper_front`, `bumper_rear`,
`door_l`/`door_r`, `mirror_l`/`mirror_r`, `spoiler`, `glass`, `lights_front`,
`lights_rear`, `interior`, `wheel_fl`/`fr`/`rl`/`rr` (origin at the hub centre). Empties
`socket_wheel_*`, `socket_light_*`, `socket_exhaust_l`/`r`, `socket_camera_hood`,
`socket_camera_cockpit`. Materials `car_paint`, `glass`, `rubber`, `rim`, `chrome`,
`light_emissive_front`, `light_emissive_rear`, `trim_black`, `interior`. LOD0 150–500k
triangles plus LOD1 and LOD2, a convex `collision` node, UVs on every render mesh.

How the checker reads a GLB: the default scene is flattened to world space. A node's LOD
is the `lodN` token in its own or an ancestor's name (`body_LOD1`, a parent `LOD2`), else
0; `_LODn` suffixes are stripped before matching required names. A node whose own or an
ancestor's name starts with `collision` is collision geometry. A part's geometry is its
subtree within the same LOD. Topology checks weld seam-split vertices by position (10 µm).

## Gates and thresholds

All numbers live in `crates/racer-assetqa/src/thresholds.rs`.

### 1. Structure and conventions

| Check | Rule |
|---|---|
| `nodes`, `sockets` | Every required node (LOD0) and socket present exactly once. `body.001`-style Blender duplicates are named in the failure. |
| `light_sockets` | ≥ 2 `socket_light_*`, at least one front (z < 0) and one rear. |
| `materials`, `material_assigned` | All nine materials exist; every primitive has a material. |
| `units` | LOD0 length 2.5–7 m (catches cm/mm/inch exports). |
| `up_axis` | Extents y < x < z (+Y up, length along Z). |
| `ground` | Lowest LOD0 point within ±3 cm of y = 0. |
| `mirrored` | No mesh node with a negative-determinant transform (flips winding). |
| `facing` | Front axle at lower z than the rear; left wheels at x < 0. |
| `origin` | Axle midpoint and wheel centreline within 2 cm of the origin. |
| `hub_origin` | Each wheel node's origin within 1 cm of its geometry's bounding-box centre. |
| `wheel_positions` | Each hub within 2 cm per axis of (±track/2, tyre radius, ±wheelbase/2) from car.toml. |
| `socket_wheels` | Wheel sockets within 2 cm of the hubs. |
| `placement` | Left/right parts (mirrors, doors, exhaust sockets) and front/rear parts (bumpers, lights) on the right side; camera sockets inside the car's bounds. |

### 2. Dimensions (vs `data/cars/<id>/car.toml`)

| Check | Tolerance |
|---|---|
| `length`, `width` (without mirror subtrees, as published), `height` | ±1.5 % |
| `wheelbase`, `track_front`, `track_rear` (from hub positions) | ±2 cm |
| `radius_*` (half the smaller of the wheel's Y and Z extents) vs the tyre size's unloaded radius | ±2 % |
| `tyre_width_*` (wheel X extent) vs section width | ±15 % (sidewall bulge, lettering) |

### 3. Mesh hygiene

| Check | Threshold |
|---|---|
| `lod0_budget` | 150 000 – 500 000 triangles |
| `lod1_ratio`, `lod2_ratio` | LOD(n+1)/LOD(n) between 0.1 and 0.6, LOD present |
| `lod1_silhouette`, `lod2_silhouette` | side silhouette IoU vs LOD0 ≥ 0.95 (1 cm grid) |
| `non_manifold` | ≤ 0.01 % of LOD0 edges shared by > 2 triangles |
| `closed_parts` | `collision` watertight (open edges ≤ 5 cm total), no non-manifold edges, positive volume (wound outwards). Render parts are legitimately open (panels split from the body, tyre beads inside the rim); openings you can see into are caught by `backfaces`. |
| `degenerate` | ≤ 0.1 % of LOD0 triangles collapsed or < 1e-8 m² |
| `loose` | ≤ 0.05 % of LOD0 triangles in pieces of ≤ 4 triangles |
| `normals_vs_winding` | ≤ 1 % of LOD0 area with stored normals opposite the winding |
| `backfaces` | ≤ 2 % of visible pixels show a single-sided back face, in each of the front, rear, left, right and top orthographic views (z-buffered, 1 cm pixels; double-sided materials exempt; the underside is reported only) |
| `uv_present` | every render primitive in every LOD has TEXCOORD_0 |
| `uv_degenerate` | ≤ 5 % of LOD0 area mapped to zero UV area |
| `uv_stretch` | ≤ 5 % of LOD0 area with σmax/σmin of the surface→UV map > 4 |
| `texel_density` | ≤ 5 % of LOD0 area with texel density outside ×¼…×4 of its material's area-weighted median |
| `collision_pieces` | 1 – 16 connected pieces |
| `collision_convex` | no hull vertex more than 5 mm outside a face plane of its own piece |
| `collision_triangles` | ≤ 1024 triangles per piece |
| `collision_coverage` | collision bounding box 0.85–1.05 × the LOD0 bounding box on every axis |

Reported only: open-edge length per part (bodies split into panels are legitimately open),
manifold edges with inconsistent winding.

### 4. Silhouette accuracy

The LOD0 triangles are rasterised orthographically (exact coverage at pixel centres) into
the pixel grid of each reference view, holes are filled (the gate compares outer outlines;
see-through gaps inside the silhouette, e.g. open panel seams, are reported as
`<sheet>:<view>:holes` in cm²), and compared with the reference silhouette:

| Metric | Threshold (per view) |
|---|---|
| IoU | ≥ 0.90 |
| mean outline distance (symmetric, exact Euclidean distance transform) | ≤ 20 mm |
| 95th-percentile outline distance | ≤ 60 mm |

References (`data/cars/<id>/refs/refs.toml`, sources and licences recorded there): sheets of
orthographic drawings. A view's silhouette is its ink (dark or saturated pixels) with every
region not reachable from the crop border filled, largest piece kept. Alignment:

* **scale** = car.toml wheelbase / pixel distance between the side view's axle centres,
  shared by all views on the sheet (the drawings are to one scale);
* **side**: axle midpoint = origin, ground line = y 0;
* **front/rear**: ground line = y 0, middle of the silhouette = centreline;
* **top**: middle = centreline, front-most pixel at the front-most z of the side view.

A sheet with `role = "crosscheck"` is reported but not gated. `racer-assetqa refs <id>`
prints each view's measured size and how well two sheets agree; for the 911 the two
independent drawings agree to IoU …, which is why the thresholds are not tighter.
Overlays (`silhouette-<sheet>-<view>.png`): dark grey both, red model only, blue reference
only.

### 5. Visual review pack

`review_render.py` imports the GLB in Blender, hides LOD1+ and collision, and renders with
Cycles (CPU, OIDN denoise) in a neutral studio: front and rear three-quarter, low front,
orthographic side/front/rear/top, and close-ups of the front and rear left wheels, front and
rear lights and the interior through the side window. The checker then builds side-by-sides
(render left, reference right) with every blueprint view and every reference photo
(`[[photo]]` in refs.toml). The gate fails only if renders are missing; otherwise it is
REVIEW until a multimodal reviewer scores the pack with the rubric (`review/rubric.md`):

1. Proportions, 2. Front signature, 3. Rear signature, 4. Wheels, 5. Glass and openings,
6. Surface quality, 7. Materials, 8. Overall likeness, each 0–2. Pass: no 0 and ≥ 12/16.

### 6. In-engine smoke test

`racer-assetqa smoke` (spawned by `check` under `xvfb-run`, lavapipe forced as in
`tools/capture.sh`) loads the GLB with Bevy 0.19's glTF loader, renders it in a lit scene,
waits for the asset and all its dependencies to load and for the pipeline cache to settle
for 10 frames, and saves `smoke.png`. Pass: loads without error, no `ERROR` lines or panics
in the log, and the car covers ≥ 2 % of the frame with luma stddev ≥ 4.

## Calibration

`cargo test -p racer-assetqa` builds a box car to the 911's car.toml (body + cabin, all
parts, sockets and materials, ~200k-triangle LOD0, LOD1/LOD2, convex collision) and nine
broken variants, and draws synthetic blueprints of the good car analytically (not with the
rasteriser under test). The good car passes gates 1–4 with silhouette IoU > 0.98; each
variant fails the checks for its fault:

| Variant | Caught by |
|---|---|
| wrong-scale (cm) | `units`, `ground`, `length`/`width`/`height`/`wheelbase`, silhouettes |
| missing-wheels (rear) | `nodes`, `sockets`, `facing`, `wheelbase`, side silhouette |
| flipped-normals (body inside out) | `backfaces` |
| non-manifold-body (internal wall) | `non_manifold` |
| wrong-orientation (facing +Z) | `facing`, `placement`, side and top silhouettes |
| z-up | `up_axis`, `ground`, `height` |
| no-uvs | `uv_present` |
| concave-collision | `collision_convex` |
| stretched (+8 % length) | `length`, `wheelbase`, side and top silhouettes (front passes) |

Further tests cover the reference-image path (a synthetic sheet PNG through refs.toml) and
the committed 911 sheets (real-size extents; the two sheets agree to IoU > 0.9).
