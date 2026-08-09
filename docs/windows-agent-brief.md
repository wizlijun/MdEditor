# note.md Windows 发版流程

> 权威副本。Windows 发版机的 agent 记忆若与本文冲突,**以本文为准**,并更新记忆。
> 编写日期:2026-08-09 · 首个适用版本:v6.808.3

## 0. 这是常驻流程

note.md 每次发版都要执行本文。用户说「note.md 发了 vX.Y.Z,补 Windows 包」即可,
不必再贴一遍流程。

## 1. 发版模型:两台机器,两个阶段,一个 Release

版本号和 Release 由 macOS 侧单方面决定,Windows 侧只往里补东西。

```
阶段 1 —— macOS(别人做)
  scripts/release.sh
    bump 版本 → 打 tag vX.Y.Z → 建 GitHub Release
    → 传 2 个 dmg + 2 个 updater tarball
    → 传 latest.json(此时只有 darwin-aarch64 / darwin-x86_64 两个条目)

阶段 2 —— Windows(本文)
  scripts/release-windows.ps1 -Tag vX.Y.Z
    对齐 tag → 构建 → 签名 → 传 setup.exe + .sig 进【同一个】 Release
    → 把 windows-x86_64 条目【合并】进那份【同一份】 latest.json → 回读校验
```

### 三条铁律

**① 永不创建 Release、不打 tag、不改版本号。** 版本号在 macOS 侧按日期推导
(`(年-2020).(月*100+日).(当日第几次)`)。自行推导必然与 mac 算出不同的号。只认 tag。

**② 必须构建与 tag 完全一致的代码。** 在 main 最新提交上构建会产出与 tag 内容
不符的包,而**签名照样通过** —— 没有任何一层能拦住这个错误。脚本用 `-Tag` 强制对齐。

**③ `latest.json` 只能合并,绝不能覆盖。** 线上那份含 macOS 的两个平台条目;
本地新造一份传上去,mac 用户的自动更新当场失效。脚本会先下载线上那份再合并,
**别绕过它手工传**。

## 2. 会挂死几小时的坑:空的签名密码

Tauri 解密 updater 私钥时,判据是环境变量**存不存在**,不是值是否为空:

| 变量状态 | 行为 |
|---|---|
| 不存在 | 打印 `Decrypting updater signing key, expect a prompt for password`,然后**阻塞等 stdin** —— 非交互会话永远等下去 |
| 存在且为空串 | 用空密码解密,通过 ✅ |
| 存在但值错误 | 立刻报 `incorrect updater private key password`(报错退出,不挂) |

难受的地方在于:挂住时安装包**已经产出**,日志最后一行是 `building`,完全不像
密码问题。

**本项目这把密钥没有密码**,所以要让变量「存在且为空」。而在 Windows 上,以下
三种写法**全都是删除变量**:

- `$env:X = ""`(PowerShell)
- `set X=`(cmd)
- `[Environment]::SetEnvironmentVariable("X", "", "Process")` ← Windows PowerShell
  5.1 跑在 .NET Framework 上,Win32 `SetEnvironmentVariableW` 传空串即删除

**唯一可行的是手工构造子进程的环境块**:

```powershell
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.UseShellExecute = $false
$psi.Environment['TAURI_SIGNING_PRIVATE_KEY_PASSWORD'] = ''   # 这里能存住空值
```

自检:让子进程跑 `cmd /c set TAURI_SIGNING_PRIVATE_KEY_PASSWORD`,
应输出 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD=` 且退出码 0。

`release-windows.ps1` 已内置这套做法并带自检 —— **只要用那个脚本就不会踩。**

**通用纪律**:任何命令超过 10 分钟没有新输出,先怀疑它在等 stdin,去查环境变量,
不要干等。

## 3. 密钥变量的另一个坑

Tauri 只认 `TAURI_SIGNING_PRIVATE_KEY`,值必须是**整个密钥文件的 base64**。

`TAURI_SIGNING_PRIVATE_KEY_PATH` 是 `scripts/release.sh` 自己的约定(它 `cat`
文件后导出成前者),**Tauri 本身不认**。把规范形态的密钥文件内容直接传进去会得到:

```
failed to decode base64 key: Invalid symbol 32, offset 9
```

offset 9 正是 `untrusted comment:` 里的那个空格。脚本两种形态都认并自动转换。

## 4. 一次性环境准备

需要:Git for Windows、Node ≥ 20 + pnpm、Rust(rustup,**MSVC** 而非 gnu)、
Visual Studio Build Tools 勾选「使用 C++ 的桌面开发」(必须含 MSVC 编译器**和**
Windows SDK,缺 SDK 会在编 `ring` 这类带 C 代码的依赖时报找不到 `assert.h`)、
WebView2 Runtime。NSIS 不用装,Tauri 自会下载。

上传需要 `gh`(并 `gh auth login`)**或**一个有 `contents: write` 的
`GH_TOKEN`/`GITHUB_TOKEN`。脚本两者都支持,优先用 `gh`。

**两个仓库要 clone 在同一层目录**,主仓库通过 `file:../moraya-core` 引用编辑器内核:

```
<某目录>/
  note.md/        ← git clone https://github.com/wizlijun/note.md.git
  moraya-core/    ← git clone https://github.com/wizlijun/moraya-core.git
