# Content pipelines

Scripts that turn text inputs in `data/` into game assets in `assets/`. Everything runs
headless and unattended (see PLAN.md section 7).

## Cars (`pipelines/blender`, `pipelines/refs`)

```sh
tools/setup.sh --with-blender                 # headless Blender into .tools/blender
tools/build_car.sh porsche-911-carrera-s-992  # GLBs, renders, blueprint comparison
```

| Script | Runs in | Does |
|---|---|---|
| `refs/blueprint_profiles.py` | python3 (numpy, Pillow) | Fetches the 4-view blueprint named in `asset.toml`, segments the car, writes silhouettes and side/top/front/rear profiles to `data/cars/<id>/refs/blueprint/`. |
| `refs/commons_fetch.py` | python3 | Downloads reference photos from Wikimedia Commons with author and licence into a folder + `SOURCES.tsv`. |
| `blender/preview_profile.py` | python3 | Rasterises the lofted body and prints the IoU against the blueprint silhouettes in ~5 s, for tuning `asset.toml` without Blender. |
| `blender/build_car.py` | Blender | Builds the car (loft, panels, details, wheels, interior, sockets, collision, LODs) and exports `car.glb`, `car_lod1.glb`, `car_lod2.glb`, `stats.json`. |
| `blender/render_views.py` | Blender (Eevee) | Orthographic and beauty renders of a GLB, and 5 mm/px silhouette masks. |
| `refs/compare_blueprint.py` | python3 | Renders over blueprint views at the same scale, with the blueprint outline drawn on the render. |
| `refs/silhouette_iou.py` | python3 | IoU of the rendered masks against the blueprint silhouettes. |

Body coordinates in `asset.toml`: `s` metres from the nose rearwards, `y` lateral,
`z` up from the ground. The GLB is in game space: metres, +Y up, car facing -Z, origin
on the ground at the wheelbase midpoint.
