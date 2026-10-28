# Porsche 911 Carrera S (992.1) asset pipeline

> Superseded in game by the generic `pipelines/blender/cars/` build from the AWS TRELLIS.2 base
> (`refs/base_mesh/base.glb`); this blueprint-modelled pipeline is kept for reference.

```sh
tools/build_car_992.sh            # -> assets/cars/porsche-911-carrera-s-992/porsche_992.{glb,blend}
```

Inputs (all in `data/cars/porsche-911-carrera-s-992/`):

| File | What |
|---|---|
| `car.toml` | Length, width, height, wheelbase, tracks, tyre sizes, disc sizes (hard constraints) |
| `refs/blueprint_dimensions_com.svg` | 4-view outline drawing (dimensions.com) |
| `refs/blueprint.json` | Metric silhouettes + 300 feature lines, from `tools/car_refs/extract_blueprint.py` |
| `refs/base_mesh/hunyuan3d_base.glb` | Image-to-3D base (Hunyuan3D-2.1, HF Space, from `refs/photos/Porsche_992_Carrera_S_coupe_IMG_5847.jpg`) |
| `refs/base_mesh/hunyuan3d_sections.json` | Cross-sections of the base (`sample_ai.py`) |
| `refs/photos/` | CC BY-SA reference photos (Wikimedia Commons), sources in `sources.json` |

Steps (`build.py`):

1. `base_fit.fit`: scale the AI mesh by its wheel positions to the real wheelbase, trim its base plate,
   warp it per station to the blueprint side/plan/front/rear silhouettes, cut the skirt below the sill.
2. `base_fit.close_and_remesh`: cap, voxel remesh (5 mm), heal the generated mirrors against the
   blueprint loft (`body_grid.py`), smooth, decimate to the LOD0 body budget.
3. Wheel arches cut at the blueprint arch circles, liners added; window apertures opened from the
   blueprint DLO / screen outlines with glass, seals and B pillar; panel-gap ribbons from the blueprint
   door, bonnet and sill lines.
4. Modelled parts: headlights (bowl, projector, 4-point DRL, lens), full-width tail bar and blade,
   engine grille slats + third brake light, intakes with slats and indicator blades, exhausts, plate,
   mirrors, handles, sills, underbody, interior (seats, dash, wheel, screen), wheels (tyre with
   sidewall/tread atlas, 10-spoke rim, disc, caliper, hub cap) from the tyre and disc sizes.
5. Procedural textures (`textures.py`), PBR materials (`materials.py`, clearcoat/transmission/emissive).
6. LOD1-3 (decimate), convex collision hull, sockets, triangulate/validate, export GLB.

Output layout (GLB root nodes): `body`, `glass`, `lights`, `interior`, `wheel_{FL,FR,RL,RR}`
(empties at hub centres; `_rim` and `_tyre` children spin), `brake_*_mount` (steer, don't spin),
`*_LOD1..3`, `COL_body_hull`, `socket_*` (wheels, exhausts, lamps, cameras; extras carry radius).
Frame: +Y up, -Z forward, origin on the ground at the wheelbase midpoint, metres.

Review renders: `blender -b --python views.py -- <glb|blend> <prefix> [workbench|cycles] [views...]`.
