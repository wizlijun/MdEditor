<#
.SYNOPSIS
  把 Windows 安装包补进一个【已经由 macOS 侧发布好】的 GitHub Release。

.DESCRIPTION
  note.md 的发版是分两台机器、两个阶段完成的:

    阶段 1(macOS,scripts/release.sh)
      bump 版本 → 打 tag → 建 GitHub Release
      → 上传 2 个 dmg + 2 个 updater tarball + latest.json(只含 darwin-*)

    阶段 2(Windows,本脚本)                       ← 你在这里
      checkout 同一个 tag → 构建 → 上传 setup.exe + updater 包
      → 把 windows-* 条目并进那份同一个 latest.json

  本脚本【不】创建 Release、【不】打 tag、【不】改版本号。版本号完全由 macOS
  侧决定,这里只认它。这样两台机器不会各自推导出不同的版本。

.PARAMETER Tag
  要补包的 tag,如 v6.808.3。省略则取 origin 上最新的 v* tag。

.EXAMPLE
  pwsh scripts/release-windows.ps1 -Tag v6.808.3
#>
[CmdletBinding()]
param(
  [string]$Tag,
  [string]$Repo = 'wizlijun/note.md'
)

$ErrorActionPreference = 'Stop'
function Say($m) { Write-Host "▸ $m" -ForegroundColor Cyan }
function Die($m) { Write-Host "✗ $m" -ForegroundColor Red; exit 1 }

# ── 0. 前置检查 ───────────────────────────────────────────────────────────────
foreach ($cmd in @('git', 'gh', 'node', 'pnpm', 'cargo')) {
  if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) { Die "$cmd 不在 PATH 上" }
}
gh auth status *> $null
if ($LASTEXITCODE -ne 0) { Die 'gh 未登录,先跑 gh auth login' }

# ── 1. 签名密钥:必须“存在且为空”,不是“不存在” ───────────────────────────────
#
# Tauri 解密 updater 私钥时判的是【环境变量存不存在】,不是值是不是空:
#   不存在        → 打印 "expect a prompt for password" 后阻塞等 stdin,
#                   非交互会话里永远挂着(这坑吃掉过 2.5 小时)
#   存在且为空串  → 用空密码解密,通过  ← 我们要的
#   存在但值错误  → 立刻报 incorrect updater private key password(会退出,不会挂)
#
# 而 PowerShell 的 `$env:X = ""` 和 cmd 的 `set X=` 语义都是【删除变量】,
# 所以必须走 .NET API 才能真正设成空串。
$keyPath = Join-Path $env:USERPROFILE '.tauri\mdeditor.key'
if (-not (Test-Path $keyPath)) { Die "找不到更新器私钥:$keyPath(从 Mac 上拷过来,别提交进 git)" }

[Environment]::SetEnvironmentVariable('TAURI_SIGNING_PRIVATE_KEY', (Get-Content $keyPath -Raw).Trim(), 'Process')
[Environment]::SetEnvironmentVariable('TAURI_SIGNING_PRIVATE_KEY_PASSWORD', '', 'Process')

if ($null -eq [Environment]::GetEnvironmentVariable('TAURI_SIGNING_PRIVATE_KEY_PASSWORD', 'Process')) {
  Die 'PASSWORD 变量仍是 UNSET —— 构建一定会挂死在密码提示上,先修这里'
}
Say 'signing env ok(PASSWORD = 已设置且为空)'

# ── 2. 对齐 tag ───────────────────────────────────────────────────────────────
git fetch origin --tags --quiet
if (-not $Tag) {
  $Tag = (git tag --list 'v*' --sort=-v:refname | Select-Object -First 1)
  Say "未指定 tag,取最新:$Tag"
}
$Version = $Tag.TrimStart('v')

$dirty = git status --porcelain
if ($dirty) { Die "工作区不干净,先提交或清理:`n$dirty" }

git checkout --quiet $Tag
$pkgVersion = (Get-Content package.json -Raw | ConvertFrom-Json).version
if ($pkgVersion -ne $Version) { Die "package.json 是 $pkgVersion,tag 是 $Version —— 对不上" }

gh -R $Repo release view $Tag *> $null
if ($LASTEXITCODE -ne 0) { Die "Release $Tag 不存在。必须先在 Mac 上发版,Windows 只做补包。" }
Say "对齐到 $Tag(版本 $Version)"

