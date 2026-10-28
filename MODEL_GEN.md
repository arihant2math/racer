# Car model generation: status and handoff

## Status (2026-10-27): all three cars in game from AWS TRELLIS.2 bases

| Car | Base (TRELLIS.2-4B, 1536³ cascade) | Blueprint IoU side/plan/front/rear (mean) | LOD0/1/2/3 tris |
|---|---|---|---|
| VW Golf GTI Mk8 | `Golf_GTI_in_Oryx_White_Pearl_Front_Left`, seed 1, best of 16 | 0.957 / 0.977 / 0.885 / 0.880 (0.925) | 348k / 126k / 46k / 24k |
| BMW M3 Competition G80 | `BMW_M3_Competition_G80_1X7A1573`, seed 1, best of 8 | 0.922 / 0.970 / 0.896 / 0.895 (0.921); blueprint is the G20 3 Series, shorter nose | 348k / 126k / 53k / 48k |
| Porsche 911 Carrera S 992 | `Carrera_S_coupe_IMG_5832`, seed 1, best of 10 | 0.959 / 0.979 / 0.896 / 0.896 (0.932; old Hunyuan base 0.870) | 348k / 126k / 43k / 31k |

- GPU: g6e.xlarge (L40S 48 GB) in us-east-1c, instance `racer-modelgen`
  (`i-0ce37ea33c0871bb6`, key `racer-gpu-20261027`, SG `racer-gpu-ssh`
  restricted to the operator IP). **Stopped** when idle; the 250 GB volume
  keeps the venvs and ~33 GB of weights. Start it, update `~/.ssh/config`
  `Host racer-gpu` with the new IP, then see `tools/car_refs/gpu/README.md`.
- Gated HF models (facebook/dinov3, briaai/RMBG-2.0) were swapped for an
  ungated DINOv3 mirror and the original BiRefNet (no HF token).
- Candidates are ranked by `tools/car_refs/score_candidates.py` (rigid +
  uniform-scale alignment, no warp; silhouette IoU against `blueprint.json`),
  the winner is written by `tools/car_refs/normalize_base.py` to
  `data/cars/<id>/refs/base_mesh/base.{glb,json}` (Git LFS) with
  `candidates.json` next to it.
- The game build is the generic `pipelines/blender/cars/` (see its README):
  the generated mesh and PBR texture are kept, faces are split into
  `car_paint` / `glass` / generated-texture detail, wheels and brakes are
  modelled from `car.toml`. `tools/build_cars.sh` builds all cars into
  `assets/cars/<id>/model.glb`; `car_model.rs` maps all three.
- `racer-capture scene showroom` takes `RACER_SHOWROOM_CAR=<index>` (0 Golf,
  1 M3, 2 911).
- The older 911-only Blender pipeline (`pipelines/blender/porsche_992`,
  blueprint-modelled parts, Hunyuan base) is kept for reference; its glTF
  (`porsche_992.glb`) is no longer loaded.

Known gaps against "GTA V quality", in order of impact:
1. Paint/detail labels come from a texture with lighting baked in, so the
   paint boundary is ragged where the photo had reflections or shadow; the
   rear of the car (unseen in a front 3/4 photo) is hallucinated. Next: run
   TRELLIS.2 / Hunyuan multi-view (front 3/4 + rear 3/4) or fuse two
   candidates; or re-texture with a clean albedo and paint mask from
   semantic segmentation of renders.
2. Glass is opaque (the generated shell has no usable cabin); no interior.
3. Lamps keep the generated texture; no emissive light materials yet.
4. Wheel designs are parametric approximations (`configs/<id>.toml`).

The rest of this document is the original handoff (before the AWS run).

