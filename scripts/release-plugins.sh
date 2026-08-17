#!/usr/bin/env bash
# Package + sign v2 plugins for the marketplace (子项目③ Task 5).
#
#   scripts/release-plugins.sh [--release] <plugin...>
#     plugin ∈ { md2pdf, roam-import, openclaw, pos-log,
#                decision-log, weekly-review, claude-agent, deepseek-agent, ebook-import,
#                idea-spark, power-mode }   (add a case below)
#     --release  currently a no-op flag reserved for build-profile parity with
#                dev-install-plugin.sh; the release builds below are always
#                release-profile.
#
# For each plugin this script:
#   1. Builds its artifacts by REUSING the existing build scripts
#      (md2pdf → scripts/build-md2pdf-v2.sh dual-arch bins;
#       roam-import/openclaw/claude-agent/deepseek-agent/ebook-import → dual-arch backend
#       crate + pnpm --filter <plugin> build → dist/).
#   2. Assembles the install-layout tree (manifest.json at root + bin/ and/or
#      ui/) in a temp staging dir, then ZIPs it into
#        dist-plugins/<id>/<version>/<arch>.notemdpkg
#      — one zip per arch for binary plugins, one universal.notemdpkg for
#      ui-only plugins. The ZIP format is REQUIRED: the Rust installer
#      (src-tauri/src/plugin_runtime/installer.rs) unpacks with the `zip` crate.
#   3. Detached-signs each package with minisign → <pkg>.minisig (sibling; the
#      client fetches the pkg URL + ".minisig").
#   4. Records the sha256 (shasum -a 256) — printed here; the index generator
#      (scripts/gen-plugin-index.mjs) recomputes it into index.json.
#   5. Drops a manifest.json copy next to the packages in
#      dist-plugins/<id>/<version>/ for gen-plugin-index.mjs to read.
#
# It DOES NOT upload. The tail prints the wrangler r2/kv commands as guidance.
#
# Signing key: env NOTEMD_PLUGIN_SIGNING_KEY (default ~/.tauri/notemd-plugins.key).
# Generate a keypair once with:
#     minisign -G -p notemd-plugins.pub -s ~/.tauri/notemd-plugins.key
# then paste the pub key line into PLUGIN_REGISTRY_PUBKEY in
#     src-tauri/src/plugin_runtime/market.rs
set -euo pipefail
cd "$(dirname "$0")/.."
REPO_ROOT="$PWD"

# ── args ──────────────────────────────────────────────────────────────────────
PLUGINS=()
for arg in "$@"; do
  case "$arg" in
    --release) : ;; # reserved; release builds are always release-profile
    md2pdf|roam-import|openclaw|pos-log|decision-log|weekly-review|claude-agent|deepseek-agent|ebook-import|idea-spark|power-mode) PLUGINS+=("$arg") ;;
    -h|--help)
      grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown arg: $arg (expected --release | md2pdf | roam-import | openclaw | pos-log | decision-log | weekly-review | claude-agent | deepseek-agent | ebook-import | idea-spark | power-mode)" >&2; exit 2 ;;
  esac
