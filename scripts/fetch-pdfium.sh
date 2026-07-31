#!/usr/bin/env bash
# Fetch the prebuilt pdfium dylib (bblanchon/pdfium-binaries) for both macOS
# architectures and cache it under plugins-src/ebook-import/backend/vendor/,
# where the ebook-import backend's build/install/release scripts expect to
# find it (see src/ocr/pdfium.rs: NOTEMD_PDFIUM_PATH env override, else
# libpdfium.dylib next to the running binary).
#
# Usage: scripts/fetch-pdfium.sh [--force]
#   --force  re-download + overwrite even if the cached dylib already exists.
#
# Source release: https://github.com/bblanchon/pdfium-binaries (latest tag).
#   pdfium-mac-arm64.tgz → lib/libpdfium.dylib → vendor/aarch64-apple-darwin/
#   pdfium-mac-x64.tgz   → lib/libpdfium.dylib → vendor/x86_64-apple-darwin/
set -euo pipefail
cd "$(dirname "$0")/.."

FORCE=0
for arg in "$@"; do
  case "$arg" in
    --force) FORCE=1 ;;
    *) echo "unknown arg: $arg (expected --force)" >&2; exit 2 ;;
  esac
done

BASE_URL="https://github.com/bblanchon/pdfium-binaries/releases/latest/download"
VENDOR_ROOT="plugins-src/ebook-import/backend/vendor"

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

fetch_one() {
  local asset="$1" triple="$2"
  local dest_dir="$VENDOR_ROOT/$triple"
  local dest="$dest_dir/libpdfium.dylib"

  if [[ -f "$dest" && "$FORCE" -eq 0 ]]; then
    echo "[fetch-pdfium] $triple: already cached at $dest (use --force to re-fetch)"
    return
  fi

  local tgz="$WORKDIR/$asset"
  echo "[fetch-pdfium] $triple: downloading $BASE_URL/${asset}..."
  curl -fL --retry 3 -o "$tgz" "$BASE_URL/$asset"

  local extract_dir="$WORKDIR/${asset%.tgz}"
  mkdir -p "$extract_dir"
  tar -xzf "$tgz" -C "$extract_dir" lib/libpdfium.dylib

  mkdir -p "$dest_dir"
  cp "$extract_dir/lib/libpdfium.dylib" "$dest"
  echo "[fetch-pdfium] $triple: cached → $dest"
}

fetch_one "pdfium-mac-arm64.tgz" "aarch64-apple-darwin"
fetch_one "pdfium-mac-x64.tgz" "x86_64-apple-darwin"

echo "[fetch-pdfium] done."
