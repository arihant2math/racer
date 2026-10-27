#!/usr/bin/env bash
# Drive every car (or the given ones) through the scripted maneuvers on the
# Avian test track headlessly, write CSVs and plot them to PNGs.
#
#   tools/telemetry.sh <out-dir> [car-id...]
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: tools/telemetry.sh <out-dir> [car-id...]" >&2
  exit 2
fi
out="$1"
shift
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
cargo build --release -p racer-capture
target/release/racer-capture telemetry "$out" "$@"

# The distro's matplotlib targets the distro Python; prefer whichever python3 can import it.
for py in python3 python3.12; do
  if command -v "$py" >/dev/null && "$py" -c "import matplotlib" 2>/dev/null; then
    exec "$py" tools/plot_telemetry.py "$out"/*.csv
  fi
done
echo "telemetry.sh: no python3 with matplotlib (run tools/setup.sh)" >&2
exit 1
