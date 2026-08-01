#!/usr/bin/env bash
# Build the md2pdf plugin package binaries for both macOS architectures.
# Produces BOTH bins per arch: `md2pdf` (the main-thread renderer, spawned by
# the plugin as a sibling process per export) and `md2pdf-v2` (the long-running
# JSON-RPC service that fronts it).
#
# Nothing is copied into the app bundle — the package lives in app_data
# (see scripts/dev-install-plugin.sh). Artifacts stay in
# plugins-src/md2pdf/target/<triple>/release/.
set -euo pipefail
cd "$(dirname "$0")/.."

# Prefer rustup-managed toolchain over any system Rust (e.g. Homebrew) that
# may be earlier in PATH and lack cross-compilation std libraries.
export PATH="$HOME/.cargo/bin:$PATH"

echo "[md2pdf-v2] ensuring rustup targets…"
rustup target add aarch64-apple-darwin >/dev/null
rustup target add x86_64-apple-darwin >/dev/null

echo "[md2pdf-v2] cargo build --release --bins × 2…"
( cd plugins-src/md2pdf && cargo build --release --bins --target aarch64-apple-darwin )
( cd plugins-src/md2pdf && cargo build --release --bins --target x86_64-apple-darwin )

BINS=()
for triple in aarch64-apple-darwin x86_64-apple-darwin; do
  for bin in md2pdf md2pdf-v2; do
    BINS+=("plugins-src/md2pdf/target/$triple/release/$bin")
  done
done

chmod +x "${BINS[@]}"
strip "${BINS[@]}"

# Codesign with hardened runtime + secure timestamp so Apple notarization
# accepts the binaries when they ship inside a signed package. Same env-var
# pattern as scripts/build-md2pdf.sh.
APPLE_TEAM_ID="${APPLE_TEAM_ID:-T5G56DH47L}"
SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-}"
if [[ -z "$SIGNING_IDENTITY" ]]; then
  SIGNING_IDENTITY=$(
    security find-identity -v -p codesigning \
      | awk -F\" -v t="$APPLE_TEAM_ID" '/Developer ID Application/ && index($0,"("t")") {print $2; exit}'
  ) || true
fi
if [[ -z "$SIGNING_IDENTITY" ]]; then
  SIGNING_IDENTITY=$(
    security find-identity -v -p codesigning \
      | awk -F\" '/Developer ID Application/ {print $2; exit}'
  ) || true
fi
if [[ -n "$SIGNING_IDENTITY" ]]; then
  echo "[md2pdf-v2] codesign with: $SIGNING_IDENTITY"
  for b in "${BINS[@]}"; do
    codesign --force --options runtime --timestamp \
      --sign "$SIGNING_IDENTITY" "$b"
  done
else
  echo "[md2pdf-v2] WARNING: no Developer ID Application identity in keychain — binaries left unsigned"
fi

echo "[md2pdf-v2] binaries written (package these into the v2 install layout):"
ls -lh "${BINS[@]}"
