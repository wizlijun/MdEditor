#!/usr/bin/env bash
#
# One-shot release helper for note.md.
#
#   scripts/release.sh [version] [--draft] [--prerelease]
#
# Version numbers are DATE-BASED and auto-derived when omitted:
#   MAJOR = current year - 2020        (2026 → 6, 2030 → 10; strictly increasing)
#   MINOR = month*100 + day            (Jul 15 → 715, Jan 5 → 105, Dec 31 → 1231)
#   PATCH = Nth release today, 1-based (counts existing v<MAJOR>.<MINOR>.* tags)
# e.g. the 3rd release on 2026-07-15 → 6.715.3. This ordering increases
# monotonically forever, which the Tauri updater requires.
#
# Examples:
#   scripts/release.sh                 # auto: today's date, next patch
#   scripts/release.sh --draft         # auto version, draft release
#   scripts/release.sh 6.715.3         # explicit override (must be x.y.z)
#
# Builds produce TWO independent per-arch macOS `.dmg`s: aarch64 (Apple Silicon)
# and x86_64 (Intel). Each architecture has its own .app bundle, dmg, updater
# tarball and signature. universal mode has been removed.
#
# Steps:
#   pre-flight → tests → bump versions
#   → for each arch in (aarch64, x86_64):
#       signed per-arch build → notarize → updater artifact + signature
#   → latest.json manifest (per-arch signatures + urls)
#   → tag → push → GitHub release (upload 2 dmg + 2 tarball + 2 sig + latest.json)
#
# Environment (auto-loaded from `.env.release` in repo root if present):
#   APPLE_TEAM_ID   default: T5G56DH47L (Wuhan Fulin). Used to locate the
#                   signing identity in the login keychain. Order of preference:
#                   "Developer ID Application" → "Apple Distribution".
#   APPLE_ID            App-Store-Connect Apple ID for notarization.
#   APPLE_PASSWORD      App-specific password (not the actual Apple ID password).
#   GH_REPO         default: wizlijun/note.md
#
# Updater signing (required — release will fail without these):
#   TAURI_SIGNING_PRIVATE_KEY           private key string OR
#   TAURI_SIGNING_PRIVATE_KEY_PATH      path to private key file (default: ~/.tauri/mdeditor.key)
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD  留空即可(我们的 key 无密码),但注意:
#                   Tauri 判的是**变量存不存在**,不是值是否为空。变量缺失时它
#                   会打印 "expect a prompt for password" 并阻塞等 stdin —— 非
#                   交互会话里就永远挂着。所以下面第 ~125 行无条件 export 成空
#                   串,不能删。Windows 侧同理,且 PowerShell 的 `$env:X=""` 是
#                   *删除*变量,必须用 [Environment]::SetEnvironmentVariable。
#
# When all three notarization vars are set AND the signing cert is Developer ID,
# Tauri's bundler runs `notarytool` automatically and the resulting .dmg passes
# Gatekeeper on first launch. Missing any var → unsigned/uninspected → user
# sees the "unidentified developer" warning, AND auto-update will fail because
# the replacement .app gets blocked.

set -euo pipefail

# Prefer rustup-managed rustc/cargo so cross-compile targets resolve.
# Some macs have Homebrew rust earlier in PATH which lacks the alternate-arch
# std libraries (causes E0463 "can't find crate for `core`" during the
# x86_64 leg of universal builds).
export PATH="$HOME/.cargo/bin:$PATH"

# ---------- load secrets (kept out of git) ----------

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [[ -f "$ROOT/.env.release" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "$ROOT/.env.release"
  set +a
fi

say() { printf '\033[1;34m▸\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; exit 1; }

# ---------- args ----------

VERSION=""
DRAFT=0; PRERELEASE=0
for arg in "$@"; do
  case "$arg" in
    --universal)
      echo "warning: --universal is no longer supported; building per-arch dmgs instead" >&2
      ;;
    --draft)      DRAFT=1      ;;
    --prerelease) PRERELEASE=1 ;;
    -*) echo "unknown flag: $arg" >&2; exit 2 ;;
    *)
      [[ -z "$VERSION" ]] || { echo "unexpected extra argument: $arg" >&2; exit 2; }
      VERSION="$arg"
      ;;
  esac
done

