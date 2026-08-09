# 交给 Windows 端 agent 的常驻任务书

> 用途:把「──── PROMPT ────」之间的内容整段贴给 Windows 机器上的 agent。
> 这是**长期流程**,不是一次性任务 —— 每次 note.md 发版都按它执行。
> 编写日期:2026-08-09 · 首个适用版本:v6.808.3

──────────────────── PROMPT ────────────────────

# 常驻职责:note.md 的 Windows 发版机

## 0. 先把这份流程记住

**这不是一次性任务。从现在起,note.md 每次发版你都要做同样的事。**

请把本文档完整保存到你自己的长期记忆(项目 `CLAUDE.md` / `AGENTS.md` / memory,取决于你的形态),标题写「note.md Windows 发版流程」。以后我只需要对你说「note.md 发了 vX.Y.Z,补 Windows 包」,你就应当能直接执行,不需要我再贴一遍这份文档。

**这份文档在仓库里也有一份权威副本:`docs/windows-agent-brief.md`。** 如果你的记忆和仓库里的版本冲突,以仓库为准,并更新你的记忆。

---

## 1. 发版模型:两台机器,两个阶段,一个 Release

note.md 是 macOS 和 Windows 双平台发布,但**版本号和 Release 由 macOS 侧单方面决定**,你只负责往里补东西:

```
阶段 1 —— macOS(别人做,你等着)
  scripts/release.sh
    bump 版本 → 打 tag vX.Y.Z → 建 GitHub Release
    → 传 2 个 dmg + 2 个 updater tarball
    → 传 latest.json(此时【只有】 darwin-aarch64 / darwin-x86_64 两个平台条目)

阶段 2 —— Windows(你做)
  scripts/release-windows.ps1 -Tag vX.Y.Z
    checkout 同一个 tag → 构建 → 把 setup.exe + updater 包传进【同一个】 Release
    → 把 windows-* 条目【合并】进那份【同一份】 latest.json
```

### 三条铁律

**① 你永远不创建 Release、不打 tag、不改版本号。**
版本号是 macOS 侧按日期推导的(`(年-2020).(月*100+日).(当日第几次)`)。你若自行推导,两台机器必然算出不同的号。你只认 tag。

**② 你必须构建与 tag 完全一致的代码。**
先 `git fetch --tags` 再 `git checkout vX.Y.Z`。不要在 main 的最新提交上构建 —— 那样产物内容和 tag 对不上,而签名照样会通过,没有任何一层能拦住这个错误。

**③ latest.json 只能【合并】,绝不能【覆盖】。**
线上那份里有 macOS 的两个平台条目。如果你本地新造一份再上传,mac 用户的自动更新会当场失效。正确做法是先 `gh release download --pattern latest.json` 把线上那份拉下来,合并,再传回去。`scripts/release-windows.ps1` 已经这么做了,**别绕过它手工传 latest.json**。

---

## 2. 一个会让你挂死几小时的坑(已有人损失 2.5 小时)

Tauri 解密 updater 签名私钥时,判据是**环境变量存不存在**,不是值是不是空字符串:

| 变量状态 | 行为 |
|---|---|
| 不存在 | 打印 `Decrypting updater signing key, expect a prompt for password` 然后**阻塞等 stdin** —— 非交互会话里永远等下去 |
| 存在且为空串 | 用空密码解密,通过 ✅ |
| 存在但值错误 | 立刻报 `incorrect updater private key password`(报错退出,不会挂) |

**我们这把密钥没有密码。** 所以要让变量「存在且为空」。

陷阱在于 **PowerShell 的 `$env:X = ""` 和 cmd 的 `set X=` 语义都是「删除变量」**,不是置空。必须用 .NET API:

```powershell
[Environment]::SetEnvironmentVariable("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "", "Process")
```

`release-windows.ps1` 已内置这一步并带自检。**只要你用那个脚本,就不会再踩。**

**通用纪律**:任何命令超过 10 分钟没有新输出,一律先怀疑它在等 stdin,去查环境变量,不要干等。

---

## 3. 一次性环境准备

只需做一次,之后每次发版直接跳到 §4。

**报告给我这三项**(我需要据此调整 toolchain 配置):Windows 版本、**CPU 架构(x64 / ARM64)**、物理机还是虚拟机。

安装:

- Git for Windows、GitHub CLI(`gh`,并 `gh auth login`)
- Node ≥ 20 + pnpm
- Rust(rustup,**必须 MSVC toolchain**,不是 gnu)
- Visual Studio Build Tools,勾选**「使用 C++ 的桌面开发」**
  必须含 MSVC 编译器 **和 Windows SDK**。缺 SDK 会在编 `ring` 这类带 C 代码的依赖时报找不到 `assert.h`。
- WebView2 Runtime(Win11 一般自带)
- NSIS 不用装,Tauri 自会下载

**两个仓库要 clone 在同一层目录**,主仓库通过 `file:../moraya-core` 引用编辑器内核:

```
<某目录>/
  note.md/        ← git clone https://github.com/wizlijun/note.md.git
  moraya-core/    ← git clone https://github.com/wizlijun/moraya-core.git
```

少了 moraya-core,`pnpm install` 直接 ENOENT。它的 `dist/` 已入库,**不需要**你构建。

**签名密钥**:从 Mac 拷来,放 `%USERPROFILE%\.tauri\mdeditor.key`。

- 这是应用自动更新的签名私钥,公钥已内嵌在程序里,**必须用这一把**,换钥匙客户端验签失败
- 整个文件就是一行 base64,全部内容都是密钥值,别只截取一段
- **绝不要**:提交进 git、打印到日志、贴回给我、放进 OneDrive 等同步目录

