# Asset QA: Porsche 911 Carrera S (992) (`porsche-911-carrera-s-992`)

Asset `assets/cars/porsche-911-carrera-s-992/car.glb + assets/cars/porsche-911-carrera-s-992/car_lod1.glb + assets/cars/porsche-911-carrera-s-992/car_lod2.glb` (13.2 MB). **Verdict: FAIL**

| Gate | Result | Failed checks |
|---|---|---|
| 1. Structure and conventions | PASS |  |
| 2. Dimensions | PASS |  |
| 3. Mesh hygiene | FAIL | degenerate, uv_stretch, texel_density, collision_convex, collision_ground, collision_coverage |
| 4. Silhouette accuracy | FAIL | dimensions-com:top |
| 5. Visual review pack | REVIEW |  |
| 6. In-engine smoke test (Bevy, lavapipe) | PASS |  |

## 1. Structure and conventions: PASS

| Check | What | Value | Threshold | Result |
|---|---|---|---|---|
| `nodes` | required LOD0 nodes present once | all 17 present | all present, unique | PASS |
| `sockets` | required socket empties present once | all 8 present | all present, unique | PASS |
| `light_sockets` | socket_light_* empties (front = z < 0) | 7 (2 front, 5 rear): socket_light_brake, socket_light_headlight_l, socket_light_headlight_r, socket_light_reverse_l, socket_light_reverse_r, socket_light_tail_l, socket_light_tail_r | ≥ 2, ≥ 1 front and rear | PASS |
| `materials` | required materials present | all present | car_paint, glass, rubber, rim, chrome, light_emissive_front, light_emissive_rear, trim_black, interior | PASS |
| `materials_extra` | other materials | brake_caliper |  | info |
| `material_assigned` | render primitives without a material (collision exempt) | 0 | 0 | PASS |
| `units` | LOD0 length (Z extent) in metres | 4.519 m | 2.5 – 7 m | PASS |
| `up_axis` | +Y up: height is the smallest extent, length (Z) the largest | x 1.990, y 1.303, z 4.519 m | y < x < z | PASS |
| `ground` | lowest LOD0 point on the ground (y = 0) | 0.000 m | \|y\| ≤ 0.03 | PASS |
| `mirrored` | mesh nodes with a mirroring (negative determinant) transform | none | none | PASS |
| `facing` | front wheels at −Z, left wheels at −X | front axle z -1.225, rear 1.225; left x -0.794/-0.779, right 0.794/0.779 | front z < rear z, left x < 0 < right x | PASS |
| `origin` | origin at the wheelbase midpoint on the centreline | axle midpoint z 0.000, wheel centre x 0.000 | \|z\|, \|x\| ≤ 0.02 | PASS |
| `hub_origin` | wheel node origin at the centre of its geometry | worst wheel_rr: 0.0000 m | ≤ 0.01 | PASS |
| `wheel_positions` | wheel hubs vs car.toml wheelbase, tracks and tyre radius | fl Δ(0.000, 0.000, 0.000); fr Δ(0.000, 0.000, 0.000); rl Δ(0.000, 0.000, 0.000); rr Δ(0.000, 0.000, 0.000) | each axis ≤ 0.02 | PASS |
| `socket_wheels` | socket_wheel_* at the wheel hubs | worst rr: 0.0000 m | ≤ 0.02 | PASS |
| `placement` | left/right and front/rear parts and sockets on the right side | all consistent | consistent with +Y up, −Z forward, +X right | PASS |


## 2. Dimensions: PASS