Goal: GTA V-quality, game-ready models for all three cars in `data/cars/`
(VW Golf GTI Mk8, BMW M3 Competition G80, Porsche 911 Carrera S 992.1),
produced by scripts from inputs in the repo (PLAN.md section 7). The 911 has a
working first pass that is wired into the game. It is **not** GTA V quality.
The next step is to run image-to-3D models on an AWS GPU instead of the free
Hugging Face Spaces, and to bring all three cars through the pipeline.

## What exists (commit c4110b8)

| Path | What |
|---|---|
| `tools/car_refs/extract_blueprint.py` | 4-view dimensions.com SVG -> `refs/blueprint.json` (metric silhouettes: side top/bottom, plan half-width, front/rear half-width per height; plus ~300 stroked detail lines with view and metric points). Scales by `length_m`/`height_m` from `car.toml`. Needs numpy, pillow, svgpathtools, `rsvg-convert`. |
| `tools/car_refs/contact_sheet.py` | `python contact_sheet.py <prefix> <view1,view2,...> <out.png> [cols]`: tiles `<prefix>_<view>.png` renders for review. |
| `data/cars/porsche-911-carrera-s-992/refs/` | blueprint SVG + JSON, 10 CC BY-SA Wikimedia photos (`photos/sources.json` has URL, licence, author), `base_mesh/hunyuan3d_base.glb` (Hunyuan3D-2.1 shape, untextured, ~472k verts, from `photos/..._IMG_5847.jpg`), `base_mesh/hunyuan3d_sections.json`. |
| `pipelines/blender/porsche_992/` | The build (`build.py`) and its modules. See the README there. 911-specific in places (feature line indices, part list). |
| `tools/build_car_992.sh` | `blender -b` wrapper -> `assets/cars/porsche-911-carrera-s-992/porsche_992.{glb,blend}` (~1 min on an M5). `assets/` is gitignored. |
| `pipelines/blender/porsche_992/views.py` | Review renders: `blender -b --python views.py -- <glb|blend> <prefix> [workbench|eevee|cycles] [side top front rear q_fl q_rr q_fr q_rl low_fl close_front close_rear wheel interior]`. Cycles uses Blender's bundled HDRIs (`RV_HDRI=city.exr`, `RV_ROT`). |
| `pipelines/blender/porsche_992/iou.py` | Silhouette IoU gate: `blender -b <model.blend> --python iou.py -- <refs/blueprint.json>`. 911 now: side 0.97, plan 0.98, front 0.91, rear 0.91. |
| `crates/racer-game/src/car_model.rs` | In-game loading (see "Game integration"). |
| `crates/racer-capture` scene `showroom` | `target/debug/racer-capture scene showroom <dir> --size 1600x900`: in-engine shots of the 911 (car index 2). Works on macOS without Xvfb. |

### 911 build steps (`build.py`)

1. `body_grid.py`: a clean loft from blueprint silhouettes x AI cross-sections
   (only used as a smooth reference surface and for the underbody now).
2. `base_fit.fit`: AI mesh frame (+X fwd, +Z up, sits on a plate, arbitrary
   scale) -> car frame. Scale from the tyre contact clusters (below the plate)
   to the real wheelbase, trim the plate, per-station warp to the blueprint
   side-top and plan widths (factors low-passed, clamped 0.85-1.15),
   greenhouse width from the front/rear views blended along x, cut the
   generator's skirt below the blueprint sill line, mirror to exact symmetry.
3. `base_fit.close_and_remesh`: fill holes, 5 mm voxel remesh, heal the
   generated mirror lumps by shrinkwrapping that region onto the loft,
   corrective smooth, decimate to ~200k tris.
4. Boolean-cut wheel arches (circles fitted to blueprint arch lines). Open
   window apertures by deleting faces inside the blueprint DLO / plan screen
   outlines. Glass patches are projected onto the shell, with seal ribbons
   and a dark inner shell.
