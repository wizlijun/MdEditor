#!/usr/bin/env bash
# Dev-install a v2 plugin into the local app-data plugins root.
#
# Usage: scripts/dev-install-plugin.sh [--release] [md2pdf|roam-import|openclaw|cef|pos-log|decision-log|weekly-review|claude-agent|ebook-import]
#   default plugin = md2pdf (preserves the original behavior).
#   --release      = build the native plugin binary in release mode (md2pdf +
#                    openclaw; ignored for the pure-UI plugins).
#
# md2pdf      → builds the CURRENT-arch native binary (fast dev loop; use
#               scripts/build-md2pdf-v2.sh for dual-arch release binaries) and
#               installs bin/ + manifest.
# roam-import → builds the CURRENT-arch native backend crate
#               (plugins-src/roam-import/backend → notemd-roam-import; roam CLI
#               discovery/probe) AND the standalone Vite UI bundle
#               (plugins-src/roam-import → dist/), then installs bin/ + ui/ +
#               manifest.
# openclaw    → builds BOTH the CURRENT-arch native backend crate
#               (plugins-src/openclaw/backend → notemd-openclaw) AND the
#               standalone Vite UI bundle (plugins-src/openclaw → dist/), then
#               installs bin/ + ui/ + manifest (backend process + streaming UI).
# pos-log     → builds the CURRENT-arch native backend crate
#               (plugins-src/pos-log/backend → notemd-pos-log; resident 30-min
#               location logger, no UI) and installs bin/ + manifest.
# claude-agent→ builds BOTH the CURRENT-arch native backend crate
#               (plugins-src/claude-agent/backend → notemd-claude-agent; the
#               headless runner plus its detached --runner mode) AND the
#               standalone Vite UI bundle, then installs bin/ + ui/ + manifest.
# ebook-import→ builds the CURRENT-arch native backend crate
#               (plugins-src/ebook-import/backend → notemd-ebook-import;
#               Calibre/HTMLZ/OCR pipeline + CLI, PDF rasterization via
#               macOS CoreGraphics -- no bundled dylib) AND the standalone
#               Vite UI bundle, then installs bin/ + ui/ + manifest.
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE=debug
PLUGIN=md2pdf
for arg in "$@"; do
  case "$arg" in
    --release) PROFILE=release ;;
    md2pdf|roam-import|openclaw|cef|pos-log|decision-log|weekly-review|claude-agent|ebook-import) PLUGIN="$arg" ;;
    *) echo "unknown arg: $arg (expected --release | md2pdf | roam-import | openclaw | cef | pos-log | decision-log | weekly-review | claude-agent | ebook-import)" >&2; exit 2 ;;
  esac
done

ROOT="$HOME/Library/Application Support/net.notemd.app/plugins"

# Update state.json: mark <id>@<version> installed + enabled.
mark_installed() {
  local id="$1" version="$2"
  node -e "
const fs=require('fs');const p='$ROOT/state.json';
const s=fs.existsSync(p)?JSON.parse(fs.readFileSync(p,'utf8')):{installed:{}};
s.installed['$id']={version:'$version',enabled:true};
fs.writeFileSync(p,JSON.stringify(s,null,2)+'\n');
"
}

if [[ "$PLUGIN" == "md2pdf" ]]; then
  ( cd plugins-src/md2pdf && cargo build $([ "$PROFILE" = release ] && echo --release) --bins )
  VERSION=$(node -e "console.log(require('./plugins-src/md2pdf/manifest.v2.json').version)")
  DEST="$ROOT/notemd.md2pdf/$VERSION"
  mkdir -p "$DEST/bin"
  cp plugins-src/md2pdf/target/$PROFILE/md2pdf "$DEST/bin/"
  cp plugins-src/md2pdf/target/$PROFILE/md2pdf-v2 "$DEST/bin/"
  cp plugins-src/md2pdf/manifest.v2.json "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.md2pdf/current"
  mark_installed "notemd.md2pdf" "$VERSION"
  echo "✓ installed notemd.md2pdf@$VERSION ($PROFILE, $(uname -m)) → $DEST"

