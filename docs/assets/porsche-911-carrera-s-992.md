# Porsche 911 Carrera S (992.1): car asset prototype, handoff note

Branch `exp/asset-911`. Built on 2026-10-27 in a GPU-less cloud container (4 cores,
15 GB RAM), Blender 5.2.3 LTS headless (`tools/setup.sh --with-blender`), Bevy 0.19.1.
This prototypes the "Real cars" pipeline of PLAN.md section 7 on one car.

**Bottom line.** It is a contract-complete, reproducible, game-ready asset. Every node,
socket and material the QA contract asks for is present. Dimensions match `car.toml`.
LOD0 is 364k triangles, and LOD1/LOD2 and the convex collision hulls are included. It
loads and renders in Bevy with the shared `car_paint` material. The shape matches the
blueprint silhouettes closely: mean IoU 0.96, but see below for what that number means.

It does **not** yet read unmistakably as a 992 from every angle. The rear (full-width
light bar, engine-lid grille with third brake light, wide haunches, staggered wheels) and
the plan and side outlines are right. The front fascia, the surface quality and several
details are generic. The self-assessment below is deliberately blunt.

| Output | Path |
|---|---|
| LOD0 (contract path) | `assets/cars/porsche-911-carrera-s-992/car.glb` (9.0 MB) |
| LOD1, LOD2 | `assets/cars/porsche-911-carrera-s-992/car_lod1.glb` (3.0 MB), `car_lod2.glb` (1.2 MB) |
| Build stats | `assets/cars/porsche-911-carrera-s-992/stats.json` (triangles per node, measured dimensions) |
| Blender renders | `docs/assets/porsche-911-carrera-s-992/*.png` |
| Blueprint comparison | `docs/assets/porsche-911-carrera-s-992/blueprint_compare.png`, `iou_*.png`, `silhouette_iou.json` |
| In-engine (Bevy) captures | `docs/assets/porsche-911-carrera-s-992/ingame/*.png` |
| Inputs | `data/cars/porsche-911-carrera-s-992/asset.toml`, `refs/` |
| Scripts | `pipelines/` (see `pipelines/README.md`), `tools/build_car.sh` |

## 1. What generation is possible here (step 1 probe)

| Approach | Result |
|---|---|
| **Hunyuan3D-2mini** image-to-3D (`hy3dgen` 2.0.2 from PyPI, weights `tencent/Hunyuan3D-2mini` from Hugging Face, torch 2.14 CPU) | **Installs and loads, too slow and not working end to end.** Download about 7.9 GB, model load 51–75 s. Needed pins: `diffusers==0.32.2` (newer diffusers needs a huggingface-hub the package conflicts with) and `transformers==4.49.0` (5.x renamed the DINOv2 weights), plus casting the fp16 weights to fp32. The 5-step turbo diffusion on a masked photo took **34 min** on 4 cores, shared with a cargo build. The VAE volume decode then failed with an fp16/fp32 mismatch in the decoder. I stopped there because the time budget for dead ends was spent. Even working, it gives one fused, unpainted marching-cubes blob from a single photo: no panels, glass or wheels, and the hidden side is a guess. It would only ever be a rough base form. |
| TripoSR, Stable Fast 3D | Code is only on GitHub (no PyPI package). Cloning third-party code from GitHub was denied by this session's permission policy. SF3D weights are also gated on Hugging Face (needs a token). Not run. |
| Open image models (concept or reference images) | Not attempted. Real reference photos were available. CPU diffusion at about 7 min per step for a 0.6 B DiT means SD-class image generation would take tens of minutes per image. |
| Public reference photos | **Works.** Wikimedia Commons API, 14 CC BY-SA 4.0 photos of 992 Carrera S coupes (`refs/photos/`, authors and licences in `SOURCES.tsv`). The shared egress IP gets HTTP 429 often, so `pipelines/refs/commons_fetch.py` backs off and retries. |
| Blueprint / orthographic drawing | **Works.** the-blueprints.com watermarked 4-view preview of the 992 Carrera 4S, which has the same wide body and published dimensions as the Carrera S (1280×905, about 5.8 mm/px). It is not redistributed. `blueprint_profiles.py` fetches it into `refs/cache/` (gitignored). Only the derived silhouettes and numeric profiles are committed (`refs/blueprint/`). |
| HDRI lighting | Poly Haven CC0 HDRIs, fetched by `render_views.py` into `.cache/hdri/`. |
| Blender Eevee headless | Works on software GL (Mesa llvmpipe, EGL warnings are harmless). About 15–40 s per 1280 px frame at 32 samples. |

**Conclusion.** ML generation is not feasible here as a base form. The asset is generated
procedurally: a blueprint-driven parametric loft in Blender Python, with the published
dimensions as hard constraints. Everything is reproducible from the scripts and text
inputs in the repo.