5. Parts are projected onto the shell or built at blueprint positions:
   - headlights (bowl, projector, 4-point DRL, lens), tail bar and blade, engine grille slats with third brake light;
   - intakes with slats and indicators, exhausts, plate, mirrors, handles, sills, underbody;
   - panel-gap ribbons;
   - interior blockout (seats, dash, wheel, screen);
   - wheels built from the tyre/disc sizes in `car.toml` (tyre with sidewall and tread atlas, 10-spoke rim, disc, caliper, cap).
6. `textures.py` (procedural numpy textures, text via Blender fonts) and
   `materials.py` (Principled -> glTF clearcoat/transmission/IOR/emissive).
7. Join by role, build LOD1-3 (decimate 0.35/0.12/0.035), convex collision hull, sockets,
   triangulate and validate, export GLB.

### GLB contract (the game relies on these names)

- Frame: +Y up, -Z forward, metres, origin on the ground under the wheelbase midpoint.
- Root nodes:
  - `body`, `glass`, `lights`, `interior`;
  - `wheel_{FL,FR,RL,RR}`: an empty at the hub centre, with children `wheel_XX_rim[_LODn]` and `wheel_XX_tyre[_LODn]` (these spin);
  - `brake_XX_mount`: children `brake_XX[_LODn]` (these steer, no spin);
  - `<name>_LOD1..3`, `COL_body_hull`;
  - `socket_*` (wheels with a `radius` extra, exhausts, head/tail lights, cameras hood/cockpit/bumper).
- Material names the game uses: `car_paint` (replaced by the grid colour). The
  `light_*` materials (`light_drl`, `light_tail`, `light_brake`,
  `light_indicator`, `light_headlamp`, `light_reverse`) are named for future
  toggling. Textures are PNG (Bevy here has no JPEG feature).

### Game integration (`crates/racer-game/src/car_model.rs`)

- `MODELS` maps car id -> GLB file under `assets/cars/<id>/`. A car whose GLB
  exists gets the scene instead of the greybox body (`visuals.rs::mesh_cars`);
  otherwise it falls back to greybox.
- On `WorldInstanceReady`:
  - the model is shifted so its wheelbase midpoint sits on the sim's CoM frame;
  - `car_paint` is swapped for `racer_render::car_paint(CarPaint)`;
  - LOD meshes get `VisibilityRange` (18/45/110 m);
  - collision and sockets are hidden;
  - the wheel nodes get `ModelWheel`, driven from `CarTelemetry`: spin, steer, and suspension travel via `wheel_center`.
- Preloading: `PreloadedModels` loads every model at Startup.
  `AppState::Loading` shows a "Loading car models" screen and waits on
  `models_ready` before going to `Race`.
- Assets root: `main.rs` and `racer-capture` set `AssetPlugin::file_path` to
  the repo's `assets/` (`car_model::assets_dir()`).
- Adding the other two cars: build their GLBs to the same contract and add
  them to `MODELS`. Nothing else is needed.

## Why it is not GTA V quality yet

- The base shape came from one Hunyuan3D-2.1 run at octree 512, 50 steps,
  shape only. The anonymous ZeroGPU quota allows about 2 runs a day, and
  TRELLIS.2 refused the job. The generator bakes in panel gaps, lamp relief and
  noise. Remeshing and smoothing remove noise but also creases: the body is soft
  and lumpy, with no crisp character lines (bonnet edges, shoulder line,
  tail-light shelf, bumper edges).
- The generated mirrors had to be healed away, which leaves a faint seam.
  Arch lips are boolean cuts with flat liners, not rolled lips.
- Parts are simplified. The rim is a generic 10-spoke, not the actual design;
  the headlight internals, intakes, interior and engine grille are blockouts.
  The tail bar is a projected strip.
- Textures are procedural. There is no baked normal or AO map from a
  high-poly mesh, and no dirt layer. Glass uses Bevy transmission/alpha
  without tuning.
- Remaining artefacts: rough edges at the bottom of the front bumper and
  aprons, and some panel-gap ribbons that break up on curvature.

## What to do next (the plan)

### 1. GPU on AWS