elif [[ "$PLUGIN" == "roam-import" ]]; then
  SRC="plugins-src/roam-import"
  cargo build $([ "$PROFILE" = release ] && echo --release) \
    --manifest-path "$SRC/backend/Cargo.toml" --bin notemd-roam-import
  pnpm --filter roam-import-plugin build
  VERSION=$(node -e "console.log(require('./$SRC/manifest.v2.json').version)")
  DEST="$ROOT/notemd.roam-import/$VERSION"
  rm -rf "$DEST"; mkdir -p "$DEST/bin" "$DEST/ui"
  cp "$SRC/backend/target/$PROFILE/notemd-roam-import" "$DEST/bin/"
  cp -R "$SRC/dist/." "$DEST/ui/"
  cp "$SRC/manifest.v2.json" "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.roam-import/current"
  mark_installed "notemd.roam-import" "$VERSION"
  echo "✓ installed notemd.roam-import@$VERSION ($PROFILE, $(uname -m), backend + ui) → $DEST"

elif [[ "$PLUGIN" == "cef" ]]; then
  SRC="plugins-src/custom-editor-fixture"
  # Build the fixture (pure vanilla HTML → dist/editor.html; no framework needed).
  pnpm --filter cef-fixture-plugin build
  VERSION=$(node -e "console.log(require('./$SRC/manifest.v2.json').version)")
  DEST="$ROOT/notemd.cef-fixture/$VERSION"
  rm -rf "$DEST"
  mkdir -p "$DEST/ui"
  cp -R "$SRC/dist/." "$DEST/ui/"
  cp "$SRC/manifest.v2.json" "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.cef-fixture/current"
  mark_installed "notemd.cef-fixture" "$VERSION"
  echo "✓ installed notemd.cef-fixture@$VERSION (ui-only) → $DEST"
  echo "  probe:                  File ▸ 'New .cef fixture' → see plugins-src/custom-editor-fixture/PROBE.md"

elif [[ "$PLUGIN" == "openclaw" ]]; then
  SRC="plugins-src/openclaw"
  # 1) Build the CURRENT-arch native backend crate (the whole UDS/relay/pair
  #    state machine). --manifest-path keeps cargo out of the workspace root.
  cargo build $([ "$PROFILE" = release ] && echo --release) \
    --manifest-path "$SRC/backend/Cargo.toml" --bin notemd-openclaw
  # 2) Build the standalone UI bundle (dist/).
  pnpm --filter openclaw-plugin build
  VERSION=$(node -e "console.log(require('./$SRC/manifest.v2.json').version)")
  DEST="$ROOT/notemd.openclaw-chat/$VERSION"
  rm -rf "$DEST"
  mkdir -p "$DEST/bin" "$DEST/ui"
  cp "$SRC/backend/target/$PROFILE/notemd-openclaw" "$DEST/bin/notemd-openclaw"
  cp -R "$SRC/dist/." "$DEST/ui/"
  cp "$SRC/manifest.v2.json" "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.openclaw-chat/current"
  mark_installed "notemd.openclaw-chat" "$VERSION"
  echo "✓ installed notemd.openclaw-chat@$VERSION ($PROFILE, $(uname -m), backend + ui) → $DEST"
  echo "  open it:                Window menu ▸ \"OpenClaw (v2)\""

elif [[ "$PLUGIN" == "pos-log" ]]; then
  SRC="plugins-src/pos-log"
  # CURRENT-arch native backend (resident background logger; no UI).
  cargo build $([ "$PROFILE" = release ] && echo --release) \
    --manifest-path "$SRC/backend/Cargo.toml" --bin notemd-pos-log
  VERSION=$(node -e "console.log(require('./$SRC/manifest.v2.json').version)")
  DEST="$ROOT/notemd.pos-log/$VERSION"
  rm -rf "$DEST"
  mkdir -p "$DEST/bin"
  cp "$SRC/backend/target/$PROFILE/notemd-pos-log" "$DEST/bin/notemd-pos-log"
  cp "$SRC/manifest.v2.json" "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.pos-log/current"
  mark_installed "notemd.pos-log" "$VERSION"
  echo "✓ installed notemd.pos-log@$VERSION ($PROFILE, $(uname -m)) → $DEST"
  echo "  it activates on next app startup and logs to <vault>/pos/YYYY-MM-DD-pos.md"