## 2. Pipeline

```sh
tools/setup.sh --with-blender
tools/build_car.sh porsche-911-carrera-s-992          # ~15 s build + ~6 min renders/checks
tools/capture.sh car-asset /tmp/car-asset             # in-engine check (Bevy, lavapipe)
.tools/blender/blender -b --factory-startup --python pipelines/blender/check_contract.py -- \
    assets/cars/porsche-911-carrera-s-992/car.glb --car data/cars/porsche-911-carrera-s-992
python3 pipelines/blender/preview_profile.py data/cars/porsche-911-carrera-s-992 /tmp/p.png  # 5 s shape check
```

1. **References** (`pipelines/refs/blueprint_profiles.py`). Segments the four blueprint
   views, then fills the glass and keeps the car's connected component. The watermark text
   is closed over, and one hand polygon fills the windscreen sliver. The output is the
   side top/bottom profile per x, the top-view half width per x, and the front/rear half
   width per z. Each view is scaled from `car.toml`: length for side and top, height for
   front and rear.
2. **Loft** (`pipelines/blender/carlib/profile.py`). 400 stations along the length.
   - Each half section is a centripetal Catmull-Rom curve through seven control points:
     floor centre, floor corner, rocker, widest point, shoulder (fender crest or
     beltline), top edge (hood/roof/deck edge, A- and C-pillars) and top centre.
   - The half width and silhouette top come from the blueprint (Gaussian-smoothed, with
     the windscreen-base kink kept and the mirrors patched out). Everything else comes
     from monotone tables over `s` in `asset.toml`.
   - Wheel arches clip the section below a circle around the hub and add the wheel well
     (lip and inner wall at track/2 − tire width/2 − 30 mm).
   - The grid is faired along the length (Gaussian over stations) to remove ripples.
3. **Shell, cuts and panel split** (`carlib/bodymesh.py`, `carlib/details.py`).
   - The closed shell carries per-face `part` and material tags, classified by station,
     section band and height.
   - Seams that cut across the grid (door bottom, bumper tops, quarter-window edge, black
     lips) are snapped onto straight polylines and re-projected onto the surface.
   - One exact boolean cuts the headlight pockets, the front intakes and the exhaust
     openings.
   - The shell is then split into `body`, `hood`, `bumper_front`, `bumper_rear`,
     `door_l/r`, `spoiler` and `glass`. Every open panel edge is rolled 6 mm in, so
     panels meeting at a seam form a V-shaped shut line.