- Account 441931764302, `us-east-1`. The local CLI uses `aws login` (an
  SSO-style login session for the root account), which had expired; the user
  needs to log in again.
- Suggested instance: g6e.xlarge (L40S 48 GB), or g5.2xlarge (A10G 24 GB) as
  the cheaper option. Use the Deep Learning AMI (Ubuntu, PyTorch, CUDA 12.x) and
  a 200 GB gp3 volume.
- Instances cost about $1-2/h: stop them when idle, and get approval from the
  user before launching.
- Run locally, from the official repos:
  - **TRELLIS.2** (`microsoft/TRELLIS.2`): best geometry plus PBR textures,
    1536 resolution, GLB extract with a decimation target and texture size.
    This was the first choice.
  - **Hunyuan3D-2.1** (`tencent/Hunyuan3D-2.1`): shape, plus the PBR paint
    model for textures. It accepts multi-view input (front/back/left/right),
    which helps symmetry and proportions a lot.
  - Optional: a multi-view diffusion step (for example Hunyuan3D-2mv) to make
    consistent orthographic views from photos.
- Generate many candidates per car (seeds x input photos x settings) and
  score each automatically with `iou.py`-style silhouettes after the
  `base_fit` alignment. Keep the best one, or fuse the best few, for example
  by averaging signed distance after alignment.
- Copy results into `data/cars/<id>/refs/base_mesh/` and record the generator,
  version, settings, seed and input image in a JSON next to them.
- Also possible: run a vision-language model on the box as a render-vs-photo
  reviewer (the PLAN.md "multimodal review" gate).

### 2. Credentials to ask the user for

- **OpenRouter key:** expired. It powers image models via codemode
  `models.generateImages` (gpt-image-2, gemini-3-pro-image, flux.2) and image
  classifiers. Use: clean studio reference renders on a white background, side
  and 3/4 views, as inputs for image-to-3D; judging renders against photos.
- **Hugging Face token (optional):** raises the ZeroGPU quota on the
  `microsoft/TRELLIS.2` and `tencent/Hunyuan3D-2.1` Spaces, if AWS is delayed.
  Their gradio APIs work via `gradio_client`; see the endpoint names below.
- The Radius gateway (MCP) has no 3D models. All of its models are chat
  models.

### 3. Per-car inputs (do this for the Golf and M3; redo the 911 if better sources appear)

- **Blueprint:** dimensions.com has 4-view SVGs per car. The URL pattern is
  `https://www.dimensions.com/element/<slug>`; the 911 one was
  `.../porsche-911-carrera-992-8th-gen` and its SVG is linked from that page
  (`*-Dimensions.svg`).
  - The extractor expects the outline group `_x32_D_x24_AG-OUTLINE` holding four filled paths, and the details group `_x32_D_x24_AG-DETAILS`.
  - Check the layout per drawing: side and plan are the wide views, front and rear the narrow ones.
  - Check that the blueprint length and height match `car.toml`; the 911 drawing was 454 cm vs 4.519 m in the spec, and the extractor scales to the spec.
- **Photos:** Wikimedia Commons search API, filtered to CC licences, with
  provenance stored in `photos/sources.json` (see the 911 for the format).
  - Best image-to-3D inputs: a clean front 3/4 view with the full car and margin, plus true side, front and rear views for multi-view input.
  - Background removal is done by the Spaces (`check_box_rembg`, `/preprocess_image`).
- **Name the feature lines:** `build.py` uses raw indices into
  `blueprint.json["lines"]` (for example 34/39 arches, 163/164 DLO, 143/154
  headlights, 94/85 screen outlines in plan, 49/50 tail bar, 65 exhausts, 121
  plate, 166/117 mirrors, 184 handle, 168/170/37/38 door and sill lines).
  - Indices differ per drawing.
  - Before generalising, add `refs/blueprint_labels.toml` per car (name -> line index), and render an indexed overlay to pick them, as was done by hand for the 911.