done
if [[ ${#PLUGINS[@]} -eq 0 ]]; then
  echo "usage: scripts/release-plugins.sh [--release] <md2pdf|roam-import|openclaw|pos-log|decision-log|weekly-review|claude-agent|deepseek-agent|ebook-import|idea-spark|power-mode>..." >&2
  exit 2
fi

# ── minisign preflight ────────────────────────────────────────────────────────
if ! command -v minisign >/dev/null 2>&1; then
  echo "ERROR: minisign not found on PATH." >&2
  echo "  Install it (brew install minisign) — signing is mandatory, not skippable." >&2
  exit 3
fi

SIGNING_KEY="${NOTEMD_PLUGIN_SIGNING_KEY:-$HOME/.tauri/notemd-plugins.key}"
if [[ ! -f "$SIGNING_KEY" ]]; then
  cat >&2 <<EOF
ERROR: plugin signing key not found: $SIGNING_KEY
  Generate a keypair once:
      minisign -G -p notemd-plugins.pub -s ~/.tauri/notemd-plugins.key
  Then paste the public key line into PLUGIN_REGISTRY_PUBKEY in
      src-tauri/src/plugin_runtime/market.rs
  Override the key path with NOTEMD_PLUGIN_SIGNING_KEY=/path/to/key.
EOF
  exit 3
fi

OUT_ROOT="$REPO_ROOT/dist-plugins"

# minisign -S wants the key passphrase on stdin. An unencrypted throwaway/CI key
# has an empty passphrase — feed a blank line so the tool never blocks on a TTY.
# A real passphrase-protected key can be handled by the operator running this
# interactively (they'll be prompted) — detect that by whether stdin is a tty.
sign_pkg() {
  local pkg="$1"
  if [[ -t 0 ]]; then
    minisign -S -s "$SIGNING_KEY" -m "$pkg"
  else
    printf '\n' | minisign -S -s "$SIGNING_KEY" -m "$pkg"
  fi
}

# Read a field from a plugin manifest.v2.json via node (no jq dependency).
manifest_field() {
  local manifest="$1" field="$2"
  node -e "const m=require('$manifest');const v=m['$field'];process.stdout.write(v==null?'':String(v))"
}

# Number of entries in a manifest's `binary` map (0 for a ui-only plugin).
manifest_binary_count() {
  local manifest="$1"
  node -e "const m=require('$manifest');process.stdout.write(String(Object.keys(m.binary||{}).length))"
}

# ZIP a staging dir into a .notemdpkg. Runs from inside the stage so paths are
# stored relative (manifest.json / bin/… / ui/…), which is what the installer's
# traversal guard expects. -X drops extra attrs; -r recurses.
#
# GUARD: a manifest declaring `binary` must never ship as `universal`.
# `is_process_plugin()` (src-tauri/src/plugin_runtime/commands.rs) returns true
# for ANY non-empty binary map, so such a plugin gets a process and a lifecycle
# — and a universal package, which by construction carries no bin/, gives it a
# process that does not exist. The window then fails to open too. roam-import
# shipped exactly this shape until 2026-08-03; the mistake is silent at package
# time and only surfaces on a user's machine, so it is caught here instead.
zip_pkg() {
  local stage="$1" pkg="$2"
  if [[ "$(basename "$pkg")" == "universal.notemdpkg" ]]; then
    local bins; bins="$(manifest_binary_count "$stage/manifest.json")"
    if [[ "$bins" != "0" ]]; then
      echo "ERROR: $(basename "$(dirname "$(dirname "$pkg")")") declares $bins binary target(s)" >&2
      echo "  but is being packaged as universal.notemdpkg, which carries no bin/." >&2
      echo "  A manifest with a \`binary\` map is a PROCESS plugin — package it per" >&2
      echo "  triple (see release_native_ui / release_native_bin), not universal." >&2
      exit 4
    fi
  fi
  rm -f "$pkg"
  ( cd "$stage" && zip -q -X -r "$pkg" . )
}

# ── md2pdf: native, per-arch binaries ─────────────────────────────────────────
release_md2pdf() {
  local id="notemd.md2pdf"
  local manifest="$REPO_ROOT/plugins-src/md2pdf/manifest.v2.json"
  local version; version="$(manifest_field "$manifest" version)"
  echo "== $id @ $version =="

  echo "[$id] building dual-arch binaries (scripts/build-md2pdf-v2.sh)…"
  bash "$REPO_ROOT/scripts/build-md2pdf-v2.sh"

  local out_dir="$OUT_ROOT/$id/$version"
  mkdir -p "$out_dir"
  cp "$manifest" "$out_dir/manifest.json"   # for gen-plugin-index.mjs

  for triple in aarch64-apple-darwin x86_64-apple-darwin; do
    local stage; stage="$(mktemp -d)"
    trap 'rm -rf "$stage"' RETURN
    mkdir -p "$stage/bin"
    cp "$manifest" "$stage/manifest.json"
    cp "$REPO_ROOT/plugins-src/md2pdf/target/$triple/release/md2pdf"    "$stage/bin/md2pdf"
    cp "$REPO_ROOT/plugins-src/md2pdf/target/$triple/release/md2pdf-v2" "$stage/bin/md2pdf-v2"
    chmod +x "$stage/bin/md2pdf" "$stage/bin/md2pdf-v2"

    local pkg="$out_dir/$triple.notemdpkg"
    zip_pkg "$stage" "$pkg"
    sign_pkg "$pkg"
    local sha; sha="$(shasum -a 256 "$pkg" | awk '{print $1}')"
    echo "[$id] $triple.notemdpkg  sha256=$sha  → $pkg"
    rm -rf "$stage"; trap - RETURN
  done
}

# ── roam-import: native backend + ui, per-arch packages. It was ui-only until
# the Roam CLI daily sync (2026-08-03) gave it a backend; the manifest declares
# a `binary` for both Darwin triples now, and `is_process_plugin()` keys off
# exactly that — so a `universal` package here would install a process plugin
# with no process. The guard in zip_pkg makes that unshippable. ─────────────
release_roam_import() {
  release_native_ui "notemd.roam-import" "$REPO_ROOT/plugins-src/roam-import" \
    "notemd-roam-import" "roam-import-plugin"
}

# ── decision-log: ui-only, single universal package ───────────────────────────
release_decision_log() {
  local id="notemd.decision-log"
  local src="$REPO_ROOT/plugins-src/decision-log"
  local manifest="$src/manifest.v2.json"
  local version; version="$(manifest_field "$manifest" version)"
  echo "== $id @ $version =="

  echo "[$id] building UI bundle (pnpm --filter decision-log build)…"
  pnpm --filter decision-log build

  local out_dir="$OUT_ROOT/$id/$version"
  mkdir -p "$out_dir"
  cp "$manifest" "$out_dir/manifest.json"   # for gen-plugin-index.mjs

  local stage; stage="$(mktemp -d)"
  trap 'rm -rf "$stage"' RETURN
  mkdir -p "$stage/ui"
  cp "$manifest" "$stage/manifest.json"
  cp -R "$src/dist/." "$stage/ui/"

  local pkg="$out_dir/universal.notemdpkg"
  zip_pkg "$stage" "$pkg"
  sign_pkg "$pkg"
  local sha; sha="$(shasum -a 256 "$pkg" | awk '{print $1}')"
  echo "[$id] universal.notemdpkg  sha256=$sha  → $pkg"
  rm -rf "$stage"; trap - RETURN
}

# ── idea-spark: ui-only, single universal package (same shape as decision-log)
release_idea_spark() {
  local id="notemd.idea-spark"
  local src="$REPO_ROOT/plugins-src/idea-spark"
  local manifest="$src/manifest.v2.json"
  local version; version="$(manifest_field "$manifest" version)"
  echo "== $id @ $version =="

  echo "[$id] building UI bundle (pnpm --filter idea-spark build)…"
  pnpm --filter idea-spark build

  local out_dir="$OUT_ROOT/$id/$version"
  mkdir -p "$out_dir"
  cp "$manifest" "$out_dir/manifest.json"   # for gen-plugin-index.mjs

  local stage; stage="$(mktemp -d)"
  trap 'rm -rf "$stage"' RETURN
  mkdir -p "$stage/ui"
  cp "$manifest" "$stage/manifest.json"
  cp -R "$src/dist/." "$stage/ui/"

  local pkg="$out_dir/universal.notemdpkg"
  zip_pkg "$stage" "$pkg"
  sign_pkg "$pkg"
  local sha; sha="$(shasum -a 256 "$pkg" | awk '{print $1}')"
  echo "[$id] universal.notemdpkg  sha256=$sha  → $pkg"
  rm -rf "$stage"; trap - RETURN
}

# ── power-mode: ui-only, single universal package (same shape as idea-spark)
release_power_mode() {
  local id="notemd.power-mode"
  local src="$REPO_ROOT/plugins-src/power-mode"
  local manifest="$src/manifest.v2.json"
  local version; version="$(manifest_field "$manifest" version)"
  echo "== $id @ $version =="

  echo "[$id] building UI bundle (pnpm --filter power-mode build)…"
  pnpm --filter power-mode build

  local out_dir="$OUT_ROOT/$id/$version"
  mkdir -p "$out_dir"
  cp "$manifest" "$out_dir/manifest.json"   # for gen-plugin-index.mjs

  local stage; stage="$(mktemp -d)"
  trap 'rm -rf "$stage"' RETURN
  mkdir -p "$stage/ui"
  cp "$manifest" "$stage/manifest.json"
  cp -R "$src/dist/." "$stage/ui/"

  local pkg="$out_dir/universal.notemdpkg"
  zip_pkg "$stage" "$pkg"
  sign_pkg "$pkg"
  local sha; sha="$(shasum -a 256 "$pkg" | awk '{print $1}')"
  echo "[$id] universal.notemdpkg  sha256=$sha  → $pkg"
  rm -rf "$stage"; trap - RETURN
}

# ── weekly-review: ui-only, single universal package (same shape as decision-log)
release_weekly_review() {
  local id="notemd.weekly-review"
  local src="$REPO_ROOT/plugins-src/weekly-review"
  local manifest="$src/manifest.v2.json"
  local version; version="$(manifest_field "$manifest" version)"
  echo "== $id @ $version =="

  echo "[$id] building UI bundle (pnpm --filter weekly-review build)…"
  pnpm --filter weekly-review build

  local out_dir="$OUT_ROOT/$id/$version"
  mkdir -p "$out_dir"
  cp "$manifest" "$out_dir/manifest.json"   # for gen-plugin-index.mjs

  local stage; stage="$(mktemp -d)"
  trap 'rm -rf "$stage"' RETURN
  mkdir -p "$stage/ui"
  cp "$manifest" "$stage/manifest.json"
  cp -R "$src/dist/." "$stage/ui/"

  local pkg="$out_dir/universal.notemdpkg"
  zip_pkg "$stage" "$pkg"
  sign_pkg "$pkg"
  local sha; sha="$(shasum -a 256 "$pkg" | awk '{print $1}')"
  echo "[$id] universal.notemdpkg  sha256=$sha  → $pkg"
  rm -rf "$stage"; trap - RETURN
}

# ── openclaw etc.: native backend + ui, per-arch packages ─────────────────────
# Shared shape (mirrors scripts/dev-install-plugin.sh, but dual-arch release):
# backend crate built per triple (--manifest-path keeps cargo out of the
# workspace root), bins codesigned like build-md2pdf-v2.sh, one Vite UI bundle,
# then one zip per triple containing manifest.json + bin/<name> + ui/.
release_native_ui() {
  local id="$1" src="$2" bin_name="$3" pnpm_filter="$4"
  local manifest="$src/manifest.v2.json"
  local version; version="$(manifest_field "$manifest" version)"
  echo "== $id @ $version =="

  export PATH="$HOME/.cargo/bin:$PATH"
  echo "[$id] building dual-arch backend ($bin_name)…"
  for triple in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target add "$triple" >/dev/null
    cargo build --release --manifest-path "$src/backend/Cargo.toml" \
      --bin "$bin_name" --target "$triple"
  done

  local identity
  identity=$(security find-identity -v -p codesigning \
    | awk -F\" '/Developer ID Application/ {print $2; exit}') || true
  if [[ -n "$identity" ]]; then
    echo "[$id] codesign with: $identity"
    for triple in aarch64-apple-darwin x86_64-apple-darwin; do
      codesign --force --options runtime --timestamp --sign "$identity" \
        "$src/backend/target/$triple/release/$bin_name"
    done
  else
    echo "[$id] WARNING: no Developer ID Application identity — binaries left unsigned"
  fi

  echo "[$id] building UI bundle (pnpm --filter $pnpm_filter build)…"
  pnpm --filter "$pnpm_filter" build

  local out_dir="$OUT_ROOT/$id/$version"
  mkdir -p "$out_dir"
  cp "$manifest" "$out_dir/manifest.json"   # for gen-plugin-index.mjs

  for triple in aarch64-apple-darwin x86_64-apple-darwin; do
    local stage; stage="$(mktemp -d)"
    trap 'rm -rf "$stage"' RETURN
    mkdir -p "$stage/bin" "$stage/ui"
    cp "$manifest" "$stage/manifest.json"
    cp "$src/backend/target/$triple/release/$bin_name" "$stage/bin/$bin_name"
    chmod +x "$stage/bin/$bin_name"
    cp -R "$src/dist/." "$stage/ui/"

    local pkg="$out_dir/$triple.notemdpkg"
    zip_pkg "$stage" "$pkg"
    sign_pkg "$pkg"
    local sha; sha="$(shasum -a 256 "$pkg" | awk '{print $1}')"
    echo "[$id] $triple.notemdpkg  sha256=$sha  → $pkg"
    rm -rf "$stage"; trap - RETURN
  done
}

release_openclaw() {
  release_native_ui "notemd.openclaw-chat" "$REPO_ROOT/plugins-src/openclaw" \
    "notemd-openclaw" "openclaw-plugin"
}

release_claude_agent() {
  release_native_ui "notemd.claude-agent" "$REPO_ROOT/plugins-src/claude-agent" \
    "notemd-claude-agent" "claude-agent"
}

# deepseek-agent: an ACP client. Only the plugin binary ships -- the crate's
# second bin (`stub-acp`) is a test fixture and release_native_ui builds by
# --bin name, so it is never packaged.
release_deepseek_agent() {
  release_native_ui "notemd.deepseek-agent" "$REPO_ROOT/plugins-src/deepseek-agent" \
    "notemd-deepseek-agent" "deepseek-agent"
}

# ── ebook-import: native backend + ui, per-arch packages. PDF rasterization
# uses macOS CoreGraphics (a system framework) -- no vendored dylib to fetch
# or bundle. ──────────────────────────────────────────────────────────────
release_ebook_import() {
  release_native_ui "notemd.ebook-import" "$REPO_ROOT/plugins-src/ebook-import" \
    "notemd-ebook-import" "ebook-import-plugin"
}

# ── pos-log: native backend only, per-arch packages (no ui/) ─────────────────
release_native_bin() {
  local id="$1" src="$2" bin_name="$3"
  local manifest="$src/manifest.v2.json"
  local version; version="$(manifest_field "$manifest" version)"
  echo "== $id @ $version =="

  export PATH="$HOME/.cargo/bin:$PATH"
  echo "[$id] building dual-arch backend ($bin_name)…"
  for triple in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target add "$triple" >/dev/null
    cargo build --release --manifest-path "$src/backend/Cargo.toml" \
      --bin "$bin_name" --target "$triple"
  done

  local identity
  identity=$(security find-identity -v -p codesigning \
    | awk -F\" '/Developer ID Application/ {print $2; exit}') || true
  if [[ -n "$identity" ]]; then
    echo "[$id] codesign with: $identity"
    for triple in aarch64-apple-darwin x86_64-apple-darwin; do
      codesign --force --options runtime --timestamp --sign "$identity" \
        "$src/backend/target/$triple/release/$bin_name"
    done
  else
    echo "[$id] WARNING: no Developer ID Application identity — binaries left unsigned"
  fi

  local out_dir="$OUT_ROOT/$id/$version"
  mkdir -p "$out_dir"
  cp "$manifest" "$out_dir/manifest.json"   # for gen-plugin-index.mjs

  for triple in aarch64-apple-darwin x86_64-apple-darwin; do
    local stage; stage="$(mktemp -d)"
    trap 'rm -rf "$stage"' RETURN
    mkdir -p "$stage/bin"
    cp "$manifest" "$stage/manifest.json"
    cp "$src/backend/target/$triple/release/$bin_name" "$stage/bin/$bin_name"
    chmod +x "$stage/bin/$bin_name"

    local pkg="$out_dir/$triple.notemdpkg"
    zip_pkg "$stage" "$pkg"
    sign_pkg "$pkg"
    local sha; sha="$(shasum -a 256 "$pkg" | awk '{print $1}')"
    echo "[$id] $triple.notemdpkg  sha256=$sha  → $pkg"
    rm -rf "$stage"; trap - RETURN
  done
}

release_pos_log() {
  release_native_bin "notemd.pos-log" "$REPO_ROOT/plugins-src/pos-log" "notemd-pos-log"
}

for plugin in "${PLUGINS[@]}"; do
  case "$plugin" in
    md2pdf)      release_md2pdf ;;
    roam-import) release_roam_import ;;
    openclaw)    release_openclaw ;;
    pos-log)     release_pos_log ;;
    decision-log) release_decision_log ;;
    weekly-review) release_weekly_review ;;
    claude-agent) release_claude_agent ;;
    deepseek-agent) release_deepseek_agent ;;
    ebook-import) release_ebook_import ;;
    idea-spark)  release_idea_spark ;;
    power-mode)  release_power_mode ;;
  esac
done

echo
echo "── packages signed under dist-plugins/. NEXT (upload is a user step) ──────"
echo "  1) Regenerate the registry index:"
echo "       node scripts/gen-plugin-index.mjs"
echo "  2) Upload each package + its .minisig to R2 (per arch), e.g.:"
echo "       wrangler r2 object put notemd-plugins/<id>/<version>/<arch>.notemdpkg \\"
echo "         --file dist-plugins/<id>/<version>/<arch>.notemdpkg"
echo "       wrangler r2 object put notemd-plugins/<id>/<version>/<arch>.notemdpkg.minisig \\"
echo "         --file dist-plugins/<id>/<version>/<arch>.notemdpkg.minisig"
echo "  3) Publish the index to KV:"
echo "       wrangler kv key put index --path dist-plugins/index.json"