# Date-based auto-derivation when no explicit version was passed. Fetch tags
# first so today's patch count reflects releases cut on other machines too.
if [[ -z "$VERSION" ]]; then
  git fetch origin --tags --quiet 2>/dev/null || true
  major=$(( $(date +%Y) - 2020 ))
  minor=$(( 10#$(date +%m) * 100 + 10#$(date +%d) ))   # 10# forces base-10 (no octal)
  # `|| true`: with no matching tag yet (the day's first release), grep exits 1
  # and set -euo pipefail would abort the whole script. Empty → patch 1.
  last=$(git tag --list "v${major}.${minor}.*" \
    | sed -E "s/^v${major}\.${minor}\.//" \
    | grep -E '^[0-9]+$' | sort -n | tail -1 || true)
  VERSION="${major}.${minor}.$(( ${last:-0} + 1 ))"
  say "auto-derived date-based version $VERSION"
fi

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "version must be x.y.z (got: $VERSION)" >&2
  exit 2
fi

TAG="v$VERSION"
APPLE_TEAM_ID="${APPLE_TEAM_ID:-T5G56DH47L}"
GH_REPO="${GH_REPO:-wizlijun/note.md}"

# Resolve Tauri updater signing key. Prefer explicit env, then path env, then default file.
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  TAURI_SIGNING_PRIVATE_KEY_PATH="${TAURI_SIGNING_PRIVATE_KEY_PATH:-$HOME/.tauri/mdeditor.key}"
  if [[ ! -f "$TAURI_SIGNING_PRIVATE_KEY_PATH" ]]; then
    die "updater private key not found at $TAURI_SIGNING_PRIVATE_KEY_PATH

Either set TAURI_SIGNING_PRIVATE_KEY in .env.release, or generate a fresh keypair:
  pnpm tauri signer generate -w ~/.tauri/mdeditor.key
and put the matching public key into src-tauri/tauri.conf.json (plugins.updater.pubkey)."
  fi
  export TAURI_SIGNING_PRIVATE_KEY="$(cat "$TAURI_SIGNING_PRIVATE_KEY_PATH")"
fi
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

cd "$ROOT"

# ---------- pre-flight ----------

say "pre-flight"

git diff --quiet && git diff --cached --quiet \
  || die "working tree is dirty — commit or stash first"

BRANCH=$(git rev-parse --abbrev-ref HEAD)
[[ "$BRANCH" == "main" ]] || die "not on main (current: $BRANCH)"

git fetch origin main --quiet
LOCAL=$(git rev-parse @)
REMOTE=$(git rev-parse '@{u}')
[[ "$LOCAL" == "$REMOTE" ]] || die "local main differs from origin/main — pull/push first"

git tag --list | grep -qx "$TAG" \
  && die "tag $TAG already exists locally"
git ls-remote --tags origin "refs/tags/$TAG" | grep -q . \
  && die "tag $TAG already exists on origin"

command -v pnpm  >/dev/null || die "pnpm not found"
command -v gh    >/dev/null || die "gh not found"
command -v cargo >/dev/null || die "cargo not found"

# Detach any leftover bundle_dmg.sh random-mount points from previous failed
# runs. They show up as /Volumes/dmg.XXXXXX. Tauri's dmg packaging step
# (bundle_dmg.sh) silently fails if a conflicting mount exists.
LEFTOVER_MOUNTS=$(ls -d /Volumes/dmg.* 2>/dev/null || true)
if [[ -n "$LEFTOVER_MOUNTS" ]]; then
  echo "    detaching stuck dmg mounts:"
  for m in $LEFTOVER_MOUNTS; do
    echo "      $m"
    hdiutil detach "$m" -force >/dev/null 2>&1 || true
  done
fi

# Signing identity — prefer Developer ID Application (Gatekeeper-friendly when
# notarized) and fall back to Apple Distribution (App-Store-only; never passes
# Gatekeeper for direct downloads even with notarization).
APPLE_SIGNING_IDENTITY=$(
  security find-identity -v -p codesigning \
    | awk -F\" -v t="$APPLE_TEAM_ID" '/Developer ID Application/ && index($0,"("t")") {print $2; exit}'
)
SIGNING_KIND="Developer ID Application"
if [[ -z "$APPLE_SIGNING_IDENTITY" ]]; then
  APPLE_SIGNING_IDENTITY=$(
    security find-identity -v -p codesigning \
      | awk -F\" -v t="$APPLE_TEAM_ID" '/Apple Distribution/ && index($0,"("t")") {print $2; exit}'
  )
  SIGNING_KIND="Apple Distribution (Gatekeeper will block direct downloads)"
fi
[[ -n "$APPLE_SIGNING_IDENTITY" ]] \
  || die "no Developer ID or Apple Distribution cert for team $APPLE_TEAM_ID in keychain"
echo "    signing as: $APPLE_SIGNING_IDENTITY"
echo "    cert kind:  $SIGNING_KIND"

# Notarization triplet — Tauri's bundler picks these up automatically. Missing
# any one → bundler skips notarization and prints a warning. We surface it
# here so the user sees it before a 5-minute build instead of after.
NOTARIZE_OK=1
for var in APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID; do
  if [[ -z "${!var:-}" ]]; then
    NOTARIZE_OK=0
    printf '\033[1;33m!\033[0m notarization var %s not set\n' "$var" >&2
  fi
done
if (( NOTARIZE_OK )); then
  echo "    notarize:   yes (APPLE_ID=$APPLE_ID, team=$APPLE_TEAM_ID)"
else
  echo "    notarize:   no — first-launch Gatekeeper warning will appear"
fi
if (( NOTARIZE_OK )) && [[ "$SIGNING_KIND" != "Developer ID Application" ]]; then
  printf '\033[1;33m!\033[0m notarization vars set but cert is %s; notarization may still fail\n' "$SIGNING_KIND" >&2
fi

# ---------- tests ----------

say "running tests"
pnpm -s test
pnpm -s check:protocol

# ---------- bump versions ----------

say "bumping versions to $VERSION"

revert_bumps() {
  git checkout -- \
    package.json \
    src-tauri/tauri.conf.json \
    src-tauri/Cargo.toml \
    src-tauri/Cargo.lock 2>/dev/null || true
}
trap 'revert_bumps' ERR

python3 - "$VERSION" <<'PY'
import json, sys
v = sys.argv[1]
for p in ("package.json", "src-tauri/tauri.conf.json"):
    with open(p) as f: d = json.load(f)
    d["version"] = v
    with open(p, "w") as f:
        json.dump(d, f, indent=2)
        f.write("\n")
PY

# Cargo.toml: only the [package] version line at the top
sed -i '' "1,/^version = /s/^version = \"[^\"]*\"/version = \"$VERSION\"/" src-tauri/Cargo.toml

grep -q "\"version\": \"$VERSION\""             package.json              || die "bump failed: package.json"
grep -q "\"version\": \"$VERSION\""             src-tauri/tauri.conf.json || die "bump failed: tauri.conf.json"
grep -q "^version = \"$VERSION\"$"              src-tauri/Cargo.toml      || die "bump failed: Cargo.toml"

# ---------- build ----------

say "building (signed)"

rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null 2>&1 || true

# Build each architecture independently. macOS bash 3.2 lacks associative
# arrays, so we stash results in arch-suffixed variables (e.g. DMG_STAGED_AARCH64).
STAGED_ASSETS=()

build_arch() {
  local arch="$1" arch_tag="$2"
  say "building target $arch"
  APPLE_SIGNING_IDENTITY="$APPLE_SIGNING_IDENTITY" pnpm tauri build --target "$arch"

  local bundle="src-tauri/target/$arch/release/bundle"
  local dmg_src tarball_src sig_src
  # Tauri uses inconsistent arch tags in dmg filenames: 'aarch64' for arm64
  # but 'x64' (not 'x86_64') for Intel. Since each target dir contains exactly
  # one dmg, match by version and ignore the arch suffix.
  dmg_src=$(find "$bundle/dmg" -maxdepth 1 -type f -name "*_${VERSION}_*.dmg" -print -quit)
  tarball_src=$(find "$bundle/macos" -maxdepth 1 -type f -name "*.app.tar.gz" -print -quit)
  sig_src=$(find "$bundle/macos" -maxdepth 1 -type f -name "*.app.tar.gz.sig" -print -quit)
  [[ -n "$dmg_src"     && -f "$dmg_src"     ]] || die "dmg not found for $arch in $bundle/dmg"
  [[ -n "$tarball_src" && -f "$tarball_src" ]] || die "updater tarball not found for $arch — is createUpdaterArtifacts on and TAURI_SIGNING_PRIVATE_KEY set?"
  [[ -n "$sig_src"     && -f "$sig_src"     ]] || die "updater signature not found for $arch — Tauri did not sign the tarball"

  local dmg_staged="/tmp/note.md-${VERSION}-${arch_tag}.dmg"
  local tarball_staged="/tmp/note.md-${arch_tag}.app.tar.gz"
  local sig_staged="/tmp/note.md-${arch_tag}.app.tar.gz.sig"
  cp "$dmg_src" "$dmg_staged"
  cp "$tarball_src" "$tarball_staged"

  # Fail-fast architecture self-check. Unpack the staged tarball and confirm its
  # inner Mach-O really matches this arch. Guards against a cross-build gone
  # wrong or a staging mix-up putting x86 bytes in the aarch64 asset — the
  # v6.720.1 "ARM upgrade became x86, won't launch" incident class. dmg and
  # tarball wrap the same .app, so checking the tarball covers the update path.
  local want_arch
  case "$arch_tag" in
    aarch64) want_arch="arm64" ;;
    x86_64)  want_arch="x86_64" ;;
    *) die "unknown arch_tag '$arch_tag' — cannot arch-check" ;;
  esac
  local arch_check_dir="/tmp/note.md-archcheck-${arch_tag}"
  rm -rf "$arch_check_dir"; mkdir -p "$arch_check_dir"
  tar xzf "$tarball_staged" -C "$arch_check_dir"
  local inner_bin got_arch
  inner_bin=$(find "$arch_check_dir" -path '*/Contents/MacOS/notemd' -type f -print -quit)
  [[ -n "$inner_bin" ]] || die "arch-check: no Contents/MacOS/notemd in staged $arch_tag tarball"
  got_arch=$(lipo -archs "$inner_bin" 2>/dev/null || true)
  [[ "$got_arch" == "$want_arch" ]] \
    || die "arch-check FAILED for $arch_tag: staged tarball binary is '$got_arch', expected '$want_arch' — refusing to publish a mismatched-arch update"
  rm -rf "$arch_check_dir"
  echo "    ${arch_tag} arch verified: binary is $got_arch"

  # DO NOT trust Tauri's own .sig ($sig_src). Tauri signs the updater tarball
  # BEFORE notarization staples the .app, so its .sig is for a stale, pre-staple
  # tarball. The tarball we actually distribute differs, so every client rejects
  # the update with "The signature verification failed" (shipped broken in
  # v5.0.2). Re-sign the EXACT bytes we upload so signature and tarball always
  # match. `tauri signer sign` writes "<file>.sig" == $sig_staged.
  say "re-signing updater tarball for $arch_tag (post-notarize bytes)"
  rm -f "$sig_staged"
  # Key and password must NOT go on the command line: pnpm echoes the full
  # command to stderr (which `>/dev/null` does not catch — v6.813.5's build
  # log carried the private key verbatim), and argv is readable in `ps` for
  # the command's whole runtime. The tauri CLI reads both from the
  # TAURI_SIGNING_PRIVATE_KEY(_PASSWORD) env vars, exported above — in fact
  # passing `-k`/`-f` *conflicts* with the env var being set.
  pnpm tauri signer sign "$tarball_staged" >/dev/null
  [[ -f "$sig_staged" ]] || die "re-sign failed: $sig_staged not produced for $arch_tag"

  # Fail-fast: if minisign is available, verify the fresh pair against the
  # public key baked into the app. Catches any future re-break at build time
  # instead of at every user's update attempt.
  if command -v minisign >/dev/null 2>&1; then
    local pub_line raw_sig
    pub_line=$(python3 -c "import base64,json;print(base64.b64decode(json.load(open('src-tauri/tauri.conf.json'))['plugins']['updater']['pubkey']).decode().splitlines()[1])")
    raw_sig="${sig_staged}.raw"
    base64 -D -i "$sig_staged" -o "$raw_sig"
    minisign -V -P "$pub_line" -m "$tarball_staged" -x "$raw_sig" >/dev/null \
      || die "re-signed updater tarball failed verification for $arch_tag"
    rm -f "$raw_sig"
    echo "    ${arch_tag} updater signature verified against app pubkey"
  else
    echo "    (minisign not installed — skipping build-time signature self-check)"
  fi

  # Export results via indirect names so the caller can pick them up. macOS
  # bash 3.2-friendly (no ${var^^} uppercase substitution).
  local up_tag
  up_tag=$(echo "$arch_tag" | tr '[:lower:]' '[:upper:]')
  eval "DMG_STAGED_${up_tag}=\"$dmg_staged\""
  eval "TARBALL_STAGED_${up_tag}=\"$tarball_staged\""
  eval "SIG_STAGED_${up_tag}=\"$sig_staged\""
  eval "SIG_CONTENT_${up_tag}=\"$(cat "$sig_staged")\""

  STAGED_ASSETS+=("$dmg_staged" "$tarball_staged" "$sig_staged")
  echo "    ${arch_tag} done: dmg=$(du -h "$dmg_staged" | cut -f1), tarball=$(du -h "$tarball_staged" | cut -f1)"
}

