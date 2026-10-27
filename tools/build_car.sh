#!/usr/bin/env bash
# Builds a car asset end to end with headless Blender:
#   tools/build_car.sh <car-id> [--no-renders]
# Writes assets/cars/<id>/{car,car_lod1,car_lod2}.glb + stats.json, renders and
# silhouette checks into docs/assets/<id>/. Needs tools/setup.sh --with-blender.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
id="${1:?usage: tools/build_car.sh <car-id> [--no-renders]}"
car="data/cars/$id"
out="assets/cars/$id"
docs="docs/assets/$id"
blender="$ROOT/.tools/blender/blender"
run() { "$blender" -b --factory-startup --python "$@" 2>&1 | grep -E '^\[|wrote|Error|Traceback|  File' || true; }

# Blueprint silhouettes and profiles (committed; refetches only if missing).
[ -f "$car/refs/blueprint/profiles.json" ] ||
  python3 pipelines/refs/blueprint_profiles.py "$car/asset.toml" "$car/refs/blueprint"
mkdir -p "$out" "$docs"
run pipelines/blender/build_car.py -- --car "$car" --out "$out"
test -s "$out/car.glb"
if [ "${2:-}" != "--no-renders" ]; then
  masks="$(mktemp -d)"
  run pipelines/blender/render_views.py -- --glb "$out/car.glb" --masks "$masks" --out "$docs"
  python3 pipelines/refs/silhouette_iou.py "$car" "$masks" --json "$docs/silhouette_iou.json"
  cp "$masks"/iou_*.png "$docs/" 2>/dev/null || true
  python3 pipelines/refs/compare_blueprint.py "$car" "$docs" "$docs/blueprint_compare.png"
  rm -rf "$masks"
fi
echo "==> $out"
ls -la "$out"
