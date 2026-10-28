#!/usr/bin/env bash
# Build the game car assets with headless Blender (pipelines/blender/cars).
#   tools/build_cars.sh [car_id ...]   (default: all cars with refs/base_mesh/base.glb)
# -> assets/cars/<id>/model.{glb,blend}
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BLENDER="${BLENDER:-$(command -v blender || echo "$ROOT/.tools/blender/blender")}"
cars=("$@")
if [ ${#cars[@]} -eq 0 ]; then
  for d in "$ROOT"/data/cars/*/; do
    [ -f "$d/refs/base_mesh/base.glb" ] && cars+=("$(basename "$d")")
  done
fi
for id in "${cars[@]}"; do
  echo "== $id"
  "$BLENDER" -b --python "$ROOT/pipelines/blender/cars/build.py" -- "$ROOT" "$id"
done
