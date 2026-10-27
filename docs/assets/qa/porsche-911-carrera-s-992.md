# Asset QA: Porsche 911 Carrera S (992)

**Verdict: FAIL.** Fix the defects below and run the check again.

| | |
|---|---|
| Asset | `assets/cars/porsche-911-carrera-s-992/car.glb` + `car_lod1.glb` + `car_lod2.glb`, branch `exp/asset-911` at `1641f24` ("992 asset: handoff note, faired loft and fastback pass, final GLBs, renders and in-engine captures"), 13.2 MB |
| sha256 | `car.glb` ac4b8dbe…dc1c, `car_lod1.glb` 2780795d…b7db, `car_lod2.glb` f2f17dec…c5f |
| Checker | `racer-assetqa` on `exp/asset-qa` ([README](README.md)), Blender 5.2.3 Cycles at 64 samples, Bevy 0.19.1 on lavapipe |
| Command | `cargo run -p racer-assetqa -- check porsche-911-carrera-s-992 --glb <worktree>/assets/cars/porsche-911-carrera-s-992/car.glb --samples 64` (the asset files were checked out into a scratch worktree; the branch was not merged) |
| Full generated report | [report.md](porsche-911-carrera-s-992/report.md), [report.json](porsche-911-carrera-s-992/report.json) |

Two earlier commits were checked too: the WIP `d1206e8` and `baf35eb`. All three fail the same
checks, except that the collision hull changed between them (see defect 1). The numbers
moved by less than the tolerances, and the renders look the same apart from a slightly
smoother bonnet.

| Gate | Result | Failing checks |
|---|---|---|
| 1. Structure and conventions | **PASS** | none |
| 2. Dimensions | **PASS** | none |
| 3. Mesh hygiene | **FAIL** | `collision_convex`, `collision_ground`, `collision_coverage`, `uv_stretch`, `texel_density`, `degenerate` |
| 4. Silhouette accuracy | **FAIL** | `dimensions-com:top` (narrow miss: mean 20.1 mm vs 20, p95 65.2 mm vs 60) |
| 5. Visual review pack | **FAIL** (scored below) | rubric 8/16, with a 0 for surface quality |
| 6. In-engine smoke test | **PASS** | none |

The asset follows the contract and the car's published numbers exactly, and its overall
shape is close to the blueprints. It fails on its collision hull, its UVs, and on how the
details and surfaces look up close.

## 1. Structure and conventions: PASS

All 17 required LOD0 nodes and all 8 required sockets are present once. There are 7 light
sockets (`headlight_l/r` at the front; `tail_l/r`, `reverse_l/r` and `brake` at the rear).
All 9 contract materials exist, plus `brake_caliper`. Every render primitive has a
material. The asset uses metres and +Y up (extents x 2.006, y 1.300, z 4.519 m). Its lowest
point is at y 0.000, nothing is mirrored, and the front axle is at z −1.225 with left wheels
at −X. The origin is at the axle midpoint (z 0.000, x 0.000). Wheel node origins sit at
their geometry centres (0.0 mm). Hubs and wheel sockets match car.toml exactly (Δ 0.000 m
on every axis). Mirrors, doors, bumpers, lights, exhaust and camera sockets are all on the
correct side.

## 2. Dimensions: PASS

| | Asset | car.toml | Δ |
|---|---|---|---|
| Length | 4.519 m | 4.519 m | +0.00 % |
| Width without mirrors | 1.862 m | 1.852 m | +0.54 % (rear tyres stand 5 mm proud of the body on each side) |
| Width with mirrors | 2.006 m | 2.024 m (Porsche) | −0.9 %, info only |
| Height | 1.300 m | 1.300 m | +0.04 % |
| Wheelbase | 2.450 m | 2.450 m | 0 |
| Track front / rear | 1.589 / 1.557 m | 1.589 / 1.557 m | 0 / 0 |
| Wheel radius front / rear | 0.3397 / 0.3582 m | 245/35 R20: 0.3397, 305/30 R21: 0.3582 | 0 |
| Tyre width front / rear | 0.245 / 0.305 m | 0.245 / 0.305 m | 0 |

## 3. Mesh hygiene: FAIL