| Check | What | Value | Threshold | Result |
|---|---|---|---|---|
| `length` | overall length (Z extent) | 4.519 m (+0.00 %) | 4.519 m ± 1.5 % | PASS |
| `width` | width without mirrors (X extent) | 1.862 m (+0.54 %) | 1.852 m ± 1.5 % | PASS |
| `height` | height (ground to roof) | 1.303 m (+0.24 %) | 1.300 m ± 1.5 % | PASS |
| `width_mirrors` | width with mirrors | 1.990 m |  | info |
| `wheelbase` | wheelbase (hub Z distance) | 2.450 m (+0.000) | 2.450 ± 0.02 m | PASS |
| `track_front` | front track (hub X distance) | 1.589 m (+0.000) | 1.589 ± 0.02 m | PASS |
| `track_rear` | rear track (hub X distance) | 1.557 m (+0.000) | 1.557 ± 0.02 m | PASS |
| `radius_fl` | wheel_fl radius | 0.3397 m (-0.00 %) | 0.3397 m ± 2 % | PASS |
| `tyre_width_fl` | wheel_fl width (X extent) | 0.245 m (+0.0 %) | 0.245 m ± 15 % | PASS |
| `radius_fr` | wheel_fr radius | 0.3397 m (-0.00 %) | 0.3397 m ± 2 % | PASS |
| `tyre_width_fr` | wheel_fr width (X extent) | 0.245 m (+0.0 %) | 0.245 m ± 15 % | PASS |
| `radius_rl` | wheel_rl radius | 0.3582 m (-0.00 %) | 0.3582 m ± 2 % | PASS |
| `tyre_width_rl` | wheel_rl width (X extent) | 0.305 m (-0.0 %) | 0.305 m ± 15 % | PASS |
| `radius_rr` | wheel_rr radius | 0.3582 m (-0.00 %) | 0.3582 m ± 2 % | PASS |
| `tyre_width_rr` | wheel_rr width (X extent) | 0.305 m (-0.0 %) | 0.305 m ± 15 % | PASS |


## 3. Mesh hygiene: FAIL

| Check | What | Value | Threshold | Result |
|---|---|---|---|---|
| `lod0_budget` | LOD0 triangles | 363874 | 150000 – 500000 | PASS |
| `lod1_ratio` | LOD1 triangles / LOD0 | 109148 (0.300) | present, 0.1 – 0.6 | PASS |
| `lod1_silhouette` | LOD1 side silhouette IoU vs LOD0 | 1.0000 | ≥ 0.95 | PASS |
| `lod2_ratio` | LOD2 triangles / LOD1 | 37271 (0.341) | present, 0.1 – 0.6 | PASS |
| `lod2_silhouette` | LOD2 side silhouette IoU vs LOD0 | 0.9984 | ≥ 0.95 | PASS |
| `non_manifold` | LOD0 edges shared by > 2 triangles | 0 of 550314 (0.000 %)  | ≤ 0.010 % | PASS |
| `open_edges` | LOD0 open (boundary) edges, all parts | 11298 edges, 160.54 m [body 63.46 m, brake_fl 0.63 m, brake_fr 0.63 m, brake_rl 0.63 m, brake_rr 0.63 m, bumper_front 5.91 m, bumper_rear 6.12 m, door_l 4.73 m, …] |  | info |
| `winding_inconsistent` | LOD0 manifold edges whose triangles disagree on winding | 618 [body 442, bumper_rear 80, lights_front 96] |  | info |
| `closed_parts` | closed parts watertight and wound outwards (positive volume) | collision: open 0.000 m, non-manifold 0, volume +6.5848 m³ | open edges ≤ 0.05 m, 0 non-manifold, volume > 0 | PASS |
| `degenerate` | LOD0 triangles with area < 1e-8 m² or collapsed | 832 (0.229 %) [lights_front 432, mirror_l 80, mirror_r 80, interior 77, brake_fl 32, brake_fr 32, brake_rl 32, brake_rr 32, …] | ≤ 0.100 % | FAIL |
| `loose` | LOD0 triangles in pieces of ≤ 4 triangles | 0 (0.000 %)  | ≤ 0.050 % | PASS |
| `normals_vs_winding` | LOD0 area whose stored normals point against the winding | 0.005 % | ≤ 1.000 % | PASS |
| `backfaces` | visible pixels showing a single-sided back face (worst of front, rear, sides, top; bottom reported) | front 0.000 %, rear 0.000 %, side 0.000 %, side-right 0.000 %, top 0.000 %, bottom 0.000 % | ≤ 2.000 % | PASS |
| `uv_present` | render primitives (all LODs) with TEXCOORD_0 | all | all | PASS |
| `uv_degenerate` | LOD0 area mapped to zero UV area | 2.088 % | ≤ 5.000 % | PASS |
| `uv_stretch` | LOD0 area with UV stretch σmax/σmin > 4 | 12.333 % largest parts, % of LOD0 area: [body 6.4, bumper_front 1.4, bumper_rear 0.9, wheel_rl 0.6, wheel_rr 0.6, wheel_fl 0.5, wheel_fr 0.5, lights_front 0.5, …] | ≤ 5.000 % | FAIL |
| `texel_density` | LOD0 area whose texel density is outside ×1/4…×4 of its material's median | 11.021 % largest parts, % of LOD0 area: [interior 5.6, body 1.7, brake_rr 0.5, brake_fr 0.5, bumper_front 0.3, wheel_rl 0.3, wheel_fl 0.3, wheel_fr 0.3, …] (medians, UV units per metre: brake_caliper 2.000/m, car_paint 0.199/m, chrome 4.320/m, glass 0.196/m, interior 0.981/m, light_emissive_front 17.913/m, light_emissive_rear 3.426/m, rim 1.596/m, rubber 1.936/m, trim_black 0.200/m) | ≤ 5.000 % | FAIL |
| `collision_pieces` | collision pieces (connected components) | 4 | 1 – 16 | PASS |
| `collision_convex` | largest distance of a hull vertex outside a face plane of its own piece | 2.3019 m; concave pieces: 276 triangles, z -1.58..1.65: 2.302 m | ≤ 0.005 m | FAIL |
| `collision_triangles` | triangles in the largest collision piece | 276 | ≤ 1024 | PASS |
| `collision_open` | collision open-edge length | 0.000 m |  | info |
| `collision_ground` | lowest collision point vs the ground (y = 0) | -0.096 m | ≥ −0.03 m | FAIL |
| `collision_coverage` | collision bounding box / LOD0 bounding box per axis | x 0.930, y 1.074, z 0.997 (collision y -0.096..1.303, x ±0.926; LOD0 y 0.000..1.303, x ±0.995) | 0.85 – 1.05 | FAIL |


