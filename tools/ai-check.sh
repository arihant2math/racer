#!/usr/bin/env bash
# Headless AI verification on the city track (release build):
#   solo laps for every car and difficulty, 8-car mixed races with contact,
#   and plots of the AI against the optimal speed profile.
#
#   tools/ai-check.sh <out-dir> [solo-laps] [races] [race-laps]
#
# Defaults: 100 solo laps per car and difficulty, 12 races of 10 laps.
# Exits non-zero if any check fails (see the reports in <out-dir>).
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: tools/ai-check.sh <out-dir> [solo-laps] [races] [race-laps]" >&2
  exit 2
fi
out="$1"
laps="${2:-100}"
races="${3:-12}"
race_laps="${4:-10}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# <out-dir> stays relative to the caller's directory; only cargo runs in the
# repository, and the binary is taken from wherever cargo puts it.
target_dir="$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
capture="$target_dir/release/racer-capture"
(cd "$ROOT" && cargo build --release -p racer-capture)
status=0
"$capture" ai-laps "$out" --laps "$laps" || status=1
"$capture" ai-races "$out" --races "$races" --laps "$race_laps" || status=1

for py in python3 python3.12; do
  if command -v "$py" >/dev/null && "$py" -c "import matplotlib" 2>/dev/null; then
    "$py" "$ROOT"/tools/plot_race.py "$out"
    exit "$status"
  fi
done
echo "ai-check.sh: no python3 with matplotlib (run tools/setup.sh); skipped plots" >&2
exit "$status"