| Check | Value | Threshold | |
|---|---|---|---|
| LOD0 triangles | 363 376 | 150k–500k | PASS |
| LOD1 / LOD0 | 109 002 (0.300) | 0.1–0.6 | PASS |
| LOD2 / LOD1 | 37 222 (0.341) | 0.1–0.6 | PASS |
| LOD1 / LOD2 side silhouette IoU vs LOD0 | 1.000 / 0.998 | ≥ 0.95 | PASS |
| Non-manifold edges | 0 of 549 538 | ≤ 0.01 % | PASS |
| Collision closed, positive volume | yes, open 0 m, +6.53 m³ | | PASS |
| Degenerate triangles | 802 (0.221 %): lights_front 432, mirror_l/r 80 each, body 40, each brake 32 | ≤ 0.1 % | **FAIL** |
| Loose pieces (≤ 4 triangles) | 0 | ≤ 0.05 % | PASS |
| Normals vs winding | 0.005 % of area | ≤ 1 % | PASS |
| Visible back faces | 0.000 % in all six views | ≤ 2 % | PASS |
| UVs present | every primitive in every LOD | all | PASS |
| Zero-area UVs | 2.12 % of area | ≤ 5 % | PASS |
| UV stretch > 4:1 | 11.9 % of LOD0 area: body 6.1, bumper_front 1.3, bumper_rear 0.8, each wheel ~0.5, lights_front 0.5 | ≤ 5 % | **FAIL** |
| Texel density outliers (×¼…×4 of the material median) | 11.2 %: interior 5.6, body 1.6, brakes 0.5 each | ≤ 5 % | **FAIL** |
| Collision pieces | 5 | 1–16 | PASS |
| Collision convexity | the 184-triangle piece spanning z −0.68…1.65 has a face with vertices 1.32 m on its outer side | ≤ 5 mm | **FAIL** |
| Collision vs ground | lowest point y = −0.096 m | ≥ −0.03 m | **FAIL** |
| Collision bbox / LOD0 bbox | x 0.922, **y 1.074**, z 0.998 | 0.85–1.05 | **FAIL** |

Reported only: 154.2 m of open edges across render parts (expected for a body split into
panels). 566 manifold edges have inconsistent winding (body 390, lights_front 96,
bumper_rear 80); none of them show as visible back faces.

## 4. Silhouette accuracy: FAIL (one view of eight)

Two independent drawings are used. The pipeline fitted the body to the-blueprints, so
dimensions.com is the drawing it was not tuned on. The two drawings agree with each other to
IoU 0.965–0.992 (see the README).

| View | vs the-blueprints (IoU, mean, p95) | vs dimensions.com (IoU, mean, p95) |
|---|---|---|
| side | 0.973, 10.3 mm, 26.1 mm | 0.969, 12.0 mm, 31.9 mm |
| front | 0.954, 14.3 mm, 49.9 mm | 0.961, 11.9 mm, 52.7 mm |
| rear | 0.961, 12.1 mm, 49.6 mm | 0.962, 11.5 mm, 52.7 mm |
| top | 0.972, 17.9 mm, 52.2 mm | **0.968, 20.1 mm, 65.2 mm** |

Thresholds per view: IoU ≥ 0.90, mean ≤ 20 mm, p95 ≤ 60 mm. The side profile is very good.
The front and rear views pass with less margin on p95. The overlays (grey both, red model only,
blue reference only) show where:

| | the-blueprints | dimensions.com |
|---|---|---|
| side | ![](porsche-911-carrera-s-992/silhouette-the-blueprints-side.png) | ![](porsche-911-carrera-s-992/silhouette-dimensions-com-side.png) |
| front | ![](porsche-911-carrera-s-992/silhouette-the-blueprints-front.png) | ![](porsche-911-carrera-s-992/silhouette-dimensions-com-front.png) |
| rear | ![](porsche-911-carrera-s-992/silhouette-the-blueprints-rear.png) | ![](porsche-911-carrera-s-992/silhouette-dimensions-com-rear.png) |
| top | ![](porsche-911-carrera-s-992/silhouette-the-blueprints-top.png) | ![](porsche-911-carrera-s-992/silhouette-dimensions-com-top.png) |

What the overlays show:

* **Front and rear:** the greenhouse is too wide at its base. There are red wedges between
  the A/C-pillar feet and the mirrors, and the roof is slightly too wide.
* **Mirrors:** they sit lower and about 9 mm per side further inboard than drawn (width over
  mirrors 2.006 m against 2.024 m).
* **Lower body corners:** the bumper and sill corners next to the tyres are too high (blue
  bands at the bottom).
* **Top view:** the plan view is too rounded at the four corners, and the tail is narrower
  than in both drawings.
* **Side view:** the front lower lip and rear lower valance hang 1–2 cm low.

See-through seams inside the silhouette (filled before comparing): about 108 cm² in the side
view and 15–63 cm² in the top view. These are panel gaps with nothing behind them.

## 5. Visual review pack: FAIL (8/16, surface quality 0)

The renders are from Blender Cycles in a neutral studio. Each side-by-side shows the render
on the left and the reference on the right. I applied the [rubric](porsche-911-carrera-s-992/rubric.md)
by looking at every image below.

| # | Criterion | Score | Evidence |
|---|---|---|---|
| 1 | Proportions | 1 | Side profile, overhangs, roofline and stance match the side-by-side well (side IoU 0.97). The front is wrong: the 911's front wings rise above the bonnet and carry the headlights on top, but here the bonnet, wings and bumper form one low rounded nose. The greenhouse base is too wide in the front/rear views. |
| 2 | Front signature | 1 | Round headlights exist but are about half the real size. They sit low and inboard, almost in the bumper corners, instead of upright on the wing tops, and they have no four-point DRL. The intakes are three plain black boxes, not the slatted side intakes plus centre slot. Fins stick out of the bumper sides (`render-front-low`). Recognisable only as "round lights". |
| 3 | Rear signature | 1 | Present: full-width light bar, vertical-louvre engine grille with the third brake light. Off: the light bar is a flat painted stripe with no lens or depth, there is no PORSCHE lettering or recess, the rear bumper is a tall flat slab, and the exhausts are small round tips instead of large ovals. |
| 4 | Wheels | 1 | Staggered sizes and tyre profiles are right. The rim is a generic five twin-spoke design. The calipers are oversized and float clear of the disc at the top (`render-wheel-front-left`). |
| 5 | Glass and openings | 1 | Glass reads as tinted glass. The windscreen is grainy in Cycles (noise on the transparent glass over the dash). The interior is not visible through the side windows. A door cut line exists, but the door's leading edge stands proud of the wing. |
| 6 | Surface quality | 0 | The bonnet is wavy with dents. Creases and kinks run where the wing meets the bumper, ahead of the front wheel. A roof-skin flap lifts at the windscreen header (`render-interior`). The rear bumper side panel is stepped proud of the body behind the rear wheel (`render-wheel-rear-left`). |
| 7 | Materials | 2 | Paint, black trim, rubber, rim, chrome, emissive front/rear lights and calipers are on the right surfaces, and the light bar glows in the Bevy smoke frame. |
| 8 | Overall likeness | 1 | The side view reads as a 992. The three-quarter and front views read as a generic rounded coupé. |

Total 8/16, below the 12 needed. Surface quality scores 0, which fails the review on its
own.

| Front three-quarter vs photo |
|---|
| ![](porsche-911-carrera-s-992/compare-photo-front-three-quarter-vauxford-2019-911-carrera-s-front.png) |

| Rear three-quarter vs photo |
|---|
| ![](porsche-911-carrera-s-992/compare-photo-rear-three-quarter-vauxford-2019-911-carrera-s-rear.png) |

| Side vs photo |
|---|
| ![](porsche-911-carrera-s-992/compare-photo-side-bretwa-911-carrera-s-side.png) |

| Orthographic renders vs dimensions.com |
|---|
| ![](porsche-911-carrera-s-992/compare-blueprint-dimensions-com-side.png) |
| ![](porsche-911-carrera-s-992/compare-blueprint-dimensions-com-front.png) |
| ![](porsche-911-carrera-s-992/compare-blueprint-dimensions-com-rear.png) |
| ![](porsche-911-carrera-s-992/compare-blueprint-dimensions-com-top.png) |

Side-by-sides with the-blueprints are in the same directory (`compare-blueprint-the-blueprints-*.png`).