4. **Overlays** (ray-cast onto the original surface):
   - headlight units: lens following the fender, pitched 30°, with chrome reflector,
     projector and four-point DRL bars
   - lamp strips over the outer intakes and intake slats
   - full-width rear light bar and the third brake light
   - engine-lid grille slats
   - flush door handles, oval tailpipes and the hood crest badge
   - a dark well under the deployable spoiler (the spoiler's origin is its hinge)
5. **Parts** (`carlib/parts.py`).
   - Wheels: tire revolved from the `car.toml` size, with four tread grooves; rim with a
     barrel and five pairs of spokes (the Carrera S style), hub, cap and bolts.
   - Brakes: discs from `[brakes]` and red calipers.
   - Mirrors on stalks.
   - Interior: an inward-facing cabin shell (headliner, door cards, floor, firewall), so
     the single-sided body never shows through the glass, plus seats, dash, console and a
     tilted steering wheel (left-hand drive).
6. **Contract and export.**
   - Sockets, convex collision hulls (farthest-point subsets of the body points, so they
     stay inside the body), box UVs where no analytic UV exists, sharp edges by angle.
   - glTF export: LOD0, then in-place collapse decimation to 30 % and 10 % for LOD1 and
     LOD2.
7. **Look and compare** (`render_views.py`, `compare_blueprint.py`, `silhouette_iou.py`).
   - Eevee orthographic and beauty renders of the exported GLB.
   - 5 mm/px Workbench silhouette masks, IoU against the blueprint, and render-over-
     blueprint sheets.
8. **In engine.** `racer-capture scene car-asset` (`crates/racer-capture/src/car_asset.rs`)
   loads the GLB from the workspace `assets/` folder, swaps every `car_paint` primitive for
   `racer_render::car_paint` and hides `collision`.

## 3. Contract

- Frame: metres, +Y up, car facing −Z, origin on the ground at the wheelbase midpoint,
  centred laterally. Verified by `check_contract.py`; all checks pass.
- Mesh nodes:
  - body panels: `body`, `hood`, `bumper_front`, `bumper_rear`, `door_l`, `door_r`,
    `mirror_l`, `mirror_r`, `spoiler`
  - `glass` (windscreen, side and rear), `lights_front`, `lights_rear`, `interior`
  - `wheel_fl/fr/rl/rr` (tire and rim; origin at the hub, spin axis X)
  - `brake_fl/fr/rl/rr` (disc and caliper; same origin, not meant to spin)
  - `collision`: one mesh made of 6 disjoint convex pieces: five slices of the lower
    body along the length (cuts at s = 0.70, 1.60, 2.90 and 3.90 m from the nose) plus
    the greenhouse; about 550 triangles in total. Each connected component is convex.
- Sockets (glTF empties):
  - `socket_wheel_fl/fr/rl/rr`
  - `socket_light_headlight_l/r`, `socket_light_tail_l/r`, `socket_light_reverse_l/r`,
    `socket_light_brake`
  - `socket_exhaust_l/r`
  - `socket_camera_hood`, `socket_camera_cockpit` (driver's eye, left-hand drive)
  - Orientation: forward-looking sockets have identity rotation (they look along −Z);
    rear-facing ones (tail, brake, reverse lights, exhausts) are rotated 180° about Y.
- Materials:
  - the nine contract materials: `car_paint`, `glass`, `rubber`, `rim`, `chrome`,
    `light_emissive_front`, `light_emissive_rear`, `trim_black`, `interior`
  - one extra, `brake_caliper` (red, on the brake nodes only)
  - The paint colour baked into `car_paint` is a preview colour (`[paint]` in
    `asset.toml`). The game replaces the material.
- UVs: on every mesh.
  - Body panels and glass: from the loft parameterisation (length × section arc length,
    metres / 5), so they are continuous and low-stretch; the two halves overlap
    (mirrored).
  - Wheels: cylindrical.
  - Everything else: box-projected per face. Not packed into a 0–1 atlas.

## 4. Triangle counts

| | LOD0 | LOD1 | LOD2 |
|---|---|---|---|
| Total (excluding collision) | 363,376 | 109,002 (30 %) | 37,244 (10.2 %) |

LOD0 per node: `body` 150k (the shell including the wheel wells and grille slats), `glass`
37k, `interior` 30k, `hood` 18k, four wheels 17.4k each, `bumper_front` 16k, doors 9.7k
each, `lights_front` 6.5k, `bumper_rear` 6.3k, `spoiler` 3.4k, mirrors 1.7k each, brakes
0.7k each, `lights_rear` 1.3k. Exact figures are in `stats.json`. The meshes are not
manifold as a whole: panels are open shells with rolled edges, and the overlays are
separate shells. The shell itself is closed before the split.

## 5. Dimensions vs car.toml

| | car.toml | Model |
|---|---|---|
| Length | 4.519 | 4.519 |
| Width (body) | 1.852 | 1.852 |
| Width with mirrors | 2.024 (blueprint) | 2.006 |
| Height | 1.300 | 1.3005 |
| Wheelbase | 2.450 | 2.450 |
| Track front / rear | 1.589 / 1.557 | 1.589 / 1.557 (hub centres) |
| Tires | 245/35 ZR20, 305/30 ZR21 | radius 0.3397 / 0.3582 m, section width 245 / 305 mm |
| Ground clearance | – | 0.12 m |
| Brake discs | 350 / 350 mm | 350 / 350 mm |

## 6. Silhouette accuracy, and against what

`silhouette_iou.json` holds the IoU of orthographic Workbench masks of the exported GLB
(including wheels and mirrors) against silhouettes segmented from the **independent
third-party blueprint drawing**: side 0.962, top 0.979, front 0.950, rear 0.962, mean
0.963. The file is rewritten on every build.

This is **not an independent accuracy measure** for side and top. The loft takes its
side-view top line and its top-view half width directly from that same drawing, so a high
side/top IoU mostly shows the pipeline reproduces its input. Front and rear are more
meaningful, because only the overall height and width come from those views. The
cross-section shapes are hand-tuned tables, so 0.95–0.96 there is earned. Even so, IoU
only measures outlines: it cannot see the problems listed below. No reference photo was
used numerically.

## 7. Self-assessment against the 992 references

Compared with the Commons photos in `refs/photos/` and the renders in
`docs/assets/porsche-911-carrera-s-992/`.

What reads as a 992:
- Overall proportions and plan shape: length, width, height, rear-engine overhangs.
- Staggered 20/21" wheels filling the arches, red calipers.
- Full-width rear light bar under the spoiler lip, black engine-lid grille with
  longitudinal slats and the third brake light.
- Wide rear haunches, a teardrop greenhouse narrowing to the rear, flush door handles.
- Round headlights with the four-point DRL signature, three-part front intakes with the
  slim lamp strips.

Honest weaknesses, worst first:
1. **The roofline and rear deck are not the 911's.** In side and three-quarter views the
   signature fastback is weak. The roof looks flat on top and the rear deck bulbous: the
   roof-to-tail slope reads as a roof followed by a hump, not one continuous arc into the
   tail. The side silhouette matches the drawing (IoU 0.96), but the cross-sections behind
   the B-pillar are too full and wide at deck height. The last pass (narrower spine, lower
   haunch crest) improved this; it is not fixed.
2. **The front fascia is generic.** The headlights sit right, but the area around them,
   the hood leading edge and the bumper face read as "a sports car", not a 992.
   - The hood has a slight centre valley and lumpiness instead of the 992's smooth, broad,
     gently crowned hood falling to a crisp leading edge.
   - The bumper face is boxy at the corners.
   - The intake shapes are plain rectangles (the real ones are trapezoids with mesh and
     fins).
   - The lamp units protrude like bezels from some angles instead of sitting flush in
     the fender.
3. **The surfaces are not class-A.** Reflections show lumps and ripples on the doors,
   front fenders, rear quarters and deck.
   - Causes: per-station sections from a 7-point spline, tables with few keys, the arch
     widening, and the bumper and fender transitions near the nose.
   - Fairing along the length removed the worst banding. Across the section and at
     transitions it remains.
   - Panel seams that cut across the grid were straightened, but some corners (rear
     bumper, A-pillar base) still show small kinks.
4. **The side glass (DLO) shape is approximate.** The window line is the loft's beltline
   and roof edge, with a polyline quarter-window edge. The 992's characteristic window
   graphic, chrome surround and B-pillar treatment are only roughly matched.
5. **Missing or simplified details:**
   - no wipers, licence plates, badging or lettering, side repeaters or reversing camera
   - no fuel filler or antenna
   - simplified rear bumper (no reflectors or vertical vents) and mirrors (an ellipsoid on
     a box stalk)
   - plain tires (no tread blocks or sidewall lettering)
   - a box-primitive interior
6. **Materials are flat.** No textures, decals or normal maps; paint flake and clearcoat
   come from the game shader. Glass is alpha-blended (no transmission in glTF).
7. **The LODs are plain collapse decimation.** Fine for distance, but the silhouettes and
   seams of LOD2 get chunky.

## 8. What it would take to reach PLAN's "GTA VI" bar

- **Surface quality first.** Move from per-station spline sections to a real fitted
  surface: either a subdivision-surface cage fitted to the profiles, or NURBS/T-spline
  patches. Fairing would run across sections, not only along the length, and curvature
  continuity at feature lines would come from the cage. The loft parameters in
  `asset.toml` can drive the cage instead of the raw grid. This also fixes the panel
  seams: cut lines become cage edges.
- **Better references and a photo-based gate.**
  - Clean, unwatermarked orthographic drawings (vector) and calibrated multi-view photos
    of the same car.
  - Add a photo-match gate: render from estimated camera poses and compare edge and
    silhouette maps with the photos, not just ortho IoU.
  - A multimodal review step scoring signature details, as PLAN describes.
- **Hand-quality hero details as library parts.** Headlight and taillight units,
  intakes, mirrors, wheels, grille, badges and wipers would be modelled once with real
  topology (or bought/scanned, if licensing allows). They would be placed by sockets
  from the pipeline, the same way the overlays are placed now.
- **Textures.**
  - Baked AO and curvature, a tread normal map, sidewall lettering decals.
  - Light-unit textures (LED arrays, reflectors), carbon/plastic grain on the trims.
  - The interior from a real cabin scan or model.
  - KTX2 output per PLAN.
- **GPU and ML.** With a GPU (or hosted API keys), image-to-3D models such as Hunyuan3D-2
  or TRELLIS could supply base forms. Realistically, though, a car at this bar is
  CAD-like hard-surface modelling. ML is more useful for textures, materials and interior
  clutter than for the body shell.
- **Proper LODs.** Silhouette-preserving LODs (decimation that protects seams and
  feature edges, or regenerating the loft at lower resolution per LOD) and a separate
  low-poly damage and collision set.

## 9. Notes for the QA session

- Contract GLB: `assets/cars/porsche-911-carrera-s-992/car.glb`. It is regenerated by
  `tools/build_car.sh porsche-911-carrera-s-992`, which takes about 15 s for the GLBs and
  is deterministic for a given `asset.toml`.
- Blueprint silhouettes for IoU: `data/cars/porsche-911-carrera-s-992/refs/blueprint/mask_*.png`
  with scales in `profiles.json`. Their orientation is explained in
  `pipelines/refs/silhouette_iou.py`, which also shows how to place them against
  orthographic renders.
- `check_contract.py` is this pipeline's own smoke test, not a substitute for the
  independent checker.
