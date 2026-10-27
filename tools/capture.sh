#!/usr/bin/env bash
# Render a scene headlessly (Xvfb + lavapipe) and write one PNG per camera pose.
#
#   tools/capture.sh <scene> <out-dir> [racer-capture scene flags...]
#
# Set CAPTURE_PROFILE=release for an optimized build (default: dev).
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: tools/capture.sh <scene> <out-dir> [--size WxH] [--timeout SECS]" >&2
  exit 2
fi
scene="$1"
out="$2"
shift 2

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profile="${CAPTURE_PROFILE:-dev}"
target_dir="$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
[ "$profile" = dev ] && subdir=debug || subdir="$profile"

(cd "$ROOT" && cargo build --profile "$profile" -p racer-capture)

# Force the software Vulkan driver so captures are reproducible across hosts.
icd="$(ls /usr/share/vulkan/icd.d/lvp_icd*.json 2>/dev/null | head -n1 || true)"
if [ -n "$icd" ]; then
  export VK_DRIVER_FILES="$icd" VK_ICD_FILENAMES="$icd"
fi
export WGPU_BACKEND="${WGPU_BACKEND:-vulkan}"

xvfb-run -a -s "-screen 0 1920x1080x24" \
  "$target_dir/$subdir/racer-capture" scene "$scene" "$out" "$@"
"$target_dir/$subdir/racer-capture" check "$out"/*.png
