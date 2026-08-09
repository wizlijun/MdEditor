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

.PARAMETER Upload
  Attach the staged files to the GitHub release for this version's tag.

  Uses the REST API with $env:GH_TOKEN (or $env:GITHUB_TOKEN) rather than the
  gh CLI: gh is not installed on a stock Windows box, and `gh auth login` is
  interactive, which rules it out for an unattended release. A token needs the
  `contents: write` scope on the repo.

  The release must already exist — this attaches to it, it does not create or
  publish one. Assets of the same name are replaced.

.EXAMPLE
  # Key already at ~/.tauri/mdeditor.key
  ./scripts/release-windows.ps1

.EXAMPLE
  # Merge into the manifest from the current release
  ./scripts/release-windows.ps1 -LatestJson C:\tmp\latest.json

.EXAMPLE
  # Build, then attach to the existing release for this version
  $env:GH_TOKEN = '...'
  ./scripts/release-windows.ps1 -Upload
#>
[CmdletBinding()]
param(
  [string] $Tag,
  [string] $LatestJson,
  [string] $OutDir,
  [switch] $SkipTests,
  [switch] $Upload
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

<#
.SYNOPSIS
  Run a command with a *present but empty* updater signing password.

.DESCRIPTION
  Tauri decides whether to prompt for the key password by whether
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD exists — not by whether it is empty:

    absent          → "Decrypting updater signing key, expect a prompt for
                      password", then blocks on stdin. In an unattended run that
                      hangs forever, *after* the installer is already built, with
                      `building` as the last line in the log. Nothing about it
                      reads as a password problem. This cost 2.5 hours once.
    present, empty  → decrypts with an empty password. What this project needs.
    present, wrong  → fails fast, "incorrect updater private key password".

  And on Windows there is no way to put an empty value in the *current*
  process's environment. All three of these DELETE the variable:

    $env:X = ''                                          (PowerShell)
    set X=                                               (cmd)
    [Environment]::SetEnvironmentVariable('X','','Process')

  The last one surprises people — Windows PowerShell 5.1 runs on .NET Framework,
  where that call forwards to Win32 SetEnvironmentVariableW, and passing an
  empty string there means "remove".

  A child's environment block, built by hand, *can* hold an empty value. So the
  build is launched through ProcessStartInfo rather than invoked inline. stdin
  is redirected and closed as a second line of defence: any prompt that does
  appear then reads EOF and fails fast instead of hanging.
#>
function Invoke-Build {
  param([Parameter(Mandatory)][string] $Arguments, [Parameter(Mandatory)][string] $What)

  $psi = New-Object System.Diagnostics.ProcessStartInfo
  # Via cmd.exe: `Get-Command pnpm` resolves to pnpm.ps1, which CreateProcess
  # cannot launch ("not a valid application for this OS platform").
  $psi.FileName = Join-Path $env:SystemRoot 'System32\cmd.exe'
  $psi.Arguments = "/c pnpm $Arguments"
  $psi.WorkingDirectory = $Root
  $psi.UseShellExecute = $false
  $psi.RedirectStandardInput = $true

  $psi.Environment['TAURI_SIGNING_PRIVATE_KEY'] = $env:TAURI_SIGNING_PRIVATE_KEY
  $psi.Environment['TAURI_SIGNING_PRIVATE_KEY_PASSWORD'] =
    if ($null -ne $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) { $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD } else { '' }
  $psi.Environment['RUSTUP_TOOLCHAIN'] = $env:RUSTUP_TOOLCHAIN

  $proc = [System.Diagnostics.Process]::Start($psi)
  $proc.StandardInput.Close()
  $proc.WaitForExit()
  if ($proc.ExitCode -ne 0) { Die "$What failed (exit $($proc.ExitCode))" }
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

# Tauri wants this variable base64-encoded, but a key file exists in the wild in
# BOTH forms: `tauri signer generate` writes the canonical minisign document
# ("untrusted comment: …\n<payload>") to disk, while CI setups and
# scripts/release.sh's `cat` path carry the base64 of that whole document.
# Handing the canonical form straight through fails deep inside the bundler with
# "failed to decode base64 key: Invalid symbol 32, offset 9" — offset 9 being
# the space in "untrusted comment:", which explains nothing to the reader.
# Normalize here so either file works.
$rawKey = $env:TAURI_SIGNING_PRIVATE_KEY.Trim()
if ($rawKey -like 'untrusted comment:*') {
  $env:TAURI_SIGNING_PRIVATE_KEY =
    [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($rawKey + "`n"))
  Say "key was in canonical minisign form — base64-encoded for the bundler"
} else {
  try {
    $decoded = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($rawKey))
  } catch {
    Die "updater key is neither a minisign document nor base64 — check $keyPath"
  }
  if ($decoded -notlike 'untrusted comment:*') {
    Die "updater key decodes to something that is not a minisign secret key — check $keyPath"
  }
}

# The signing password is NOT set here — it cannot be. See Invoke-Native.
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

# ---------- align with the tag ----------
#
# The Windows build must be of exactly the code the macOS side tagged. Building
# whatever happens to be checked out produces a package whose contents do not
# match the tag — and the signature still verifies, so nothing downstream can
# catch it. `-Tag` makes that impossible to get wrong by accident.

if ($Tag) {
  Invoke-Native { git fetch origin --tags --quiet } "git fetch --tags"

  $dirty = (git status --porcelain)
  if ($dirty) {
    Die @"
working tree is dirty — refusing to check out $Tag over local changes.
Commit or stash first:

$dirty
"@
  }

  Say "checking out $Tag"
  Invoke-Native { git -c advice.detachedHead=false checkout --quiet $Tag } "git checkout $Tag"
}

$version = (Get-Content (Join-Path $Root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
if (-not $version) { Die "could not read version from src-tauri/tauri.conf.json" }
$tag = if ($Tag) { $Tag } else { "v$version" }
if ($tag -ne "v$version") {
  Die "tag $tag does not match the checked-out version $version — the tag is authoritative, so this build would ship the wrong code"
}
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
Invoke-Build -Arguments 'tauri build' -What 'build'

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

# .NET does not read HTTP(S)_PROXY the way curl and reqwest do, so a machine
# that only reaches GitHub through a proxy needs it passed explicitly — to the
# download below as well as to the uploads further down.
$proxy = $env:HTTPS_PROXY
if (-not $proxy) { $proxy = $env:HTTP_PROXY }
$dlCommon = @{}
if ($proxy) { $dlCommon.Proxy = $proxy }

# latest.json is ONE manifest shared by every platform. The copy already on the
# release carries the darwin-* entries; a locally-built replacement would delete
# them and break auto-update for every macOS user the moment it is uploaded.
# So: fetch the live one, merge into it, and refuse to proceed if it cannot be
# fetched — failing here leaves the release untouched, which is the safe state.
if (-not $LatestJson -and $Upload) {
  $LatestJson = Join-Path $OutDir 'latest.upstream.json'
  Say "downloading the live latest.json from $tag"
  try {
    Invoke-WebRequest @dlCommon -Uri "https://github.com/$repo/releases/download/$tag/latest.json" `
      -OutFile $LatestJson -UseBasicParsing -TimeoutSec 60
  } catch {
    Die @"
could not download the live latest.json from ${tag}: $($_.Exception.Message)

Refusing to build one from scratch — it would carry only windows-x86_64 and
drop the darwin-* entries, breaking auto-update for every macOS user. Fix the
network/tag and retry, or pass -LatestJson with a copy fetched by hand.
"@
  }
}

if ($LatestJson) {
  if (-not (Test-Path $LatestJson)) { Die "latest.json not found: $LatestJson" }
  $manifest = Get-Content $LatestJson -Raw | ConvertFrom-Json
  if ($manifest.version -ne $version) {
    Die "version mismatch: latest.json says '$($manifest.version)', this build is '$version'"
  }
  $manifest.platforms | Add-Member -NotePropertyName 'windows-x86_64' -NotePropertyValue $winEntry -Force
  $names = ($manifest.platforms | Get-Member -MemberType NoteProperty).Name
  foreach ($required in 'darwin-aarch64', 'darwin-x86_64') {
    if ($names -notcontains $required) {
      Die "merged manifest is missing $required — uploading it would strand those users"
    }
  }
  Say "merged windows-x86_64; platforms now: $($names -join ', ')"
} else {
  Write-Host "! no -LatestJson and no -Upload: emitting a Windows-only manifest for inspection." -ForegroundColor Yellow
  Write-Host "  Do NOT upload this by hand — it has no darwin-* entries." -ForegroundColor Yellow
  $manifest = [pscustomobject]@{
    version   = $version
    notes     = "See https://github.com/$repo/releases/tag/$tag"
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [pscustomobject]@{ 'windows-x86_64' = $winEntry }
  }
}

$manifest | ConvertTo-Json -Depth 10 | Set-Content -Path $outFile -Encoding utf8
Ok "manifest: $outFile"

# ---------- upload ----------

# The staged set. `$updater` is the installer itself under Tauri 2, so it is
# deduplicated here rather than uploaded twice.
$assets = @($setup, $sigFile, $outFile) + @(if ($updater -ne $setup) { $updater })
$assets = $assets | Select-Object -Unique

if ($Upload) {
  $token = $env:GH_TOKEN
  if (-not $token) { $token = $env:GITHUB_TOKEN }
  if (-not $token) { Die "-Upload needs GH_TOKEN (or GITHUB_TOKEN) with 'contents: write' on $repo" }

  $common = @{ Headers = @{
      Authorization = "Bearer $token"
      'User-Agent'  = 'notemd-release-windows'
      Accept        = 'application/vnd.github+json'
    } }
  if ($proxy) {
    $common.Proxy = $proxy
    Say "uploading via proxy $proxy"
  }

  Say "resolving release $tag"
  try {
    $release = Invoke-RestMethod @common -Uri "https://api.github.com/repos/$repo/releases/tags/$tag"
  } catch {
    Die "no release found for $tag — create it first (the macOS assets normally land there via scripts/release.sh): $($_.Exception.Message)"
  }
  Ok "release: $($release.name) (draft=$($release.draft), prerelease=$($release.prerelease))"

  foreach ($path in $assets) {
    $name = [IO.Path]::GetFileName($path)

    # Same-name assets are replaced: re-running after a fix should converge,
    # not accumulate `foo.exe`, `foo-1.exe`.
    $existing = $release.assets | Where-Object { $_.name -eq $name }
    foreach ($old in $existing) {
      Say "replacing existing asset $name"
      Invoke-RestMethod @common -Method Delete -Uri $old.url | Out-Null
    }

    $uploadUri = ($release.upload_url -replace '\{\?name,label\}', '') + "?name=$name"
    Say "uploading $name ($([math]::Round((Get-Item $path).Length / 1MB, 2)) MB)"
    $headers = $common.Headers.Clone()
    $headers['Content-Type'] = 'application/octet-stream'
    # Not `$args` — that is an automatic variable in PowerShell.
    $uploadArgs = @{ Headers = $headers; Method = 'Post'; Uri = $uploadUri; InFile = $path }
    if ($proxy) { $uploadArgs.Proxy = $proxy }
    Invoke-RestMethod @uploadArgs | Out-Null
    Ok "uploaded $name"
  }

  # ---------- read back ----------
  #
  # Confirm against what the release actually serves, not against what we
  # believe we sent. A silently-truncated upload or a stale CDN copy looks
  # exactly like success from the POST alone, and the failure mode — macOS
  # clients losing their update channel — is invisible from here.
  Say "verifying the published latest.json"
  $publishedPath = Join-Path $OutDir 'latest.published.json'
  Invoke-WebRequest @dlCommon -Uri "https://github.com/$repo/releases/download/$tag/latest.json" `
    -OutFile $publishedPath -UseBasicParsing -TimeoutSec 60
  $published = Get-Content $publishedPath -Raw | ConvertFrom-Json
  $publishedNames = ($published.platforms | Get-Member -MemberType NoteProperty).Name
  foreach ($required in 'darwin-aarch64', 'darwin-x86_64', 'windows-x86_64') {
    if ($publishedNames -notcontains $required) {
      Die "published latest.json is missing $required (has: $($publishedNames -join ', ')) — the release is in a bad state, fix it before announcing"
    }
  }
  if ($published.platforms.'windows-x86_64'.signature -ne $signature) {
    Die "published windows-x86_64 signature does not match the one just built"
  }
  Ok "published platforms: $($publishedNames -join ', ')"

  Ok "https://github.com/$repo/releases/tag/$tag"
}

# ---------- summary ----------

Write-Host ""
Ok "staged for $tag"
foreach ($path in $assets) { Write-Host "  $path" }
if (-not $Upload) {
  Write-Host ""
  Write-Host "To attach these to the existing $tag release:" -ForegroundColor Cyan
  Write-Host '  $env:GH_TOKEN = "<token with contents: write>"'
  Write-Host "  ./scripts/release-windows.ps1 -Upload -SkipTests"
}
Write-Host ""
Write-Host "Reminder: this build carries NO Authenticode signature, so Windows" -ForegroundColor Yellow
Write-Host "SmartScreen will warn on download. minisign only satisfies the updater." -ForegroundColor Yellow
