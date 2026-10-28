# Car asset pipeline (all cars)

```sh
tools/build_cars.sh [car_id ...]   # -> assets/cars/<id>/model.{glb,blend}, ~25 s per car
```

Inputs per car:

| File | What |
|---|---|
| `data/cars/<id>/car.toml` | length, wheelbase, tracks, tyre and disc sizes |
| `data/cars/<id>/refs/base_mesh/base.glb` | TRELLIS.2 (1536³) mesh + PBR texture, aligned to the blueprint (metres, game frame) |
| `data/cars/<id>/refs/base_mesh/base.json` | generator, settings, seed, input photo, blueprint IoU |
| `data/cars/<id>/refs/base_mesh/candidates.json` | IoU scores of every generated candidate |
| `pipelines/blender/cars/configs/<id>.toml` | rim design, rim and caliper colours, tyre lettering, exhaust tips, hub badge |

Base meshes come from `tools/car_refs/gpu/` (AWS), are ranked by
`tools/car_refs/score_candidates.py` (silhouette IoU against `refs/blueprint.json`
after a rigid + uniform-scale alignment) and written by
`tools/car_refs/normalize_base.py`.

`build.py` steps:

1. Import the base, weld, find the generated axles from the tyre contacts, put
   the origin on the ground at the wheelbase midpoint.
2. Delete the generated wheels (faces inside the tyre cylinders), add arch liners.
3. Classify faces from the generated PBR texture:
   - paint (`car_paint`, swapped for the grid colour by the game): same chroma
     as the bonnet (sampled by top-down ray casts), dielectric;
   - glass (`glass`, opaque dark gloss: the generated shell has no usable
     cabin): translucent or more metallic than the bonnet above the belt line,
     grown through the greenhouse;
   - everything else keeps the generated texture (`body_detail`: lamps, grilles,
     badges, trim, tyres' shadows).
   A majority filter and a small-island merge clean the labels.
4. Decimate the body to ~320k tris (LOD0 ~350k with wheels).
5. Wheels and brakes from the tyre/disc sizes (`wheels.py`, from the 911 pipeline,
   parametrised per car).
6. LOD1-3 (progressive decimation; non-manifold edges are split when the collapse
   stalls), convex hull, sockets, glTF export. Same GLB contract as before
   (MODEL_GEN.md): `body`, `glass`, `lights` (if any), `wheel_XX`, `brake_XX_mount`,
   `*_LOD1..3`, `COL_body_hull`, `socket_*`.

Known limits: the paint/detail split comes from a baked texture, so lighting
baked into the photo (reflections on the glass, shadowed lower panels) shows as
ragged label edges; tail lamps keep the generated texture rather than an
emissive `light_tail` material; no interior (glass is opaque).