elif [[ "$PLUGIN" == "decision-log" ]]; then
  SRC="plugins-src/decision-log"
  # Build the standalone UI bundle (dist/). Pure UI plugin; no native backend.
  pnpm --filter decision-log build
  VERSION=$(node -e "console.log(require('./$SRC/manifest.v2.json').version)")
  DEST="$ROOT/notemd.decision-log/$VERSION"
  rm -rf "$DEST"
  mkdir -p "$DEST/ui"
  cp -R "$SRC/dist/." "$DEST/ui/"
  cp "$SRC/manifest.v2.json" "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.decision-log/current"
  mark_installed "notemd.decision-log" "$VERSION"
  echo "✓ installed notemd.decision-log@$VERSION (ui-only) → $DEST"

elif [[ "$PLUGIN" == "weekly-review" ]]; then
  SRC="plugins-src/weekly-review"
  # Build the standalone UI bundle (dist/). Pure UI plugin; no native backend.
  pnpm --filter weekly-review build
  VERSION=$(node -e "console.log(require('./$SRC/manifest.v2.json').version)")
  DEST="$ROOT/notemd.weekly-review/$VERSION"
  rm -rf "$DEST"
  mkdir -p "$DEST/ui"
  cp -R "$SRC/dist/." "$DEST/ui/"
  cp "$SRC/manifest.v2.json" "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.weekly-review/current"
  mark_installed "notemd.weekly-review" "$VERSION"
  echo "✓ installed notemd.weekly-review@$VERSION (ui-only) → $DEST"

elif [[ "$PLUGIN" == "claude-agent" ]]; then
  SRC="plugins-src/claude-agent"
  # 1) CURRENT-arch native backend (headless runner: task discovery, task lock,
  #    claude child process + stream-json parsing, run records, --runner mode).
  cargo build $([ "$PROFILE" = release ] && echo --release) \
    --manifest-path "$SRC/backend/Cargo.toml" --bin notemd-claude-agent
  # 2) Standalone UI bundle (dist/).
  pnpm --filter claude-agent build
  VERSION=$(node -e "console.log(require('./$SRC/manifest.v2.json').version)")
  DEST="$ROOT/notemd.claude-agent/$VERSION"
  rm -rf "$DEST"
  mkdir -p "$DEST/bin" "$DEST/ui"
  cp "$SRC/backend/target/$PROFILE/notemd-claude-agent" "$DEST/bin/notemd-claude-agent"
  cp -R "$SRC/dist/." "$DEST/ui/"
  cp "$SRC/manifest.v2.json" "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.claude-agent/current"
  mark_installed "notemd.claude-agent" "$VERSION"
  echo "✓ installed notemd.claude-agent@$VERSION ($PROFILE, $(uname -m), backend + ui) → $DEST"
  echo "  open it:                Plugins menu ▸ \"Claude Agent…\" (restart the app first)"
  echo "  needs:                  Claude Code installed and logged in (claude --version)"

elif [[ "$PLUGIN" == "ebook-import" ]]; then
  SRC="plugins-src/ebook-import"
  cargo build $([ "$PROFILE" = release ] && echo --release) \
    --manifest-path "$SRC/backend/Cargo.toml" --bin notemd-ebook-import
  pnpm --filter ebook-import-plugin build
  VERSION=$(node -e "console.log(require('./$SRC/manifest.v2.json').version)")
  DEST="$ROOT/notemd.ebook-import/$VERSION"
  rm -rf "$DEST"; mkdir -p "$DEST/bin" "$DEST/ui"
  cp "$SRC/backend/target/$PROFILE/notemd-ebook-import" "$DEST/bin/"
  cp -R "$SRC/dist/." "$DEST/ui/"
  cp "$SRC/manifest.v2.json" "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.ebook-import/current"
  mark_installed "notemd.ebook-import" "$VERSION"
  echo "✓ installed notemd.ebook-import@$VERSION ($PROFILE, $(uname -m), backend + ui) → $DEST"
  echo "  open it:                Plugins menu ▸ \"导入电子书(epub、pdf、docx)…\""
  echo "  CLI:                    notemd ebook <file.epub|.pdf|.docx> [--ocr] [--ocr-provider wechat|baidu] [--root <vault-relative>]"
