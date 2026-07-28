#!/usr/bin/env bash
# Build the standalone macOS location helper (location-helper/) and stage it as a
# Tauri sidecar at src-tauri/binaries/notemd-location-<target-triple>. Tauri's
# externalBin picks up that arch-suffixed name, copies it into note.md.app as
# Contents/MacOS/notemd-location, and code-signs it as part of the app bundle.
#
#   scripts/build-location-helper.sh [target-triple]
#
# With no argument, builds for the host triple (used by beforeDevCommand /
# beforeBuildCommand so `tauri dev` and a plain `tauri build` find the binary).
# release.sh calls it once per arch before each `tauri build --target`.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

TRIPLE="${1:-}"
if [[ -z "$TRIPLE" ]]; then
  TRIPLE="$(rustc -vV | awk '/^host:/{print $2}')"
fi

echo "[location-helper] building for $TRIPLE"
cargo build --release --manifest-path location-helper/Cargo.toml --target "$TRIPLE"

OUT_DIR="src-tauri/binaries"
mkdir -p "$OUT_DIR"
SRC="location-helper/target/$TRIPLE/release/notemd-location"
DST="$OUT_DIR/notemd-location-$TRIPLE"
cp "$SRC" "$DST"

# Sanity: the embedded Info.plist section is what lets TCC read the location
# usage description for this bundled helper — fail loudly if it went missing.
if ! otool -s __TEXT __info_plist "$DST" >/dev/null 2>&1; then
  echo "[location-helper] ERROR: __TEXT,__info_plist section missing in $DST" >&2
  exit 1
fi
echo "[location-helper] staged $DST ($(lipo -archs "$DST"))"
