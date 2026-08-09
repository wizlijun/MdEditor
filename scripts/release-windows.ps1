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

# 本脚本会 `git checkout <tag>`,也就是把工作区切到 detached HEAD。任何一步失败
# 都必须切回来 —— 否则一次失败的补包会把发版机静悄悄地留在旧 tag 上,下一个来
# 操作的人(或下一次跑这个脚本)面对的是错误的代码,而表面上什么都正常。
#
# 恢复动作挂在 Die 里而不是只挂 finally:Die 走的是 `exit`,依赖 finally 在
# `exit` 时一定执行是个不必要的赌注,直接在退出前恢复更确定。
$script:OriginalRef = $null

function Restore-Ref {
  if (-not $script:OriginalRef) { return }
  $current = (git rev-parse --abbrev-ref HEAD 2>$null)
  if ($current -eq $script:OriginalRef) { return }
  Write-Host "▸ 切回 $script:OriginalRef" -ForegroundColor Cyan
  # 这里不能用 Die:它会调回自己。失败只警告,并把手工恢复的命令给出来。
  git checkout --quiet $script:OriginalRef 2>&1 | Out-Null
  if ($LASTEXITCODE -ne 0) {
    Write-Host "! 切回 $script:OriginalRef 失败,仓库仍在 detached HEAD。手工执行:git checkout $script:OriginalRef" -ForegroundColor Yellow
  }
}

function Die($m) {
  Write-Host "✗ $m" -ForegroundColor Red
  Restore-Ref
  exit 1
}

# 原生命令统一经此调用。PS 5.1 在 $ErrorActionPreference='Stop' 下,会把原生程序
# 写到 stderr 的【任意一行】变成终止性错误,而 gh / git / cargo 都在 stderr 上叙述
# 进度或提示 —— 未登录时 `gh auth status` 的那句提示就会让脚本死在那一行,而不是
# 走到我们自己那句友好的 Die。只按退出码判定。
function Invoke-Cli {
  param([Parameter(Mandatory)][scriptblock] $Cmd)
  $prev = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try { & $Cmd } finally { $ErrorActionPreference = $prev }
}

# ── 0. 前置检查 ───────────────────────────────────────────────────────────────
foreach ($cmd in @('git', 'gh', 'node', 'pnpm', 'cargo')) {
  if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) { Die "$cmd 不在 PATH 上" }
}
Invoke-Cli { gh auth status *> $null }
if ($LASTEXITCODE -ne 0) { Die 'gh 未登录,先跑 gh auth login' }

# ── 1. 签名密钥:必须“存在且为空”,不是“不存在” ───────────────────────────────
#
# Tauri 解密 updater 私钥时判的是【环境变量存不存在】,不是值是不是空:
#   不存在        → 打印 "expect a prompt for password" 后阻塞等 stdin,
#                   非交互会话里永远挂着(这坑吃掉过 2.5 小时)
#   存在且为空串  → 用空密码解密,通过  ← 我们要的
#   存在但值错误  → 立刻报 incorrect updater private key password(会退出,不会挂)
#
# 而在 Windows 上,给【当前进程】设空环境变量是做不到的 —— 下面三种写法全是删除:
#   $env:X = ''                                     (PowerShell)
#   set X=                                          (cmd)
#   [Environment]::SetEnvironmentVariable('X','','Process')
# 最后一条最反直觉:Windows PowerShell 5.1 跑在 .NET Framework 上,该调用转发到
# Win32 SetEnvironmentVariableW,传空串的语义就是「删除」。实测自检会打印 UNSET。
# 唯一可行的是【手工构造子进程的环境块】,见下面的 Invoke-Build。
$keyPath = Join-Path $env:USERPROFILE '.tauri\mdeditor.key'
if (-not (Test-Path $keyPath)) { Die "找不到更新器私钥:$keyPath(从 Mac 上拷过来,别提交进 git)" }