**ARM64 额外一步**:`rustup target add aarch64-pc-windows-msvc`,并告诉我 —— 仓库的 `src-tauri/rust-toolchain.toml` 目前只列了 x86_64,需要我补。

---

## 4. 每次发版要跑的(正常情况就这一条命令)

```powershell
cd note.md
pwsh scripts/release-windows.ps1 -Tag v6.808.3   # 换成实际 tag
```

脚本会自动完成:签名环境自检 → 对齐 tag → 识别架构 → 构建 → 上传 setup.exe 与 updater 产物 → 下载并合并 latest.json → 传回 → **回读线上内容校验**。

它在任何一步失败都会带原因退出,并且**在 latest.json 合并失败时不会上传**,线上保持旧的完好状态。

跑完请报告:输出末尾的平台条目清单、安装包大小、Release 链接。

---

## 5. 首次运行前还缺的构建配置

仓库里目前**还没有** `src-tauri/tauri.windows.conf.json`。第一次跑之前需要新建它(Tauri 会在 Windows 构建时自动 merge 这个文件):

```json
{
  "bundle": {
    "targets": ["nsis"],
    "icon": ["icons/icon.ico"],
    "windows": {
      "webviewInstallMode": { "type": "downloadBootstrapper" }
    }
  }
}
```

**为什么必须单独建这个文件**:主 `tauri.conf.json` 的 `bundle.targets` 是 `["app","dmg"]`(纯 macOS 目标),不处理的话 Windows 上编得出 .exe 但**不产出任何安装包**。而直接改主配置会破坏 macOS 的发版流水线。

关于图标:`src-tauri/icons/icon.ico` 存在但是脚手架默认图(仅 16/32px),不是品牌图。**先用它跑通,不要花时间做图标**,我会在 Mac 侧用品牌源图重新生成。

建完这个文件后**告诉我**,我会把它合进 main,之后你就不用再管了。

---

## 6. 已知会编译失败的地方

**只编主程序 `src-tauri`,不要编插件。**

`plugins-src/ebook-import/backend/Cargo.toml` 把 `core-foundation` / `core-graphics` / `foreign-types-shared` 列成了无条件依赖,Windows 上必然编不过。若 workspace 配置导致连带编到它,把那三行挪进 `[target.'cfg(target_os = "macos")'.dependencies]`,**并在报告里告诉我**。

---

## 7. 以下问题是预期的,记录即可,**不要动手修**

note.md 目前**还没有做 Windows 移植**,只是「能编能启动」。下列问题我已逐条分析过、有专门的改造方案(`docs/2026-08-08-pc-port-refactor-plan.md`),**你去修会浪费几天并与方案冲突**:

- 所有插件失效(架构标识符硬编码成 `*-apple-darwin`)
- 插件窗口打不开或 404(`plugin://` 在 WebView2 上被改写成 `http://plugin.localhost`)
- 菜单快捷键显示 `Cmd+X` 而非 `Ctrl+X`
- 各种路径相关功能出错(前端 39 处硬编码 POSIX 路径假设)
- 任何 git 操作(vault 同步)弹黑窗
- vault 同步报「git 不可用」(Windows 不自带 git,预期)
- 托盘图标行为异常、CLI 读错配置目录、「设为默认应用」不可用

**唯一例外**:若某问题导致**程序压根启动不了**,报告给我,并只做**最小改动**让它能启动,把改动单独列出。

---

## 8. 红线

- 不把密钥、`.env.release` 或任何凭据提交进 git,不打印到日志
- 不打 tag、不改版本号、不创建 Release
- 不手工上传 latest.json(必须走脚本的合并逻辑)
- 不改主 `tauri.conf.json`
- 不改任何 macOS 专属代码路径(`#[cfg(target_os = "macos")]` 内)
- **任何为让编译通过而做的改动,逐条列出**,我 review 后才合进 main

---

## 9. 每次发版后交回给我

1. §4 脚本输出末尾的平台条目清单(必须同时含 `darwin-aarch64`、`darwin-x86_64`、`windows-*`)
2. 安装包文件名与大小
3. 改动过的文件(附 `git diff`),没有就说没有
4. **首次**额外报:主界面截图、能否新建/编辑/保存 .md、关窗后 app 行为、有无黑窗闪烁
5. 卡住或失败处:**完整报错原文**,不要摘要

────────────────── PROMPT 结束 ──────────────────

## 给 Bruce 的备注(不要贴给 Windows agent)

**本次新增的东西**

- `scripts/release-windows.ps1` —— Windows 侧补包脚本,内置密码变量自检、tag 对齐校验、latest.json 下载-合并-回传、回读校验。
- `scripts/merge-latest-json-core.mjs` + `.mjs` CLI + `.test.ts`(10 条测试)—— 合并逻辑按仓库惯例做成 core+测试。校验包括:版本错位、空签名(正是密码没设时的症状)、URL 指向别的 tag、未知平台键、以及**只增不减**(合并后原有平台条目必须逐字节不变)。

**已核实的平台键**:取自 `tauri-plugin-updater-2.10.1/src/updater.rs` 的 `updater_os()`/`updater_arch()`,是 `windows-x86_64` / `windows-aarch64`;客户端查找顺序为 `{os}-{arch}-{installer}` → `{os}-{arch}`,所以写不带 installer 后缀的通用键即可。

**一个已知空窗期**:mac 发版后、Windows 补包前,那个 Release 的 latest.json 没有 windows 条目,此时 Windows 用户查更新会得到「无可用更新」。目前 Windows 用户为零,可接受。若将来要消除,做法是让 `release.sh` 把上一版的 windows 条目先结转进去。

**待办**:`release.sh:41` 的注释写着 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD optional, leave unset if no password`,与第 125 行 `export ...="${...:-}"` 的实际行为矛盾 —— 这条误导性注释是本次事故的间接来源,应改。