build_arch aarch64-apple-darwin aarch64
build_arch x86_64-apple-darwin  x86_64

# ---------- commit, tag, push ----------

say "committing v$VERSION"
trap - ERR  # build succeeded — keep version bumps even if a later step fails
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore: release v$VERSION"
git tag -a "$TAG" -m "note.md $VERSION"

say "pushing to origin"
git push origin main
git push origin "$TAG"

# ---------- release ----------

say "creating GitHub release"

PREAMBLE=$(cat <<EOF
## Install

Pick the dmg matching your Mac's chip:

- **Apple Silicon (M1/M2/M3/…):** \`note.md-${VERSION}-aarch64.dmg\`
- **Intel:** \`note.md-${VERSION}-x86_64.dmg\`

> Code-signed with Developer ID Application (\`$APPLE_TEAM_ID\`), hardened runtime, notarized.
> Auto-update from a previous installed version picks the correct architecture automatically.

EOF
)

# Auto-generate "What's Changed" body from the GitHub API and prepend the install preamble.
GENERATED=$(
  gh api -X POST "repos/$GH_REPO/releases/generate-notes" \
    -f tag_name="$TAG" \
    -f target_commitish="main" \
    --jq .body 2>/dev/null \
  || true
)

NOTES="${PREAMBLE}${GENERATED}"

EXTRA=()
(( DRAFT ))      && EXTRA+=(--draft)
(( PRERELEASE )) && EXTRA+=(--prerelease)

# Generate latest.json — the updater manifest that the app polls. Each arch
# key points to its own tarball + signature.
TARBALL_URL_AARCH64="https://github.com/$GH_REPO/releases/download/$TAG/note.md-aarch64.app.tar.gz"
TARBALL_URL_X86_64="https://github.com/$GH_REPO/releases/download/$TAG/note.md-x86_64.app.tar.gz"
PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
LATEST_JSON_STAGED="/tmp/latest.json"
python3 - "$VERSION" "$PUB_DATE" "$TAG" "$GH_REPO" \
    "$TARBALL_URL_AARCH64" "$SIG_CONTENT_AARCH64" \
    "$TARBALL_URL_X86_64"  "$SIG_CONTENT_X86_64" \
    > "$LATEST_JSON_STAGED" <<'PY'
import json, sys
(version, pub_date, tag, repo,
 url_aarch64, sig_aarch64,
 url_x86_64,  sig_x86_64) = sys.argv[1:9]
manifest = {
    "version": version,
    "notes": f"See https://github.com/{repo}/releases/tag/{tag}",
    "pub_date": pub_date,
    "platforms": {
        "darwin-aarch64": {"signature": sig_aarch64, "url": url_aarch64},
        "darwin-x86_64":  {"signature": sig_x86_64,  "url": url_x86_64},
    },
}
print(json.dumps(manifest, indent=2))
PY
echo "    manifest: $LATEST_JSON_STAGED"

gh -R "$GH_REPO" release create "$TAG" \
  --title "note.md $VERSION" \
  --notes "$NOTES" \
  "${EXTRA[@]}" \
  "${STAGED_ASSETS[@]}" \
  "$LATEST_JSON_STAGED"

URL=$(gh -R "$GH_REPO" release view "$TAG" --json url -q .url)
printf '\033[1;32m✓\033[0m released: %s\n' "$URL"
