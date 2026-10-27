#!/usr/bin/env bash
# Download the optional CC0 Poly Haven textures listed in
# data/city/textures/polyhaven.toml into assets/textures/polyhaven/<id>/,
# verifying each file's md5. Idempotent; no account or API key needed.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$ROOT/data/city/textures/polyhaven.toml"
dest="$ROOT/assets/textures/polyhaven"
python3 -I - "$manifest" <<'PY' | while read -r id role url md5; do
import sys, tomllib
for t in tomllib.load(open(sys.argv[1], "rb"))["texture"]:
    for role in ("albedo", "normal", "orm"):
        print(t["id"], role, t[role]["url"], t[role]["md5"])
PY
  dir="$dest/$id"
  file="$dir/$role.jpg"
  mkdir -p "$dir"
  if [ -f "$file" ] && [ "$(md5sum "$file" | cut -d' ' -f1)" = "$md5" ]; then
    continue
  fi
  echo "==> $id $role"
  curl -fsSL --retry 3 -o "$file.part" "$url"
  got="$(md5sum "$file.part" | cut -d' ' -f1)"
  if [ "$got" != "$md5" ]; then
    echo "md5 mismatch for $url: $got != $md5" >&2
    rm -f "$file.part"
    exit 1
  fi
  mv "$file.part" "$file"
done
echo "==> textures in $dest"