| | | |
|---|---|---|
| ![](porsche-911-carrera-s-992/render-front-low.png) | ![](porsche-911-carrera-s-992/render-front.png) | ![](porsche-911-carrera-s-992/render-rear.png) |
| ![](porsche-911-carrera-s-992/render-wheel-front-left.png) | ![](porsche-911-carrera-s-992/render-wheel-rear-left.png) | ![](porsche-911-carrera-s-992/render-interior.png) |
| ![](porsche-911-carrera-s-992/render-lights-front.png) | ![](porsche-911-carrera-s-992/render-lights-rear.png) | ![](porsche-911-carrera-s-992/render-top.png) |

## 6. In-engine smoke test: PASS

Bevy's glTF loader loaded the GLB with all dependencies in 44 frames (13 s on lavapipe):
48 mesh entities, 12 materials, no ERROR lines and no panics. The car covers 8.8 % of the
frame with luma stddev 13.9. Collision and LOD1+ nodes are hidden, as the game will do.
Note that the collision mesh has no material, so Bevy draws it with the default material
unless the game hides it.

![](porsche-911-carrera-s-992/smoke.png)

## Defects for the asset author, by priority

1. **Collision hull is unusable.** The 184-triangle piece spanning z −0.68…1.65 is not
   convex: one of its faces has vertices 1.32 m on its outer side, so the hull has a pocket.
   Avian would build a convex hull from it and silently change its shape, or reject it. The
   same piece reaches **9.6 cm below the ground** (y −0.096), so the car would ride on its
   collider instead of its tyres. That is also why the collision box is 7.4 % taller than the
   car (`collision_coverage`). Rebuild each piece as a true convex hull from its own points
   and clamp it to y ≥ ground clearance (0.12 m). (`d1206e8` used 6 convex pieces, but they
   also failed `collision_coverage`, at y 1.073. `baf35eb` had 4 pieces, one of them 2.30 m
   concave.)
2. **Body surfaces (review 0/2).** Fix the wavy bonnet and the creases where the front wing
   meets the bumper. Fix the roof flap at the windscreen header. Make the door's leading edge
   and the rear bumper side panel flush, without steps. These are visible in every
   three-quarter render.
3. **Front end design.** Raise the front wings above the bonnet line and put large, upright
   oval headlights with four-point DRL on top of them, at roughly twice the current size.
   Replace the three box intakes with the 992's slatted side intakes and slim centre slot,
   and remove the fins protruding from the bumper sides.
4. **UVs.** 11.9 % of LOD0 area is stretched more than 4:1 (body 6.1 %, front bumper 1.3 %,
   rear bumper 0.8 %). 11.2 % of the area is off its material's texel density by more than 4×,
   mostly `interior` (5.6 %) and `body` (1.6 %). Re-unwrap the body and bumper panels with
   conformal/angle-based unwrapping and pack each material at one texel density. Livery
   decals and dirt masks depend on this.
5. **Rear end detail.** Give the light bar a lens with depth and the PORSCHE lettering
   recess. Use large oval exhaust outlets set in the diffuser. Reduce the height of the flat
   rear bumper slab.
6. **Greenhouse and mirrors.** Narrow the greenhouse base (the red wedges in the front and
   rear overlays). Move the mirrors about 9 mm outboard and up, to 2.024 m over mirrors.
   Round off less at the plan-view corners, and widen the tail (top view; the
   `dimensions-com:top` failure).
7. **Degenerate triangles.** There are 802 (0.22 %), mostly in `lights_front` (432),
   `mirror_l`/`mirror_r` (80 each), `body` (40) and each brake (32). Dissolve degenerate
   faces in those generators.
8. **Wheels and brakes.** Replace the generic five twin-spoke rim with the Carrera S design.
   Shrink the calipers and seat them on the disc.
9. **Glass and seams.** Close the see-through panel seams: about 108 cm² in the side view.
   Back the panel gaps with an inner surface. Make the interior visible through the side
   glass: it reads as solid black at the review distance.
10. **Minor.** Lower lip and rear valance hang 1–2 cm low in the side view. 566 edges have
    inconsistent winding in `body`, `lights_front` and `bumper_rear`; none are visible yet,
    but they will break normal recalculation.