### 4. Pipeline improvements, in priority order

1. Generalise `pipelines/blender/porsche_992` into
   `pipelines/blender/cars/` with per-car config: feature labels, part list,
   wheel design parameters, light types. The 911-specific code is mostly in
   `build.py` sections 5-7 (lights, front/rear details, mirrors).
2. Better base: AWS generation, multi-view, best-of-N by IoU.
3. Hard surfaces: after the fit, sharpen character lines. Options:
   - snap the remeshed body to the blueprint feature curves;
   - or rebuild the main panels as clean subdivision surfaces fitted to the
     AI mesh (quad retopo from the blueprint loft cage, then subdivision, then
     shrinkwrap), with real panel splits along the blueprint door, bonnet and
     bumper lines.

   Bake a normal map and AO from the high-res mesh to the game mesh.
4. Use the generators' texture output (TRELLIS.2 / Hunyuan paint) only as
   a guide or for masks: paint is a game material, and baked lighting must be
   avoided.
5. Hero details: actual rim designs, proper lamp internals with emissive
   masks, interior from reference, rolled arch lips, wipers, badges as
   decals.
6. Gates in CI, per PLAN.md:
   - IoU at or above a threshold (for example 0.95 side/plan, 0.92 front/rear);
   - triangle budget (LOD0 300-500k per PLAN.md section 6);
   - a multimodal review of `views.py` renders against `refs/photos`.

## Gotchas learned

- **Blender 5.2** (`/opt/homebrew/bin/blender`):
  - EXACT booleans on the open AI shell need `use_hole_tolerant`.
  - Voxel remesh needs a closed mesh: fill holes first, or it collapses.
  - glTF export needs triangulated, validated meshes, otherwise the tangents
    and meshes fail.
  - Workbench previews use `material.diffuse_color`, which `materials.py`
    sets.
- **The AI mesh's own material slot** came through as a stray material
  ahead of `car_paint`, so the shell rendered white in-game.
  `build.py` now clears the shell's slots before assigning paint.
- **Projection from the front or rear hits the bonnet or deck first.** The
  windscreen and rear screen are built from the plan-view outlines,
  projected top-down. `Surface.project` falls back to the nearest surface
  point when a ray goes through a hole.
- **Bevy 0.19 API:**
  - scenes are `WorldAssetRoot(Handle<WorldAsset>)` with
    `GltfAssetLabel::Scene(0)`;
  - the ready event is `WorldInstanceReady` (an observer on the root);
  - primitives carry `GltfMaterialName` and `MeshMaterial3d<StandardMaterial>`;
  - `VisibilityRange` lives in `bevy::camera::visibility`.
- **HF Space endpoints:**
  - TRELLIS.2: `/start_session`, then `/preprocess_image`, then `/image_to_3d(image, seed, resolution, ...)`, then `/extract_glb(decimation_target, texture_size)`.
  - Hunyuan3D-2.1: `/shape_generation(image, mv_image_front/back/left/right, steps, guidance_scale, seed, octree_resolution, check_box_rembg, num_chunks, randomize_seed)`. It returns `{'value': path}`. `/generation_all` adds texture.
- **Tool environment:** a uv venv lives at `~/.cache/racer-tools/venv`
  (numpy, pillow, gradio_client, trimesh, scipy, opencv, svgpathtools,
  scikit-image). Blender's bundled Python has numpy but not scipy, so the
  Blender modules are numpy-only.
- **Downloads:** put untrusted downloads in their own directory and run Python on them with `-I`.
- **Unrelated failure:** `racer-trackc` test `generated_data_matches_the_track` fails on
  main independently of this work (racing line moved 0.0136 m against a 0.01 m
  limit). `racer-trackc compile` regenerates the data.
- **Repo size:** `refs/base_mesh/hunyuan3d_base.glb` is 16 MB in git. Consider
  Git LFS before adding more base meshes.