## 4. Silhouette accuracy: FAIL

| Check | What | Value | Threshold | Result |
|---|---|---|---|---|
| `the-blueprints:side` | side view vs the-blueprints | IoU 0.974, outline mean 10.0 mm, p95 26.1 mm, max 78 mm | IoU ≥ 0.9, mean ≤ 20 mm, p95 ≤ 60 mm | PASS |
| `the-blueprints:side:holes` | see-through gaps and seams inside the side silhouette (filled by the closing) | 298 px = 101.6 cm² |  | info |
| `the-blueprints:front` | front view vs the-blueprints | IoU 0.949, outline mean 15.9 mm, p95 52.6 mm, max 92 mm | IoU ≥ 0.9, mean ≤ 20 mm, p95 ≤ 60 mm | PASS |
| `the-blueprints:front:holes` | see-through gaps and seams inside the front silhouette (filled by the closing) | 46 px = 15.7 cm² |  | info |
| `the-blueprints:rear` | rear view vs the-blueprints | IoU 0.958, outline mean 13.4 mm, p95 52.9 mm, max 89 mm | IoU ≥ 0.9, mean ≤ 20 mm, p95 ≤ 60 mm | PASS |
| `the-blueprints:rear:holes` | see-through gaps and seams inside the rear silhouette (filled by the closing) | 46 px = 15.7 cm² |  | info |
| `the-blueprints:top` | top view vs the-blueprints | IoU 0.972, outline mean 18.2 mm, p95 49.9 mm, max 140 mm | IoU ≥ 0.9, mean ≤ 20 mm, p95 ≤ 60 mm | PASS |
| `the-blueprints:top:holes` | see-through gaps and seams inside the top silhouette (filled by the closing) | 132 px = 45.0 cm² |  | info |
| `dimensions-com:side` | side view vs dimensions-com | IoU 0.969, outline mean 11.9 mm, p95 31.9 mm, max 64 mm | IoU ≥ 0.9, mean ≤ 20 mm, p95 ≤ 60 mm | PASS |
| `dimensions-com:side:holes` | see-through gaps and seams inside the side silhouette (filled by the closing) | 664 px = 103.7 cm² |  | info |
| `dimensions-com:front` | front view vs dimensions-com | IoU 0.955, outline mean 14.0 mm, p95 59.3 mm, max 100 mm | IoU ≥ 0.9, mean ≤ 20 mm, p95 ≤ 60 mm | PASS |
| `dimensions-com:front:holes` | see-through gaps and seams inside the front silhouette (filled by the closing) | 346 px = 54.0 cm² |  | info |
| `dimensions-com:rear` | rear view vs dimensions-com | IoU 0.956, outline mean 13.6 mm, p95 59.4 mm, max 100 mm | IoU ≥ 0.9, mean ≤ 20 mm, p95 ≤ 60 mm | PASS |
| `dimensions-com:rear:holes` | see-through gaps and seams inside the rear silhouette (filled by the closing) | 46 px = 7.2 cm² |  | info |
| `dimensions-com:top` | top view vs dimensions-com | IoU 0.968, outline mean 20.6 mm, p95 64.3 mm, max 142 mm | IoU ≥ 0.9, mean ≤ 20 mm, p95 ≤ 60 mm | FAIL |
| `dimensions-com:top:holes` | see-through gaps and seams inside the top silhouette (filled by the closing) | 474 px = 74.0 cm² |  | info |

