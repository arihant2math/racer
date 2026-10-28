#!/usr/bin/env bash
# Net check (release build): one dedicated server and 8 headless bot clients
# (the AI driving each client's car through its controls) on localhost, at
# three simulated network conditions: none, 150 ms RTT with 15 ms jitter and
# 1 % loss (the M3 exit criterion), 250 ms RTT with 25 ms jitter and 3 % loss.
#
#   tools/net-check.sh <out-dir> [races] [laps]
#
# Defaults: 2 races of 3 laps per condition (about 30 minutes). Writes
# <out-dir>/<condition>/net.md and exits non-zero if any condition fails.
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: tools/net-check.sh <out-dir> [races] [laps]" >&2
  exit 2
fi
out="$1"
races="${2:-2}"
laps="${3:-3}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target_dir="$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
(cd "$ROOT" && cargo build --release -p racer-capture -p racer-server)
capture="$target_dir/release/racer-capture"

status=0
for condition in 0,0,0 150,15,1 250,25,3; do
  dir="$out/rtt-${condition//,/-}"
  "$capture" net-race "$dir" --bots 8 --races "$races" --laps "$laps" \
    --conditions "$condition" --server "$target_dir/release/racer-server" || status=1
done
"$capture" net-cost | tee "$out/cost.txt"
exit "$status"
