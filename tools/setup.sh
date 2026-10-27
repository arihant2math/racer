#!/usr/bin/env bash
# Idempotent, non-interactive setup for a fresh Ubuntu 24.04 container or a
# GitHub Actions ubuntu runner. Installs the Rust toolchain, Bevy's Linux build
# dependencies and a software Vulkan stack (lavapipe + Xvfb) for headless
# rendering. Pass --with-blender to also install headless Blender into
# .tools/blender (only needed from M4 on).
set -euo pipefail

BLENDER_VERSION=5.2.3
WITH_BLENDER=0
for arg in "$@"; do
  case "$arg" in
    --with-blender) WITH_BLENDER=1 ;;
    -h | --help)
      echo "usage: tools/setup.sh [--with-blender]"
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# rustup reads the pinned toolchain from rust-toolchain.toml in the repo root.
cd "$ROOT"
SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo"; fi

APT_PACKAGES=(
  # Software Vulkan (lavapipe) and a virtual X server for headless rendering.
  mesa-vulkan-drivers xvfb xauth libxkbcommon-x11-0
  # Bevy build dependencies.
  pkg-config libwayland-dev libxkbcommon-dev libasound2-dev libudev-dev
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev
  # Toolchain bootstrap.
  build-essential curl ca-certificates xz-utils
)
if [ "$WITH_BLENDER" -eq 1 ]; then
  # Shared libraries the official Blender tarball needs in background mode.
  APT_PACKAGES+=(libxxf86vm1 libxfixes3 libxrender1 libgl1 libsm6 libxkbcommon0 libegl1)
fi

missing=()
for pkg in "${APT_PACKAGES[@]}"; do
  dpkg-query -W -f='${Status}' "$pkg" 2>/dev/null | grep -q "install ok installed" || missing+=("$pkg")
done
if [ "${#missing[@]}" -gt 0 ]; then
  echo "==> apt: installing ${missing[*]}"
  export DEBIAN_FRONTEND=noninteractive
  $SUDO apt-get update -q
  $SUDO apt-get install -y -q --no-install-recommends "${missing[@]}"
else
  echo "==> apt: all packages present"
fi

if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi
if ! command -v cargo >/dev/null 2>&1; then
  echo "==> rustup: installing rustup"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- -y --profile minimal --default-toolchain none
  export PATH="$HOME/.cargo/bin:$PATH"
fi
if command -v rustup >/dev/null 2>&1; then
  # Installs the toolchain and components pinned in rust-toolchain.toml.
  rustup toolchain install
fi
echo "==> $(cargo --version)"

if [ "$WITH_BLENDER" -eq 1 ]; then
  dest="$ROOT/.tools/blender-$BLENDER_VERSION"
  if [ -x "$dest/blender" ]; then
    echo "==> blender $BLENDER_VERSION already installed"
  else
    series="${BLENDER_VERSION%.*}"
    base="https://download.blender.org/release/Blender$series"
    tarball="blender-$BLENDER_VERSION-linux-x64.tar.xz"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    echo "==> blender: downloading $base/$tarball"
    curl -fsSL --retry 3 -o "$tmp/$tarball" "$base/$tarball"
    curl -fsSL --retry 3 -o "$tmp/sums" "$base/blender-$BLENDER_VERSION.sha256"
    (cd "$tmp" && grep " $tarball\$" sums | sha256sum -c -)
    mkdir -p "$tmp/x"
    tar -xJf "$tmp/$tarball" -C "$tmp/x"
    mkdir -p "$ROOT/.tools"
    rm -rf "$dest"
    mv "$tmp/x/blender-$BLENDER_VERSION-linux-x64" "$dest"
  fi
  ln -sfn "blender-$BLENDER_VERSION" "$ROOT/.tools/blender"
  "$ROOT/.tools/blender/blender" -b --factory-startup --python-expr \
    "import bpy; print('blender-ok', bpy.app.version_string)" 2>&1 | grep blender-ok
fi

# Confirm lavapipe is visible to the Vulkan loader.
if ls /usr/share/vulkan/icd.d/lvp_icd*.json >/dev/null 2>&1; then
  echo "==> lavapipe ICD present"
else
  echo "lavapipe ICD not found under /usr/share/vulkan/icd.d" >&2
  exit 1
fi
echo "==> setup complete"