Overlays: dark grey = both, red = model only, blue = reference only. References are scaled by the wheelbase and aligned on the ground line (see `data/cars/<id>/refs/refs.toml`).

![silhouette-the-blueprints-side.png](silhouette-the-blueprints-side.png)
![silhouette-the-blueprints-front.png](silhouette-the-blueprints-front.png)
![silhouette-the-blueprints-rear.png](silhouette-the-blueprints-rear.png)
![silhouette-the-blueprints-top.png](silhouette-the-blueprints-top.png)
![silhouette-dimensions-com-side.png](silhouette-dimensions-com-side.png)
![silhouette-dimensions-com-front.png](silhouette-dimensions-com-front.png)
![silhouette-dimensions-com-rear.png](silhouette-dimensions-com-rear.png)
![silhouette-dimensions-com-top.png](silhouette-dimensions-com-top.png)

## 5. Visual review pack: REVIEW

| Check | What | Value | Threshold | Result |
|---|---|---|---|---|
| `renders` | beauty renders from the fixed cameras | 12 renders | front-three-quarter, rear-three-quarter, side, front, rear, top, front-low | PASS |
| `blender_stats` | scene as Blender imports it | LOD0 363874 triangles, per LOD {"0":363874}, 22 mesh objects, import 1.1 s |  | info |

The renders and side-by-sides need a reviewer: score them with `rubric.md` (left = render, right = reference in every side-by-side).

![render-front-low.png](render-front-low.png)
![render-front-three-quarter.png](render-front-three-quarter.png)
![render-front.png](render-front.png)
![render-interior.png](render-interior.png)
![render-lights-front.png](render-lights-front.png)
![render-lights-rear.png](render-lights-rear.png)
![render-rear-three-quarter.png](render-rear-three-quarter.png)
![render-rear.png](render-rear.png)
![render-side.png](render-side.png)
![render-top.png](render-top.png)
![render-wheel-front-left.png](render-wheel-front-left.png)
![render-wheel-rear-left.png](render-wheel-rear-left.png)
![compare-blueprint-the-blueprints-side.png](compare-blueprint-the-blueprints-side.png)
![compare-blueprint-the-blueprints-front.png](compare-blueprint-the-blueprints-front.png)
![compare-blueprint-the-blueprints-rear.png](compare-blueprint-the-blueprints-rear.png)
![compare-blueprint-the-blueprints-top.png](compare-blueprint-the-blueprints-top.png)
![compare-blueprint-dimensions-com-side.png](compare-blueprint-dimensions-com-side.png)
![compare-blueprint-dimensions-com-front.png](compare-blueprint-dimensions-com-front.png)
![compare-blueprint-dimensions-com-rear.png](compare-blueprint-dimensions-com-rear.png)
![compare-blueprint-dimensions-com-top.png](compare-blueprint-dimensions-com-top.png)
![compare-photo-front-three-quarter-vauxford-2019-911-carrera-s-front.png](compare-photo-front-three-quarter-vauxford-2019-911-carrera-s-front.png)
![compare-photo-rear-three-quarter-vauxford-2019-911-carrera-s-rear.png](compare-photo-rear-three-quarter-vauxford-2019-911-carrera-s-rear.png)
![compare-photo-side-bretwa-911-carrera-s-side.png](compare-photo-side-bretwa-911-carrera-s-side.png)

## 6. In-engine smoke test (Bevy, lavapipe): PASS

| Check | What | Value | Threshold | Result |
|---|---|---|---|---|
| `loads` | GLB loads in Bevy (glTF loader, all dependencies) | loaded; 48 mesh entities, 12 materials, 45 frames in 11 s | loads without error | PASS |
| `log_errors` | ERROR lines or panics in the Bevy log | none | none | PASS |
| `non_blank` | frame not blank: car coverage and luma spread | coverage 8.8 %, luma stddev 14.1 | coverage ≥ 2 %, stddev ≥ 4 | PASS |

Full log: `smoke.log` in the report directory.

![smoke.png](smoke.png)