# ── 3. 架构 ───────────────────────────────────────────────────────────────────
switch ($env:PROCESSOR_ARCHITECTURE) {
  'AMD64' { $Triple = 'x86_64-pc-windows-msvc';  $PlatformKey = 'windows-x86_64' }
  'ARM64' { $Triple = 'aarch64-pc-windows-msvc'; $PlatformKey = 'windows-aarch64' }
  default { Die "未知架构:$env:PROCESSOR_ARCHITECTURE" }
}
rustup target add $Triple | Out-Null
Say "target = $Triple / updater key = $PlatformKey"

# ── 4. 构建 ───────────────────────────────────────────────────────────────────
Say '构建中(首次约 10-20 分钟)…'
pnpm install --frozen-lockfile
pnpm tauri build --target $Triple
if ($LASTEXITCODE -ne 0) { Die 'tauri build 失败' }

$bundleDir = "src-tauri/target/$Triple/release/bundle/nsis"
$setup = Get-ChildItem "$bundleDir/*-setup.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
$zip   = Get-ChildItem "$bundleDir/*-setup.nsis.zip" -ErrorAction SilentlyContinue | Select-Object -First 1
$sig   = Get-ChildItem "$bundleDir/*-setup.nsis.zip.sig" -ErrorAction SilentlyContinue | Select-Object -First 1

if (-not $setup) { Die "没找到 setup.exe —— bundle.targets 是否含 nsis?(见 src-tauri/tauri.windows.conf.json)" }
if (-not $zip)   { Die '没找到 .nsis.zip —— createUpdaterArtifacts 没生效' }
if (-not $sig)   { Die '没找到 .sig —— 签名没跑,回头看 §1 的 PASSWORD 变量' }
if ((Get-Item $sig).Length -eq 0) { Die '.sig 是空文件 —— 签名没真正完成' }

Say "产物:$($setup.Name) ($([math]::Round($setup.Length/1MB,1)) MB)"

# ── 5. 上传到【同一个】 Release ───────────────────────────────────────────────
# --clobber:允许重跑覆盖,避免失败一次就得手工去网页删附件。
Say '上传安装包与 updater 产物…'
gh -R $Repo release upload $Tag $setup.FullName $zip.FullName $sig.FullName --clobber
if ($LASTEXITCODE -ne 0) { Die '上传失败' }

# ── 6. 把 windows-* 并进那份 latest.json ──────────────────────────────────────
# 关键:必须【下载线上那份】再合并,不能本地新造一份 —— 本地造的会丢掉
# macOS 的 darwin-* 条目,等于把 mac 用户的自动更新掐断。
# 合并逻辑与校验在 scripts/merge-latest-json-core.mjs(有单测)。
$work = New-Item -ItemType Directory -Force -Path (Join-Path $env:TEMP "notemd-$Version")
$latest = Join-Path $work 'latest.json'
Remove-Item $latest -ErrorAction SilentlyContinue
gh -R $Repo release download $Tag --pattern latest.json --dir $work
if (-not (Test-Path $latest)) { Die '线上没有 latest.json —— mac 侧发版可能没成功' }

$zipUrl = "https://github.com/$Repo/releases/download/$Tag/$($zip.Name)"
node scripts/merge-latest-json.mjs --file $latest --platform $PlatformKey --url $zipUrl --sig-file $sig.FullName --version $Version
if ($LASTEXITCODE -ne 0) { Die '合并 latest.json 失败(上面有原因),尚未上传,线上仍是旧的' }

gh -R $Repo release upload $Tag $latest --clobber
if ($LASTEXITCODE -ne 0) { Die 'latest.json 上传失败' }

# ── 7. 回读校验:不信脚本输出,只信线上真实内容 ───────────────────────────────
Say '回读线上 latest.json 校验…'
$check = Join-Path $work 'verify'
Remove-Item $check -Recurse -ErrorAction SilentlyContinue
gh -R $Repo release download $Tag --pattern latest.json --dir $check
$live = Get-Content (Join-Path $check 'latest.json') -Raw | ConvertFrom-Json
$keys = $live.platforms.PSObject.Properties.Name | Sort-Object

if ($live.version -ne $Version) { Die "线上 latest.json 版本是 $($live.version),不是 $Version" }
foreach ($need in @('darwin-aarch64', 'darwin-x86_64', $PlatformKey)) {
  if ($keys -notcontains $need) { Die "线上 latest.json 缺 $need —— 平台条目被弄丢了!" }
}

Write-Host ''
Write-Host "✓ $Tag 已补齐 Windows" -ForegroundColor Green
Write-Host "  平台条目:$($keys -join ', ')"
Write-Host "  安装包:https://github.com/$Repo/releases/tag/$Tag"
