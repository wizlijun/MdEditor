<#
.SYNOPSIS
  Build, minisign-sign and stage the Windows x64 release of note.md.

.DESCRIPTION
  The Windows half of scripts/release.sh, which is macOS-only (it builds two
  per-arch .dmg files and notarizes them through Apple's notarytool). This
  script covers what that one cannot: the NSIS installer, its updater artifact,
  the minisign signature over that artifact, and the `windows-x86_64` entry for
  latest.json.

  It deliberately does NOT create the GitHub release. latest.json is a single
  manifest shared by every platform, so the Windows entry has to be merged into
  the macOS one rather than replace it — pass -LatestJson to merge, then upload
  the merged file together with the macOS assets.

  Two signatures are involved and they are unrelated:

    minisign      — what this script applies. It is what the Tauri updater
                    verifies, using the public key pinned in tauri.conf.json.
                    Without it, auto-update refuses the download.
    Authenticode  — a paid certificate from a CA. NOT applied here. Until one
                    exists, SmartScreen will warn on every download; minisign
                    does nothing about that. See the release notes in
                    docs/2026-08-08-pc-port-refactor-plan.md §6.2 / D7.

.PARAMETER LatestJson
  An existing latest.json (e.g. downloaded from the current GitHub release).
  The windows-x86_64 entry is merged into a copy of it; the input is untouched.
  Omit to emit a Windows-only manifest.

.PARAMETER OutDir
  Where to stage latest.json. Defaults to a temp directory.

.PARAMETER SkipTests
  Skip the test suites. The release path runs them by default for the same
  reason release.sh does.

.EXAMPLE
  # Key already at ~/.tauri/mdeditor.key
  ./scripts/release-windows.ps1

.EXAMPLE
  # Merge into the manifest from the current release
  ./scripts/release-windows.ps1 -LatestJson C:\tmp\latest.json
#>
[CmdletBinding()]
param(
  [string] $LatestJson,
  [string] $OutDir,
  [switch] $SkipTests
)

$ErrorActionPreference = 'Stop'

function Say  { param([string]$m) Write-Host "> $m" -ForegroundColor Cyan }
function Die  { param([string]$m) Write-Host "x $m" -ForegroundColor Red; exit 1 }
function Ok   { param([string]$m) Write-Host "v $m" -ForegroundColor Green }

# Run a native executable, judging it by its exit code alone.
#
# `$ErrorActionPreference = 'Stop'` is what we want for cmdlets, but under
# Windows PowerShell 5.1 it also turns any line a native program writes to
# stderr into a terminating NativeCommandError. cargo and tauri both narrate
# progress on stderr, so a perfectly healthy build would abort at its first
# `Info …` line. Relax the preference around the call and check $LASTEXITCODE,
# which is the only reliable signal here.
function Invoke-Native {
  param([Parameter(Mandatory)][scriptblock] $Cmd, [Parameter(Mandatory)][string] $What)
  $prev = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try { & $Cmd } finally { $ErrorActionPreference = $prev }
  if ($LASTEXITCODE -ne 0) { Die "$What failed (exit $LASTEXITCODE)" }
}

$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

# ---------- secrets (same convention as release.sh: .env.release, git-ignored) ----------

$envFile = Join-Path $Root '.env.release'
if (Test-Path $envFile) {
  Say "loading .env.release"
  foreach ($line in Get-Content $envFile) {
    $t = $line.Trim()
    if ($t -eq '' -or $t.StartsWith('#')) { continue }
    $i = $t.IndexOf('=')
    if ($i -lt 1) { continue }
    $k = $t.Substring(0, $i).Trim()
    $v = $t.Substring($i + 1).Trim().Trim('"').Trim("'")
    Set-Item -Path "env:$k" -Value $v
  }
}

# ---------- updater signing key ----------
#
# This MUST be the key whose public half is pinned in tauri.conf.json
# (plugins.updater.pubkey). Generating a fresh keypair does not "fix" a missing
# key — it silently orphans every install already out there, because the app
# verifies against the pinned public key and will reject the new signature.

if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
  $keyPath = $env:TAURI_SIGNING_PRIVATE_KEY_PATH
  if (-not $keyPath) { $keyPath = Join-Path $HOME '.tauri\mdeditor.key' }
  if (-not (Test-Path $keyPath)) {
    Die @"
updater private key not found at $keyPath

Put the EXISTING key there (or set TAURI_SIGNING_PRIVATE_KEY / _PATH, or add it
to .env.release). It is the same key scripts/release.sh uses on macOS.

Do NOT run ``pnpm tauri signer generate`` to get past this: the public key in
src-tauri/tauri.conf.json is pinned to the current keypair, and a new one would
break auto-update for every existing install.
"@
  }
  $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $keyPath -Raw).Trim()
}
if (-not $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
}
Ok "updater signing key loaded"