# Tauri 只认 TAURI_SIGNING_PRIVATE_KEY,且值必须是【整个密钥文件的 base64】。
# 密钥文件在外面有两种形态:`tauri signer generate` 写到盘上的是规范 minisign
# 文档(以 "untrusted comment:" 开头),而 CI/release.sh 的 `cat` 路径传的是那份
# 文档的 base64。把规范形态直接传进去会得到
#   failed to decode base64 key: Invalid symbol 32, offset 9
# offset 9 正是 "untrusted comment:" 里的那个空格 —— 这条报错解释不了任何事。
# 两种形态都认。
$SigningKey = (Get-Content $keyPath -Raw).Trim()
if ($SigningKey -like 'untrusted comment:*') {
  $SigningKey = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($SigningKey + "`n"))
  Say '私钥是规范 minisign 形态,已转成 base64 交给打包器'
}

# 自检:确认子进程真能看到一个「存在且为空」的变量。这一步失败就别往下走了,
# 否则会挂死在密码提示上(而且是在安装包已经产出之后,日志最后一行是 building,
# 完全不像密码问题 —— 这坑吃掉过 2.5 小时)。
$probe = New-Object System.Diagnostics.ProcessStartInfo
$probe.FileName = Join-Path $env:SystemRoot 'System32\cmd.exe'
$probe.Arguments = '/c set TAURI_SIGNING_PRIVATE_KEY_PASSWORD'
$probe.UseShellExecute = $false
$probe.RedirectStandardOutput = $true
$probe.Environment['TAURI_SIGNING_PRIVATE_KEY_PASSWORD'] = ''
$probeProc = [System.Diagnostics.Process]::Start($probe)
$probeOut = $probeProc.StandardOutput.ReadToEnd()
$probeProc.WaitForExit()
if ($probeProc.ExitCode -ne 0 -or $probeOut.Trim() -ne 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD=') {
  Die "子进程看不到空的 PASSWORD 变量(读到 '$($probeOut.Trim())')—— 构建会挂死在密码提示上"
}
Say 'signing env ok(子进程可见 PASSWORD = 空)'

