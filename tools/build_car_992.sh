#!/usr/bin/env bash
# Build the Porsche 992 game asset with headless Blender.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BLENDER="${BLENDER:-$(command -v blender || echo "$ROOT/.tools/blender/blender")}"
OUT="${1:-$ROOT/assets/cars/porsche-911-carrera-s-992}"
mkdir -p "$OUT"
"$BLENDER" -b --python "$ROOT/pipelines/blender/porsche_992/build.py" -- "$ROOT" "$OUT"