# ---------- toolchain ----------
#
# src-tauri/rust-toolchain.toml lists four Apple targets. Left to itself, rustup
# tries to download all of them before cargo runs — minutes of pointless traffic
# on a Windows box, and an interrupted sync can leave the toolchain unusable.
# Pinning RUSTUP_TOOLCHAIN bypasses the file entirely.

if (-not $env:RUSTUP_TOOLCHAIN) {
  $hostTriple = (rustc -vV | Select-String '^host:').ToString().Split(' ')[1]
  $env:RUSTUP_TOOLCHAIN = "stable-$hostTriple"
}
Say "toolchain: $env:RUSTUP_TOOLCHAIN"

$version = (Get-Content (Join-Path $Root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
if (-not $version) { Die "could not read version from src-tauri/tauri.conf.json" }
$tag = "v$version"
$repo = $env:GH_REPO
if (-not $repo) { $repo = 'wizlijun/note.md' }
Say "version $version  tag $tag  repo $repo"

# ---------- tests ----------

if (-not $SkipTests) {
  Say "frontend tests"
  Invoke-Native { pnpm test } "frontend tests"

  Say "rust tests"
  Push-Location (Join-Path $Root 'src-tauri')
  try { Invoke-Native { cargo test } "rust tests" } finally { Pop-Location }
  Ok "tests pass"
}

# ---------- build ----------

Say "building (release, nsis)"
Invoke-Native { pnpm tauri build } "build"

$exe = Join-Path $Root 'src-tauri\target\release\notemd.exe'
$bundleDir = Join-Path $Root 'src-tauri\target\release\bundle\nsis'
$setup = Join-Path $bundleDir "note.md_${version}_x64-setup.exe"
# Tauri 2 signs the NSIS installer itself and drops `<setup>.exe.sig` beside it.
# (Tauri 1 produced a separate `.nsis.zip` to sign; there is no such file here.)
# So the installer doubles as the updater artifact — latest.json points at it.
$updater = $setup
$sigFile = "$setup.sig"

foreach ($f in @($exe, $setup, $sigFile)) {
  if (-not (Test-Path $f)) { Die "expected artifact missing: $f" }
}

# ---------- architecture assertion ----------
#
# The counterpart of the `lipo` check release.sh runs on the macOS side: read the
# PE header's Machine field and refuse to ship anything that is not AMD64.
# (The NSIS *installer* stub is legitimately i386 regardless of payload, so this
# is asserted on notemd.exe, not on setup.exe.)

$fs = [IO.File]::OpenRead($exe)
try {
  $br = New-Object IO.BinaryReader($fs)
  $fs.Position = 0x3C
  $peOffset = $br.ReadInt32()
  $fs.Position = $peOffset
  $sig = $br.ReadInt32()
  $machine = $br.ReadUInt16()
} finally {
  $fs.Close()
}
if ($sig -ne 0x00004550) { Die "notemd.exe is not a PE image" }
if ($machine -ne 0x8664) { Die ("notemd.exe is not x64 (PE Machine=0x{0:X4})" -f $machine) }
Ok ("architecture verified: PE Machine=0x{0:X4} (AMD64)" -f $machine)

$signature = (Get-Content $sigFile -Raw).Trim()
if (-not $signature) { Die "signature file is empty: $sigFile" }

# ---------- the signature must match the pinned public key ----------
#
# Tauri only *warns* when TAURI_SIGNING_PRIVATE_KEY does not match
# `plugins.updater.pubkey` ("...won't be accepted at runtime when performing
# update"). A warning is the wrong severity for this: the build succeeds, the
# release looks fine, and every user's auto-update silently fails afterwards.
# Compare the minisign key IDs and refuse to stage on a mismatch.
#
# Both files are base64 of a two-line minisign document; in each, the payload
# line decodes to [2-byte algorithm][8-byte key id][key or signature].
function Get-MinisignKeyId {
  param([Parameter(Mandatory)][string] $Base64Document, [Parameter(Mandatory)][string] $What)
  try {
    $text = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($Base64Document.Trim()))
  } catch {
    Die "$What is not valid base64"
  }
  $payload = @($text -split "`r?`n" | Where-Object { $_.Trim() -ne '' -and $_ -notmatch '^(un)?trusted comment:' })
  if ($payload.Count -lt 1) { Die "$What has no payload line" }
  $bytes = [Convert]::FromBase64String($payload[0].Trim())
  if ($bytes.Length -lt 10) { Die "$What payload is too short to carry a key id" }
  return [BitConverter]::ToString($bytes[2..9])
}

$confRaw = Get-Content (Join-Path $Root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
$pubkey = $confRaw.plugins.updater.pubkey
if (-not $pubkey) { Die "plugins.updater.pubkey missing from tauri.conf.json" }

$pubKeyId = Get-MinisignKeyId -Base64Document $pubkey -What "tauri.conf.json pubkey"
$sigKeyId = Get-MinisignKeyId -Base64Document $signature -What "signature file"
if ($pubKeyId -ne $sigKeyId) {
  Die @"
signature was made with the WRONG key — refusing to stage.

  pubkey in tauri.conf.json : $pubKeyId
  key that signed the build : $sigKeyId

Every existing install verifies against the pinned public key, so publishing
this would break auto-update for all of them. Point TAURI_SIGNING_PRIVATE_KEY
at the real key (the one scripts/release.sh uses on macOS) and build again.
"@
}
Ok "signature verified against the pinned public key (key id $pubKeyId)"
Ok "updater artifact signed ($([IO.Path]::GetFileName($updater)))"

# ---------- latest.json ----------

if (-not $OutDir) {
  $OutDir = Join-Path $env:TEMP "notemd-release-$version"
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$outFile = Join-Path $OutDir 'latest.json'

$url = "https://github.com/$repo/releases/download/$tag/" + [IO.Path]::GetFileName($updater)
$winEntry = [pscustomobject]@{ signature = $signature; url = $url }

if ($LatestJson) {
  if (-not (Test-Path $LatestJson)) { Die "latest.json not found: $LatestJson" }
  $manifest = Get-Content $LatestJson -Raw | ConvertFrom-Json
  if ($manifest.version -ne $version) {
    Die "version mismatch: latest.json says '$($manifest.version)', this build is '$version'"
  }
  $manifest.platforms | Add-Member -NotePropertyName 'windows-x86_64' -NotePropertyValue $winEntry -Force
  Say "merged windows-x86_64 into $LatestJson"
} else {
  Write-Host "! no -LatestJson given: emitting a Windows-only manifest." -ForegroundColor Yellow
  Write-Host "  Uploading this as-is would drop the darwin-* entries and strand macOS users." -ForegroundColor Yellow
  $manifest = [pscustomobject]@{
    version   = $version
    notes     = "See https://github.com/$repo/releases/tag/$tag"
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [pscustomobject]@{ 'windows-x86_64' = $winEntry }
  }
}

$manifest | ConvertTo-Json -Depth 10 | Set-Content -Path $outFile -Encoding utf8
Ok "manifest: $outFile"

# ---------- summary ----------

Write-Host ""
Ok "staged for $tag"
Write-Host "  installer : $setup"
Write-Host "  updater   : $updater"
Write-Host "  signature : $sigFile"
Write-Host "  manifest  : $outFile"
Write-Host ""
Write-Host "Next (needs the gh CLI, and the macOS assets already attached to $tag):" -ForegroundColor Cyan
Write-Host "  gh -R $repo release upload $tag ```"$setup```" ```"$updater```" ```"$sigFile```" ```"$outFile```" --clobber"
Write-Host ""
Write-Host "Reminder: this build carries NO Authenticode signature, so Windows" -ForegroundColor Yellow
Write-Host "SmartScreen will warn on download. minisign only satisfies the updater." -ForegroundColor Yellow