# 用手工环境块跑命令。stdin 重定向后立即关闭:万一还有别的提示,子进程读到 EOF
# 快速失败,而不是把无人值守的发版挂住。
#
# 输出既实时打到控制台、也留一份在 $script:LastBuildLog,供构建后断言用
# —— 有些致命问题 Tauri 只 warn 不失败,只看退出码是看不出来的。
function Invoke-Build {
  param([Parameter(Mandatory)][string] $CommandLine, [Parameter(Mandatory)][string] $What)
  $script:LastBuildLog = Join-Path $env:TEMP "notemd-build-$(Get-Random).log"
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  # 经 cmd.exe:`Get-Command pnpm` 解析到 pnpm.ps1,CreateProcess 起不了它
  # ("not a valid application for this OS platform")。
  $psi.FileName = Join-Path $env:SystemRoot 'System32\cmd.exe'
  # 合并 stderr 到 stdout 再落盘:cargo / tauri 把进度叙述写在 stderr 上,
  # 要断言的那条 warning 也在那儿。
  $psi.Arguments = "/c $CommandLine > `"$script:LastBuildLog`" 2>&1"
  $psi.WorkingDirectory = (Get-Location).Path
  $psi.UseShellExecute = $false
  $psi.RedirectStandardInput = $true
  $psi.Environment['TAURI_SIGNING_PRIVATE_KEY'] = $SigningKey
  $psi.Environment['TAURI_SIGNING_PRIVATE_KEY_PASSWORD'] = ''
  $proc = [System.Diagnostics.Process]::Start($psi)
  $proc.StandardInput.Close()
  # 边跑边把日志尾巴打出来:构建要几分钟,完全静默会让人分不清"在编"和"挂住了"
  # —— 而挂住恰恰是这个脚本历史上最贵的失败模式。
  $shown = 0
  while (-not $proc.HasExited) {
    Start-Sleep -Seconds 5
    if (Test-Path $script:LastBuildLog) {
      $lines = @(Get-Content $script:LastBuildLog -ErrorAction SilentlyContinue)
      if ($lines.Count -gt $shown) {
        $lines[$shown..($lines.Count - 1)] | ForEach-Object { Write-Host "  $_" -ForegroundColor DarkGray }
        $shown = $lines.Count
      }
    }
  }
  if (Test-Path $script:LastBuildLog) {
    $lines = @(Get-Content $script:LastBuildLog -ErrorAction SilentlyContinue)
    if ($lines.Count -gt $shown) {
      $lines[$shown..($lines.Count - 1)] | ForEach-Object { Write-Host "  $_" -ForegroundColor DarkGray }
    }
  }
  if ($proc.ExitCode -ne 0) { Die "$What 失败(exit $($proc.ExitCode)),完整日志:$script:LastBuildLog" }
}

# ── 2. 对齐 tag ───────────────────────────────────────────────────────────────
Invoke-Cli { git fetch origin --tags --quiet }
if (-not $Tag) {
  $Tag = Invoke-Cli { git tag --list 'v*' --sort=-v:refname } | Select-Object -First 1
  Say "未指定 tag,取最新:$Tag"
}
$Version = $Tag.TrimStart('v')

$dirty = Invoke-Cli { git status --porcelain }
if ($dirty) { Die "工作区不干净,先提交或清理:`n$dirty" }

# 记下当前分支,好在结束(无论成败)时切回来。detached HEAD 上启动的话这里得到
# "HEAD",没有可回去的分支名,就不做恢复 —— 但要说明白,免得以为脚本会兜底。
$script:OriginalRef = Invoke-Cli { git rev-parse --abbrev-ref HEAD }
if ($script:OriginalRef -eq 'HEAD') {
  Write-Host "! 启动时已是 detached HEAD,结束后不会自动切回任何分支" -ForegroundColor Yellow
  $script:OriginalRef = $null
}

Invoke-Cli { git checkout --quiet $Tag }
$pkgVersion = (Get-Content package.json -Raw | ConvertFrom-Json).version
if ($pkgVersion -ne $Version) { Die "package.json 是 $pkgVersion,tag 是 $Version —— 对不上" }

Invoke-Cli { gh -R $Repo release view $Tag *> $null }
if ($LASTEXITCODE -ne 0) { Die "Release $Tag 不存在。必须先在 Mac 上发版,Windows 只做补包。" }
Say "对齐到 $Tag(版本 $Version)"

# ── 3. 架构 ───────────────────────────────────────────────────────────────────
switch ($env:PROCESSOR_ARCHITECTURE) {
  'AMD64' { $Triple = 'x86_64-pc-windows-msvc';  $PlatformKey = 'windows-x86_64' }
  'ARM64' { $Triple = 'aarch64-pc-windows-msvc'; $PlatformKey = 'windows-aarch64' }
  default { Die "未知架构:$env:PROCESSOR_ARCHITECTURE" }
}
Invoke-Cli { rustup target add $Triple } | Out-Null
Say "target = $Triple / updater key = $PlatformKey"

# ── 4. 构建 ───────────────────────────────────────────────────────────────────
# 构建前清场,两件事都吃过亏:
#
# ① 残留的 notemd.exe 会占住产物文件,打包阶段报
#    `failed to bundle project 另一个程序正在使用此文件 (os error 32)`。
#    上一轮冒烟测试没退干净就会这样。
$stale = Get-Process notemd -ErrorAction SilentlyContinue
if ($stale) {
  Say "清掉残留的 notemd 进程($($stale.Count) 个),否则打包会被文件占用挡住"
  $stale | Stop-Process -Force
  Start-Sleep -Milliseconds 800
}

# ② 更隐蔽:同一份代码重复打包时 cargo 不会重新链接,于是 Tauri 拿到的是
#    【上一轮已经被 patch 过】的二进制,找不到待写入的 __TAURI_BUNDLE_TYPE 标记位,
#    只 warn 不失败 —— 产出一个更新器认不出的包,而构建报"成功"。
#    删掉最终产物即可逼 cargo 重链(只重链,不重编,几十秒),比 cargo clean 便宜得多。
$builtExe = "src-tauri/target/$Triple/release/notemd.exe"
if (Test-Path $builtExe) {
  Remove-Item $builtExe -Force
  Say '删掉上一轮的 notemd.exe,强制重新链接(避免 __TAURI_BUNDLE_TYPE 标记丢失)'
}
Remove-Item "src-tauri/target/$Triple/release/bundle" -Recurse -Force -ErrorAction SilentlyContinue

Say '构建中(首次约 10-20 分钟)…'
Invoke-Build -CommandLine 'pnpm install --frozen-lockfile' -What 'pnpm install'
Invoke-Build -CommandLine "pnpm tauri build --target $Triple" -What 'tauri build'

# 断言而不是只靠预防:这条是 warning,退出码照样是 0,漏了就发出一个
# 「Updater plugin may not be able to update this package」的包。
if (Select-String -Path $script:LastBuildLog -Pattern '__TAURI_BUNDLE_TYPE variable not found' -Quiet) {
  Die @"
构建产出的二进制缺 __TAURI_BUNDLE_TYPE 标记 —— 更新器将认不出这个包。
成因通常是 cargo 复用了上一轮已被 patch 过的 notemd.exe。
清掉后重跑:cargo clean -p notemd --release --target $Triple
完整日志:$script:LastBuildLog
"@
}

# Tauri 2 直接对 NSIS 安装包签名,产出 `<setup>.exe.sig`;并没有 Tauri 1 那个
# 单独的 `.nsis.zip`。所以安装包本身就是 updater 产物,latest.json 指向它。
$bundleDir = "src-tauri/target/$Triple/release/bundle/nsis"
$setup = Get-ChildItem "$bundleDir/*-setup.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $setup) { Die "没找到 setup.exe —— bundle.targets 是否含 nsis?(见 src-tauri/tauri.windows.conf.json)" }
$sig = Get-ChildItem "$($setup.FullName).sig" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $sig)   { Die '没找到 .sig —— 签名没跑,回头看上面 §1 的 PASSWORD 自检' }
if ((Get-Item $sig).Length -eq 0) { Die '.sig 是空文件 —— 签名没真正完成' }