```

少了 moraya-core,`pnpm install` 直接 ENOENT。它的 `dist/` 已入库,不需要构建。

**从 Git Bash 跑 `pnpm install` 会失败**:pnpm 把 MSYS 形态的 PATH
(`/c/Program Files/nodejs`)原样传给 lifecycle script,esbuild 的 postinstall 用
cmd.exe 执行时找不到 `node`。用 PowerShell。

**签名密钥**放 `%USERPROFILE%\.tauri\mdeditor.key`(从 Mac 拷)。绝不提交进 git、
不打印到日志、不放进 OneDrive 等同步目录。项目有两把 minisign 私钥,更新器用的是
key id `CC775B462AC3BF41` 那把;另一把 `2BAFE555935FE0A9` 是签插件包的,别拿错。
**绝不能用 `tauri signer generate` 生成新的来"解决"密钥缺失** —— 公钥钉死在程序里,
换一把等于让所有已安装用户的自动更新失效。

**ARM64 额外一步**:`rustup target add aarch64-pc-windows-msvc`,并告知维护者 ——
`src-tauri/rust-toolchain.toml` 目前只列了 x86_64。

## 5. 每次发版

```powershell
cd note.md
pwsh scripts/release-windows.ps1 -Tag v6.808.3   # 换成实际 tag
```

脚本依次完成:签名环境自检 → 对齐 tag → 构建 → 架构断言(读 PE 头 Machine 字段,
对应 macOS 侧的 lipo 断言)→ 签名 key id 校验 → 下载线上 `latest.json` → 合并
`windows-x86_64` → 上传 → 回读校验。

任何一步失败都带原因退出;`latest.json` 合并失败时**不上传**,线上保持旧的完好状态。

跑完报告:平台条目清单(必须同时含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`)、
安装包大小、Release 链接。

## 6. 只编主程序

`plugins-src/ebook-import/backend/Cargo.toml` 把 `core-foundation` /
`core-graphics` / `foreign-types-shared` 列成无条件依赖,Windows 上编不过。
只编 `src-tauri` 时 workspace 不会连带编它,实测没有触发。若真被连带编到,
把那三行挪进 `[target.'cfg(target_os = "macos")'.dependencies]` 并报告。

## 7. 预期内的功能缺失

Windows 移植的完整方案见 `docs/2026-08-08-pc-port-refactor-plan.md`。
截至 v6.808.3,分支 `windows-x64-port` 已修掉相当一部分(菜单快捷键、前端路径层、
插件子进程环境、`safe_path`、主题 file URL、插件安装的 junction 回退、子进程黑窗、
CLI 配置目录),但以下仍未做,遇到记录即可,**不要动手修**:

- `plugin://` 在 WebView2 上被改写成 `http://plugin.localhost`,插件窗口鉴权失效
  (需要方案 §3 的 token 化)
- 插件市场的架构标识符仍是 `*-apple-darwin`
- 托盘角标、「设为默认应用」
- vault 同步报「git 不可用」(Windows 不自带 git,预期;设置页已有 git 代理项)

**唯一例外**:若某问题导致程序**压根启动不了**,报告并只做最小改动让它能启动,
把改动单独列出。

## 8. 红线

- 不把密钥、`.env.release` 或任何凭据提交进 git,不打印到日志
- 不打 tag、不改版本号、不创建 Release
- 不手工上传 `latest.json`(必须走脚本的合并逻辑)
- 不改主 `tauri.conf.json`(Windows 侧的覆盖走 `tauri.windows.conf.json`,
  Tauri 在 Windows 构建时自动 merge)
- 不改任何 `#[cfg(target_os = "macos")]` 内的代码路径
- 任何为让编译通过而做的改动,逐条列出,review 后才合进 main
