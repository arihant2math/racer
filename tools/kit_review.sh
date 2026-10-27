#!/usr/bin/env bash
# Export the track-side kit as OBJ and review it in headless Blender:
# contact-sheet render (Cycles, CPU) plus an independent mesh report.
#
#   tools/kit_review.sh <out-dir> [--samples N]
#
# Needs Blender from `tools/setup.sh --with-blender`.
set -euo pipefail
if [ "$#" -lt 1 ]; then
  echo "usage: tools/kit_review.sh <out-dir> [--samples N]" >&2
  exit 2
fi
out="$1"
shift
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
blender="$ROOT/.tools/blender/blender"
[ -x "$blender" ] || { echo "blender not found; run tools/setup.sh --with-blender" >&2; exit 1; }
mkdir -p "$out"
out="$(cd "$out" && pwd)"
(cd "$ROOT" && cargo run -q --release -p racer-citykit -- obj "$out/obj")
"$blender" -b --factory-startup --python "$ROOT/pipelines/blender/kit_review.py" -- \
  "$out/obj" "$out/contact_sheet.png" "$out/blender_report.json" "$@" 2>&1 | grep -E "kit-review|Error|Traceback" || true
test -s "$out/contact_sheet.png" && test -s "$out/blender_report.json"