# 签名必须出自 tauri.conf.json 里钉住的那把钥匙。Tauri 对不匹配【只 warn】,
# 而后果是所有用户的自动更新静默失效 —— 严重性不对,这里升级成硬错。
# 公钥与签名都是两行 minisign 文档的 base64,payload 行解出来的第 2..9 字节是 key id。
function Get-MinisignKeyId([string] $Base64Doc, [string] $What) {
  try { $text = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($Base64Doc.Trim())) }
  catch { Die "$What 不是合法 base64" }
  $payload = @($text -split "`r?`n" | Where-Object { $_.Trim() -ne '' -and $_ -notmatch '^(un)?trusted comment:' })
  if ($payload.Count -lt 1) { Die "$What 没有 payload 行" }
  $bytes = [Convert]::FromBase64String($payload[0].Trim())
  if ($bytes.Length -lt 10) { Die "$What 的 payload 太短,放不下 key id" }
  return [BitConverter]::ToString($bytes[2..9])
}
$pinned = (Get-Content 'src-tauri/tauri.conf.json' -Raw | ConvertFrom-Json).plugins.updater.pubkey
$pinnedId = Get-MinisignKeyId $pinned 'tauri.conf.json 的 pubkey'
$sigId = Get-MinisignKeyId (Get-Content $sig.FullName -Raw) '签名文件'
if ($pinnedId -ne $sigId) {
  Die "签名用错了钥匙 —— 拒绝上传`n  钉住的公钥:$pinnedId`n  实际签名的:$sigId`n所有已安装用户都按钉住的公钥验签,传上去等于废掉他们的自动更新。"
}
Say "签名校验通过(key id $pinnedId)"