fi

# ---------------------------------------------------------------------------
# Manual E2E walkthrough — md2pdf (plugin-runtime-v2 plan, Task 12 Step 3):
#   1. scripts/dev-install-plugin.sh
#   2. pnpm tauri dev
#   3. File menu shows "Export to PDF (v2)…" → export an .md tab → PDF
#      written + success toast (emitted by the plugin via plugin-toast).
#   4. CLI: `notemd pdf2 x.md` (dev CLI, same flag) → PDF appears next to x.md.
#   5. Export again immediately → the long-running v2 process is reused.
#   6. Wait 120 s idle (idle_shutdown_seconds) → process exits; export once
#      more → lazy re-activation works.
# Automated fallback coverage lives in the Task 5/6/11 integration tests.
# ---------------------------------------------------------------------------
# Manual E2E walkthrough — roam-import (plugin-ui-mechanism plan ②, Task 6):
#   1. scripts/dev-install-plugin.sh roam-import
#   2. pnpm tauri dev  (with a Vault configured)
#   3. File ▸ Import ▸ "Roam Research (v2)" appears → click it.
#   4. A "Import from Roam Research" plugin window opens (plugin:// bridge).
#   5. Click the picker → choose a Roam .json export → import runs; progress
#      bar advances, then a success toast + summary banner (wiki/daily/skipped).
#   6. Files land in the vault: <vault>/<wikiDir>/*.note.md,
#      <vault>/<dailyDir>/<yyyy>/<yyyy-MM-dd>.note.md, and the incremental
#      manifest at <vault>/.notemd/roam-import.json.
#   7. Spot-diff a page against the v1 output (File ▸ Import ▸ "Roam Research")
#      run over the SAME export into a scratch vault: the .note.md text should
#      be byte-identical (same parse/plan/convert core; only the IO layer moved
#      to host RPC). Re-run the v2 import → unchanged pages report as skipped.
# ---------------------------------------------------------------------------
# Manual E2E walkthrough — openclaw (plugin-openclaw-migration plan ②b, Task 5):
#   1. scripts/dev-install-plugin.sh openclaw
#   2. pnpm tauri dev
#   3. Window menu ▸ "OpenClaw (v2)" → the OpenClaw chat window opens
#      (plugin:// bridge; the backend process is pre-activated on open so the
#      reader can stream frames immediately).
#   4. If unpaired: the onboarding screen appears → enter the host's pairing
#      code → pair_claim over the bridge → window reconnects.
#   5. Type a message → user.message frame goes UI→process→relay/UDS; the
#      agent's reply streams back token-by-token (agent.message.delta pushed via
#      host.ui.post, fanned out by onMessage → onFrame).
#   6. On the host side, approve a new device claim from the pending-claim toast
#      (pending-claim kind pushed by the 8s poller).
# ---------------------------------------------------------------------------
# Manual E2E probe — cef (custom-editor-fixture, 子项目④ Task 2):
#   1. scripts/dev-install-plugin.sh cef
#   2. pnpm tauri dev
#   3. File ▸ "New .cef fixture" → save dialog → save to ~/Desktop/test.cef
#      (or open any existing .cef file via File ▸ Open).
#   4. Follow the full probe checklist in plugins-src/custom-editor-fixture/PROBE.md.
#   Pass: (a)-(e) all green → base can migrate as a custom-editor tab (Task 4).
#   Fail: any blocker step fails → investigate iframe mechanism before migration.
# ---------------------------------------------------------------------------
# Manual E2E walkthrough — claude-agent:
#   1. scripts/dev-install-plugin.sh claude-agent
#   2. pnpm tauri dev   (with a Vault configured)
#   3. Plugins ▸ "Claude Agent…" → the window opens with two tasks in the left
#      column (selfcheck / answer-note-question), seeded on first activation into
#      <vault>/.notemd/agent-tasks/. The vault's .gitignore gains
#      .notemd/agent-runs/ and the settings.local.json line.
#      NOTE the menu is built at startup and the manifest is read then, so an
#      already-running app shows nothing — restart after installing.
#   4. Pick selfcheck → Run → tool calls and text stream in live; the footer
#      turns terminal. A record appears at
#      <vault>/.notemd/agent-runs/selfcheck/runs/*.json and in "Recent runs".
#   5. Pick answer-note-question → Run → hit Stop mid-run → status reads "Stopped";
#      `pgrep -f 'claude -p'` confirms the child process group was reaped.
#   6. CLI: `notemd agent selfcheck` → returns a run_id immediately (detached).
#      With the window OPEN, the selfcheck row turns "Running" with a pulsing
#      dot within ~5s (polled off the lock file, since that run is in another
#      process), then flips to its result; a trigger:"cli" row tagged CLI shows
#      up under "Recent runs" in the All-tasks scope.
#   7. CLI: `notemd agent selfcheck --wait` → blocks, then returns the result.
#   8. Run two DIFFERENT tasks at once → both proceed. Run the SAME task twice →
#      the second is refused with an "already running" toast.
#   If claude isn't on PATH: the window shows "claude executable not found";
#   point NOTEMD_CLAUDE_BIN at it and retry.
# ---------------------------------------------------------------------------
# Manual E2E walkthrough — ebook-import:
#   1. scripts/dev-install-plugin.sh ebook-import
#   2. pnpm tauri dev   (with a Vault configured)
#   3. Plugins menu ▸ "导入电子书(epub、pdf、docx)…" → the import window opens
#      (backend activates on open; the Calibre-detection settings row shows
#      found/not-found + version, since HTMLZ conversion needs Calibre's
#      `ebook-convert` on PATH or in the well-known app locations).
#   4. Drag an .epub (or .pdf/.docx) onto the window — the host forwards the
#      OS drag-drop event through to the plugin webview (Task 1's core
#      windows.rs plumbing) — or click "Add files" and pick one/several.
#   5. The queue runs jobs serially (one book converts at a time; the rest
#      wait "queued"); each row's status flips
#      queued → running → done/error live.
#   6. Per book, check
#      <vault>/ssot/ebooks/YYYY-MM/<书名>/ contains the three-piece set:
#      config.txt (bookread-format config), book.md (HTML→MD from the
#      Calibre HTMLZ), and images/ (localized image assets referenced from
#      book.md).
#   7. Click the row's editor button → book.md opens in a normal note.md tab.
#   8. Tick the OCR checkbox on a SCANNED PDF (no extractable text layer) and
#      pick a provider:
#        - 微信/wechat  → needs the intranet WeChat OCR URL reachable (set in
#          the plugin's device settings); progress advances page-by-page.
#        - 百度/baidu   → needs an API key/secret configured in Settings
#          first; submit/poll/download flow surfaces Baidu's error code +
#          message inline on failure.
#      Either way, book.md should contain the OCR'd text merged in page order.
#   9. CLI: `notemd ebook <file.epub|.pdf|.docx>` (dev CLI) does the same
#      import headless; add `--ocr` (+ `--ocr-provider wechat|baidu`) to
#      exercise the OCR path from the command line, and `--root
#      <vault-relative>` to override the default ssot/ebooks/ destination.
#  10. First run on a fresh machine needs Calibre installed
#      (https://calibre-ebook.com/) — until then the settings row reports
#      "not found" and HTMLZ conversion jobs fail fast with that reason
#      surfaced in the row's error text.
# ---------------------------------------------------------------------------