# 架构断言:对应 macOS 侧的 lipo 断言。断在 notemd.exe 上而非 setup.exe ——
# NSIS 的安装器外壳无论载荷是什么都是 i386,断在它上面恒假。
$exePath = "src-tauri/target/$Triple/release/notemd.exe"
$fs = [IO.File]::OpenRead($exePath)
try {
  $br = New-Object IO.BinaryReader($fs)
  $fs.Position = 0x3C; $peOff = $br.ReadInt32(); $fs.Position = $peOff
  $peSig = $br.ReadInt32(); $machine = $br.ReadUInt16()
} finally { $fs.Close() }
$wantMachine = if ($Triple -like 'aarch64*') { 0xAA64 } else { 0x8664 }
if ($peSig -ne 0x00004550) { Die "notemd.exe 不是 PE 文件" }
if ($machine -ne $wantMachine) { Die ("notemd.exe 架构不对:PE Machine=0x{0:X4},期望 0x{1:X4}" -f $machine, $wantMachine) }
Say ("架构校验通过:PE Machine=0x{0:X4}" -f $machine)

Say "产物:$($setup.Name) ($([math]::Round($setup.Length/1MB,1)) MB)"

# ── 5. 上传到【同一个】 Release ───────────────────────────────────────────────
# --clobber:允许重跑覆盖,避免失败一次就得手工去网页删附件。
Say '上传安装包与 updater 产物…'
Invoke-Cli { gh -R $Repo release upload $Tag $setup.FullName $sig.FullName --clobber }
if ($LASTEXITCODE -ne 0) { Die '上传失败' }

# ── 6. 把 windows-* 并进那份 latest.json ──────────────────────────────────────
# 关键:必须【下载线上那份】再合并,不能本地新造一份 —— 本地造的会丢掉
# macOS 的 darwin-* 条目,等于把 mac 用户的自动更新掐断。
# 合并逻辑与校验在 scripts/merge-latest-json-core.mjs(有单测)。
$work = New-Item -ItemType Directory -Force -Path (Join-Path $env:TEMP "notemd-$Version")
$latest = Join-Path $work 'latest.json'
Remove-Item $latest -ErrorAction SilentlyContinue
Invoke-Cli { gh -R $Repo release download $Tag --pattern latest.json --dir $work }
if (-not (Test-Path $latest)) { Die '线上没有 latest.json —— mac 侧发版可能没成功' }

# updater 产物就是安装包本身(Tauri 2 直接签它),所以 url 指向 setup.exe。
$setupUrl = "https://github.com/$Repo/releases/download/$Tag/$($setup.Name)"
Invoke-Cli { node scripts/merge-latest-json.mjs --file $latest --platform $PlatformKey --url $setupUrl --sig-file $sig.FullName --version $Version }
if ($LASTEXITCODE -ne 0) { Die '合并 latest.json 失败(上面有原因),尚未上传,线上仍是旧的' }

Invoke-Cli { gh -R $Repo release upload $Tag $latest --clobber }
if ($LASTEXITCODE -ne 0) { Die 'latest.json 上传失败' }

# ── 7. 回读校验:不信脚本输出,只信线上真实内容 ───────────────────────────────
Say '回读线上 latest.json 校验…'
$check = Join-Path $work 'verify'
Remove-Item $check -Recurse -ErrorAction SilentlyContinue
Invoke-Cli { gh -R $Repo release download $Tag --pattern latest.json --dir $check }
$live = Get-Content (Join-Path $check 'latest.json') -Raw | ConvertFrom-Json
$keys = $live.platforms.PSObject.Properties.Name | Sort-Object

if ($live.version -ne $Version) { Die "线上 latest.json 版本是 $($live.version),不是 $Version" }
foreach ($need in @('darwin-aarch64', 'darwin-x86_64', $PlatformKey)) {
  if ($keys -notcontains $need) { Die "线上 latest.json 缺 $need —— 平台条目被弄丢了!" }
}

# 成功路径同样要切回去 —— 否则跑通之后仓库停在 detached HEAD 上,
# 下一个来操作的人面对的是 tag 的代码而不是分支。
Restore-Ref

Write-Host ''
Write-Host "✓ $Tag 已补齐 Windows" -ForegroundColor Green
Write-Host "  平台条目:$($keys -join ', ')"
Write-Host "  安装包:https://github.com/$Repo/releases/tag/$Tag"
