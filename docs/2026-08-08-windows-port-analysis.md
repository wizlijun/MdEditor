# note.md Windows 版发布 —— 兼容性分析与工作量评估

> 日期:2026-08-08 · 代码基线:`main` @ 3dd75ff(v6.806.5)· 方法:全量静态审计(Rust 后端 / Svelte 前端 / 11 个插件 / 发布脚本),关键结论逐条对照源码与上游 crate 源码核实,带 `文件:行号` 引用。
>
> **一句话结论:没有不可逾越的技术障碍,但也不是"加个 target 就能出包"。主程序本体的跨平台度尚可,真正的成本集中在三处 —— 插件运行时的 `plugin://` 协议在 WebView2 上会塌成单一 origin(既是功能问题也是安全回退)、6 个带原生二进制的插件里有 4 个深度绑定 macOS 框架、以及一条完全 macOS-only 的发布流水线。整体量级约 2.5–4 人月。**

---

## 0. 结论摘要

### 0.1 三句话版本

1. **主程序**:97 处 `target_os` 判断里 84 处是 iOS、13 处是 macOS,说明"平台分叉"这件事在架构上已经做过一遍;但**没有一处 `target_os = "windows"`** —— 所有非 macOS 分支都是"返回不支持"的降级桩,不是可用实现。
2. **插件体系**:进程模型(stdio + NDJSON JSON-RPC)天然可移植,但**插件 UI 的加载协议、插件的身份鉴权、插件二进制的架构白名单**三件事都写死在 macOS 假设上。
3. **插件生态**:11 个插件里 4 个纯 UI 的几乎零成本;roam-import 轻改;claude-agent / openclaw 中等;**md2pdf、ebook-import、pos-log 需要重写渲染/OCR/定位后端,或在 Windows 上直接不上架**。

### 0.2 工作量估算

| 阶段 | 内容 | 估算 |
|---|---|---|
| 阶段 1 | 主程序在 Windows 上编译、启动、可用(无插件) | 17–25 人日 |
| 阶段 2 | 插件运行时可用(协议、隔离、安装器、CLI) | 8–12 人日 |
| 阶段 3 | 插件逐个移植(不含 pos-log) | 20–35 人日 |
| 阶段 4 | 发布/签名/更新流水线 | 8–13 人日 + 证书采购周期 |
| **合计** | | **约 53–85 人日(2.5–4 人月)** |

### 0.3 强烈建议先做的产品决策

在动工前拍板三件事,否则阶段 3 的估算会失控:

- **Windows 首发是否带插件市场?** 若首发只发"编辑器本体 + vault 同步",阶段 2/3 可整体后置,首发成本降到 20–28 人日。
- **md2pdf / ebook-import 在 Windows 上做不做?** 这两个是纯重写(见 §5)。
- **pos-log 建议直接标记 macOS-only**,Windows 上的位置服务价值与成本严重不匹配。

---

## 1. 现状基线

### 1.1 技术栈

- Tauri 2.11 + Rust(`src-tauri`,`notemd` 二进制)+ Svelte 5 / Vite 6 前端
- 编辑器内核 `@moraya/core`(ProseMirror,`file:../moraya-core` 本地依赖)
- 插件体系 v2:独立进程 + stdio NDJSON JSON-RPC(`plugin-protocol` / `notemd-plugin-sdk`)
- 插件 UI:独立 Tauri 窗口,经自定义 `plugin://` scheme 加载,零 capability
- vault 同步:**外部 `git` CLI**(不是 libgit2,见 §4.4)

### 1.2 平台相关代码分布

```
target_os 出现 97 次 / 9 个文件:
  ios    84   (iOS 分支,与本次无关)
  macos  13
  windows 0   ← 一处都没有
```

已经做好 cfg 隔离、能干净地在 Windows 上编译成"不支持"桩的:

| 位置 | 内容 | Windows 现状 |
|---|---|---|
| `lib.rs:275-380` | LaunchServices 默认应用注册 | 已有 `#[cfg(not(macos))]` 返回 "only supported on macOS" |
| `plugin_runtime/location.rs:22-47` | CoreLocation 定位 | 已有非 macOS 桩 |
| `themes/commands.rs:22-31` | `theme_reveal` 用 `open` | 已有非 macOS 桩(返回 Err) |
| `agents_sync/mod.rs:59-78` | CLAUDE.md 符号链接 | **已写好 `std::os::windows::fs::symlink_file` 分支** |
| `plugins-src/md2pdf/Cargo.toml:22` | objc2/WebKit/PDFKit | 已 target 化(但没有替代实现) |

这说明工程里"平台意识"是有的 —— 障碍主要不在于代码脏,而在于**从来没有人写过 Windows 那一侧的实现**。

---

## 2. P0 —— 编译与打包阻断项

这些不解决,`cargo build --target x86_64-pc-windows-msvc` 或 `tauri build` 直接失败。

### 2.1 `git2` 的 `vendored-openssl` 会炸掉 Windows 构建 ⭐ 高优先级、低成本

`src-tauri/Cargo.toml:69-73`:

```toml
[target.'cfg(target_os = "ios")'.dependencies]
git2 = { ..., features = ["vendored-libgit2", "vendored-openssl"] }

[target.'cfg(not(target_os = "ios"))'.dependencies]
git2 = { ..., features = ["vendored-libgit2", "vendored-openssl"] }
```

`vendored-openssl` 会让 `openssl-src` 在 windows-msvc 上从源码构建 OpenSSL,需要 **Strawberry Perl + NASM** 在 PATH 中,CI/本地都极易失败。

**而且这个依赖在桌面端根本没用**:全仓 `git2::` 的使用点只出现在 `src-tauri/src/vault_ios/`(iOS 专用),桌面端所有 git 操作走外部 `git` CLI。

**修法(约 0.5 天)**:把 `git2` 收窄成 iOS-only 依赖,删掉 `cfg(not(target_os = "ios"))` 那一块。这同时还能给 macOS/Windows 构建瘦身。

### 2.2 `rust-toolchain.toml` 无 Windows target

`src-tauri/rust-toolchain.toml` 只声明了 4 个 Apple target。需要加 `x86_64-pc-windows-msvc`(和可选的 `aarch64-pc-windows-msvc`,ARM Windows)。

注意:**Windows 版无法在 macOS 上交叉编译** —— WebView2 的 loader、MSVC link、NSIS 打包都要求在 Windows 上构建。必须准备一台 Windows 构建机(物理机 / Parallels VM / 云 runner)。

### 2.3 `tauri.conf.json` 没有任何 Windows 打包配置

`src-tauri/tauri.conf.json`:

- `bundle.targets: ["app", "dmg"]` —— 需要加 `"nsis"`(推荐)或 `"msi"`
- `bundle.icon: ["icons/icon.icns", "icons/icon.png"]` —— 缺 `icons/icon.ico`。文件本身存在(32/16px,6 张),但是**脚手架默认图标,不是 `scripts/gen-macos-icon.sh` 那套品牌图标**,需要重新生成一份多尺寸(16/32/48/256)ico
- 缺 `bundle.windows`(WebView2 安装策略、证书指纹、NSIS 语言包等)
- `plugins.updater.windows.installMode: "passive"` —— **这个已经配好了**,是唯一一处已有的 Windows 准备
- `fileAssociations` 列了 60+ 扩展名。Tauri 的 NSIS 后端支持文件关联,但会往注册表写 60+ 条 ProgID;建议 Windows 上收敛成 `md/markdown/mdown/mkd/txt` 一小撮,其余靠"打开方式"

### 2.4 WebView2 运行时依赖

Windows 10 1803+ 通常已预装 Edge WebView2 Runtime,但不保证。需要在 NSIS 里选 `downloadBootstrapper`(体积小、需联网)或 `embedBootstrapper`。这会打破 README 里"7 MB 下载"的卖点 —— 对外宣传需要为 Windows 单独措辞。

### 2.5 插件侧:`ebook-import` 无条件依赖 CoreGraphics

`plugins-src/ebook-import/backend/Cargo.toml:24-26` 把 `core-foundation` / `core-graphics` / `foreign-types-shared` 列为**无条件依赖**(不是 `[target.'cfg(target_os="macos")']`)。在 Windows 上连编译都过不去。详见 §5。

---

## 3. P0 —— 架构级不兼容

这三条不是"改几行",是需要设计决策的。

### 3.1 `plugin://` 自定义协议在 WebView2 上会塌成单一 origin ⭐⭐ 最高优先级

**这是整个移植里最硬的一块。**

当前设计(`plugin_runtime/protocol.rs:1-7`):

- 插件 UI 加载 `plugin://<插件id>/<entry>`(`windows.rs:195`)
- 服务端从 **URL host 取插件 id**(`protocol.rs:370,386`)
- **插件身份鉴权靠 Origin 头**:`let expected = format!("plugin://{plugin_id}")`(`protocol.rs:223`)
- 每个插件因此有独立的浏览器 origin,`'self'` CSP 天然只覆盖它自己(`protocol.rs:80`)
- 前端 `CustomEditorIframe.svelte:11-12` 也用 `plugin://<id>` 作为 `postMessage` 的 targetOrigin,并据此校验来源(`lib/plugins/v2/custom-editor-msg.ts`)

**上游行为(已核实,不是推测)**:wry 0.55 `src/lib.rs:1172,1190` 的文档明确写着,Windows/Android 上自定义协议会被改写为
`{protocol}://<host>/abc` → `http://{protocol}.localhost/<host>/abc`;
tauri 2.11 `src/webview/mod.rs:2388` 的测试注释也确认 "On Windows/Android, custom protocols are served as `https://<name>.localhost/`"。

**后果**:

1. `plugin://notemd.md2pdf/index.html` 在 Windows 上变成 `http://plugin.localhost/notemd.md2pdf/index.html` —— **插件 id 从 host 掉到了 path 第一段**,`protocol.rs` 的 id 提取逻辑直接失效,所有插件窗口 404。
2. 更严重:**所有插件共享同一个 origin `http://plugin.localhost`**。基于 Origin 头的插件身份鉴权(`protocol.rs:223`)不再能区分调用方,`'self'` CSP 也不再是"仅本插件"。**这是一次实打实的安全能力回退** —— 恶意插件 A 的窗口可以冒充插件 B 调用 `__rpc__`。
3. `custom-editor-msg.ts` 的 targetOrigin 校验同样退化成"任意插件皆可"。

**修法(5–8 人日,含设计与安全评审)**:

- 抽出 `plugin_url(id, path)` 与 `origin_of(request)` 两个平台抽象,替换 6 处硬编码
- **必须为 Windows 补一套不依赖 origin 的鉴权**。可选方案:
  - (a) 每个插件窗口注入一次性 `initialization_script` 的 capability token,`__rpc__` 请求带 token 头,宿主按 token→plugin_id 查表(推荐,平台无关,顺带把 macOS 侧也加固了)
  - (b) 给每个插件注册**独立的 scheme**(`pluginXXX://`),Windows 上得到 `http://pluginXXX.localhost` 的独立 origin —— 但 scheme 需在启动时静态注册,与动态安装插件冲突,不可行
  - (c) 走 `http://<id>.plugin.localhost`(子域)—— wry 不支持自定义映射,不可行
- 建议直接做 (a),并把它作为**跨平台统一方案**,而不是 Windows 分支

### 3.2 插件二进制的架构白名单只有 apple-darwin

`plugin_runtime/discovery.rs:15-18`:

```rust
pub(crate) fn current_arch_triple() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "aarch64" => Some("aarch64-apple-darwin"),
        "x86_64"  => Some("x86_64-apple-darwin"),
        _ => None,
    }
}
```

在 Windows 上 `ARCH == "x86_64"` → 返回 `x86_64-apple-darwin` → 所有带二进制的插件都会去找一个不存在的 macOS 二进制并报 "no binary for host arch"。**注意这不是返回 None 报错,而是静默匹配到错误的 triple**,排查体验很差。

修法:改用 `std::env::consts::OS` + `ARCH` 组合,产出 `x86_64-pc-windows-msvc` 等;所有插件 `manifest.v2.json` 的 `binary` map 增加 Windows key。市场索引侧是**通用 arch-keyed 设计**(`market.rs:60-76`、`scripts/gen-plugin-index.mjs:65-76`),**无需改动** —— 这一点设计得很好。

另需检查安装器:`installer.rs:201-203` 用 `std::os::unix::fs::PermissionsExt` 设可执行位;Windows 上不需要但代码需 cfg 保护。

### 3.3 CLI 的配置目录硬编码 macOS 路径

`cli/mod.rs:34-40`:

```rust
pub fn resolve_config_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return Path::new(&home).join("Library").join("Application Support").join(APP_BUNDLE_ID);
    }
    PathBuf::from(".")
}
```

Windows 上 GUI 用 `app_config_dir()`(→ `%APPDATA%\net.notemd.app`),而 CLI 会算出 `%HOME%\Library\Application Support\...`(Git Bash 下)或 `.`(cmd 下)。**两者读到不同的 settings.json** —— 这正是 memory 里 `reference_cli_frontend_store_not_loaded` 那类坑的翻版。同一问题也存在于 `cli/runner.rs:101` 与 `cli/builtin.rs:456-462`(那两处用 `dirs::data_dir()`,本身跨平台正确,但**与 `resolve_config_dir` 在 Windows 上不再一致**,而 `runner.rs:441` 的注释明确说这个等价关系"仅在 macOS 上成立")。

另:`cli/mod.rs:52-70` 的 `is_cli_mode` 靠 `argv[0]` 是否含 `.app/Contents/MacOS/` 判定。Windows 上 GUI 是 `notemd.exe`,`file_name()` 为 `"notemd.exe"` ≠ `"notemd"` → 恰好不会误入 CLI 模式(靠运气,不靠设计),但**符号链接式 CLI 在 Windows 上也就完全不可达**,只剩 `--cli` 显式入口。

### 3.4 前端 POSIX 路径假设遍布 30 个文件

`src/` 下 **39 处 / 30 个文件**使用 `split('/')` / `lastIndexOf('/')` / `startsWith('/')`,例如:

- `lib/folder-view.svelte.ts:29-40` `parentDir()` 用 `lastIndexOf('/')`,根目录返回 `'/'`
- `lib/fs.ts:38-43` 路径拼接固定用 `/`
- `lib/link-open.ts:22-35` 相对链接解析全按 POSIX
- `lib/sotvault-logic.ts:32,81,89` vault 相对路径计算
- `lib/recent-merge.ts:114-118` 用 `~` 缩写 home

后端也有对称问题:`lib.rs:652` 的 `abbreviate_path` 直接读 `$HOME` 做 `~` 缩写。

**好消息**:`lib/paste-resources.ts` 已经在多处做了 `.replace(/\\/g, '/')` 归一化,`lib/editor-bridge.ts:20` 甚至有 `const sep = filePath.includes('\\') ? '\\' : '/'` —— 说明有人想过这件事,只是没有推广。

**修法(3–5 人日)**:引入统一的 `path` 工具模块(`join/dirname/basename/relative/normalize/isAbsolute`),内部按平台归一化(建议内部一律用 `/`,只在跟系统 API 交界处转换),然后机械替换这 39 处 + 补测试。这是纯体力活,但量不小且容易漏。

---

## 4. P1 —— 功能性不兼容(逐模块)

### 4.1 菜单与快捷键:19 处硬编码 `Cmd+`

`lib.rs` 里有 **19 处** `.accelerator("Cmd+...")`(1966–2076 行区间),Tauri 在 Windows 上不认 `Cmd`,应统一改成 `CmdOrCtrl`。同时:

- **菜单结构本身是 macOS 形态**:`lib.rs:1957` 建了一个叫 "note.md" 的应用菜单,含"关于/隐藏/偏好设置/退出"。Windows 惯例是"关于/退出"进 Help 与 File,没有应用菜单;`app.hide` 更是没有对应概念。Windows 需要一套单独的菜单编排。
- **前端展示层**:19 处 UI 字符串里写着 `Cmd`/`⌘`。`lib/outline/shortcuts.ts:45-54` 的 `displayShortcut(shortcut, isMac)` **已经支持双形态**,但 `SettingsDialog.svelte:388` 是靠 `navigator.platform.includes('Mac')` 判断 —— 在 WebView2 里返回 `Win32`,能正确降级(可用,但应改走统一的 `platform()`)。
- **`isMacOS: true` 被硬编码传给编辑器内核**:`src/lib/editor-bridge.ts:13` 和 `src/editor-kit/rich.ts:73` 都写死 `isMacOS: true`。moraya-core 用它决定键位与部分装饰逻辑(`../moraya-core/src/plugins/editor-props-plugin.ts:161,592,622`)。Windows 上必须改成动态值,否则编辑器键位行为错乱。
- **全局热键语义冲突**:`lib.rs:1203` 注册 `Cmd+Ctrl+M` / `Cmd+Ctrl+N`,实现用 `Modifiers::SUPER | Modifiers::CONTROL`(`lib.rs:1008`)。Windows 上 SUPER = Win 键,`Win+Ctrl+M/N` 与系统虚拟桌面快捷键相邻且体验古怪。Windows 应换一套(如 `Ctrl+Alt+M/N`),并同步 `build_tray_menu` 里的展示串。插件也受影响:`idea-spark/manifest.v2.json:41` 声明了 `"accelerator": "Cmd+Ctrl+I"`,幸好宿主的解析器(`lib.rs:60-65`)已做别名归一。

### 4.2 托盘:角标(数字)在 Windows 上不显示

`lib.rs:855` 用 `tray.set_title(Some(n.to_string()))` 显示通知角标。Tauri 2.11 `src/tray/mod.rs:537` 明确:**`set_title` 在 Windows 上 Unsupported**。

统一通知基建(`project_unified_notifications`)的四态黄灯/角标是产品级功能,Windows 上会静默失效。替代方案:按 count 切换不同的托盘图标(0/1/2/3+/N),或用 Windows 原生 toast 通知补足。`set_tooltip` 在 Windows 上可用(仅 Linux 不支持),tooltip 那条信息不丢。

### 4.3 子进程会闪黑窗 ⭐ 高可见度、低成本

Rust 侧运行时会 spawn 的外部进程:

| 位置 | 命令 | 频率 |
|---|---|---|
| `vault_sync/git_ops.rs:12,21` | `git`(所有同步操作的漏斗) | 周期性、频繁 |
| `git_history/mod.rs:116` | `git diff --no-index` | 打开历史面板时 |
| `okf/mod.rs:37` | `git config --get` | 写 OKF 文档时 |
| `plugin_runtime/process.rs:67` | 插件二进制 | 每次插件激活 |

Windows 上 `std::process::Command` **默认会弹一个控制台窗口**。vault 同步是周期任务 —— 用户会看到黑框不停闪烁,这是"这软件不专业"的第一印象来源。

**修法(1 天)**:所有 spawn 点加 `#[cfg(windows)] .creation_flags(CREATE_NO_WINDOW /* 0x08000000 */)`,包装成一个 `spawn_hidden()` 工具函数强制走同一入口。

### 4.4 vault 同步依赖外部 `git`,Windows 不自带

桌面端所有 git 操作走 CLI(`git2` 只在 iOS 用)。macOS 上 Xcode CLT 基本保证有 git;**Windows 上默认没有**。

`git_ops.rs:9-18` 的 `version()` 已经会探测 git 是否存在并"显式报告 git unavailable",所以不会静默失败 —— 但需要为 Windows 补一条引导路径:检测缺失时给出下载链接/一键跳转,或考虑打包 MinGit(约 40 MB,与体积卖点冲突)。

**建议**:首发保持"需自行安装 Git for Windows",在 onboarding 里显式引导。

### 4.5 文件锁与原子写

Windows 的强制文件锁(mandatory locking)会带来一类 macOS 上不存在的失败:

- `std::fs::rename` 覆盖已存在文件在 Windows 上可行,但**若目标文件被另一进程打开则失败**。全仓有 9 处 `fs::rename` / `NamedTempFile::persist` 式原子写。
- 编辑器保存 + 文件监听 + vault git 同步三者并发时,Windows 更容易撞上 "file in use"。memory 里 `reference_vault_sync_no_worktree_revert` 记录的那个"已被其他应用修改"误报窗口,在 Windows 上出现概率更高。
- 杀毒软件(Defender 实时扫描)会短暂持有新写入的文件句柄,加剧上述问题。

**修法**:所有原子写加"失败重试 + 短退避";vault 目录建议在首启时提示用户加入 Defender 排除项。这块需要实机压测,估 2–3 天。

### 4.6 文件监听语义差异

`notify = { version = "7", default-features = false, features = ["macos_fsevent"] }` —— 该 feature 在 Windows 上是无害的(Windows 后端由 cfg 选择,始终编译),**不是编译阻断**。但 `ReadDirectoryChangesW` 与 FSEvents 语义不同:重命名事件成对/不成对、递归监听的句柄开销、大 vault 下的事件风暴。需要针对 vault watcher 做实机验证。

### 4.7 CLAUDE.md 符号链接需要"开发者模式"

`agents_sync/mod.rs:59-78` 已有 `std::os::windows::fs::symlink_file` 分支,并且注释就写着 "Fails on Windows"。事实上 Windows 上创建符号链接需要管理员权限**或**开启"开发者模式"。

代码在失败时会 `crate::dlog(...)` 跳过(`mod.rs:114-115`),不会崩 —— 但 CLAUDE.md 就不会生成。**建议**:Windows 上降级为"复制 + 内容监听同步"(即回到 memory 里 `project_claude_md_symlink` 改造前的形态),或直接不生成 CLAUDE.md、只留 AGENTS.md。

### 4.8 各类 macOS-only 桩需要 Windows 实现

| 功能 | 当前 | Windows 需要 |
|---|---|---|
| 设为默认应用(`lib.rs:346-380`) | LaunchServices | 注册表 ProgID + `SHChangeNotify`,或引导用户去"默认应用"设置页 |
| 主题目录 reveal(`themes/commands.rs:19-31`) | `open` | `explorer.exe /select,` —— 或直接改用已在前端使用的 `revealItemInDir`(`@tauri-apps/plugin-opener`,跨平台,`sotvault.svelte.ts:148`/`folder-view.svelte.ts:337` 已在用),**建议统一到 opener 插件,顺手删掉 macOS 分支** |
| CLI 安装(`cli/install.rs:97,129`) | `osascript` 提权 + `/usr/local/bin` 软链 | 写 `%LOCALAPPDATA%\Programs\notemd` 并追加 PATH,或干脆 Windows 不提供 CLI 安装项 |
| Rosetta 检测(`lib.rs:921`) | `sysctl` | 已是 `cfg(all(macos, x86_64))`,无需处理 |

### 4.9 窗口与生命周期语义

`lib.rs:210-228` 的 `should_prevent_exit` 让"最后一个窗口关闭"不退出应用(macOS 惯例),常驻托盘。**Windows 用户预期相反** —— 关掉窗口就该退出,或至少要有明确的"最小化到托盘"设置项。这条不改会造成"关不掉的软件"投诉。

另:deep-link 在 Windows 上需要注册表注册,`tauri.conf.json` 目前**没有 `plugins.deep-link` 配置**(macOS 靠 `Info.plist`)。需要补 `desktop.schemes`。

### 4.10 字体与外观

`src/styles/app.css:2,45` 用 `-apple-system, BlinkMacSystemFont, 'Helvetica Neue', sans-serif` —— Windows 上会落到系统默认无衬线(通常是 Segoe UI 的兜底或更糟)。等宽栈 `ui-monospace, 'SF Mono', Menlo, Consolas, monospace`(`editor-kit/kit.css:53`)里有 Consolas,还算合理;但 `editor-base.css:239,303,619` 三处**只有 `Menlo`,没有 Consolas**。

需要补一轮字体栈(`'Segoe UI', 'Microsoft YaHei UI'` 等)与中日韩字形验证 —— 这是"看起来像原生应用"的关键,别省。

### 4.11 WebView2 vs WKWebView 的编辑器行为差异(不确定性最高)

memory 里已有多条 WKWebView 特有的坑记录,这些补丁在 WebView2 上要么无效、要么可能反而致错:

- `RichEditor.svelte:957` 的 Cmd+A 修复,注释明说是绕 "WebKit 的 `selectAll:` responder action"(v6.806.5 刚修的)
- `SourceView.svelte` 的透明 textarea 选区绘制(memory `reference_find_editor_gotchas`)
- `reference_editor_prewrap_html_decoration`、`reference_prosemirror_schema_identity` 等一系列渲染/选区行为
- IME(中文输入)在 WebView2 + ProseMirror 下的 composition 事件序列与 WebKit 有差异
- 拖放(`dragDropEnabled: true`)、剪贴板图片粘贴(`lib/paste-resources.ts`、`lib/copy-image.ts`)行为差异

**这块估 3–5 天,但方差最大** —— 只能靠实机回归。建议在阶段 1 早期就先跑一个"能打字、能保存"的最小版本上机,把未知量前置暴露。

---

## 5. 插件兼容矩阵

`plugins-src/` 共 11 个(其中 `custom-editor-fixture` 是测试夹具,不上架)。

| 插件 | 形态 | 原生二进制 | Windows 结论 | 主要障碍 | 估算 |
|---|---|---|---|---|---|
| **decision-log** | 纯 UI | 无 | ✅ 直接可用 | 仅需 §3.1 协议适配 | 0.5d |
| **idea-spark** | 纯 UI | 无 | ✅ 直接可用 | manifest 里的 `Cmd+Ctrl+I` 需换键位 | 0.5d |
| **weekly-review** | 纯 UI | 无 | ✅ 直接可用 | — | 0.5d |
| **power-mode** | 纯 UI | 无 | ✅ 直接可用 | 引擎在宿主侧,插件只是控制台;需验证震动/特效在 WebView2 的性能 | 1d |
| **roam-import** | UI + Rust 后端 | 是 | 🟡 轻改 | `discover.rs:20-21` 硬编码 `/opt/homebrew/bin`、`/usr/local/bin` 找 `roam` CLI;`PermissionsExt` 判可执行位(`discover.rs:75`、`ledger.rs:113,258`) | 1–2d |
| **openclaw-chat** | UI + Rust 后端 | 是 | 🟠 中等 | `uds_client.rs:9,45` 用 **Unix Domain Socket** 连本地 openclaw。Windows 需换命名管道或 TCP loopback,并与 openclaw 端协商;其余(WebSocket/QR/HMAC)跨平台 | 3–5d |
| **claude-agent** | UI + Rust 后端 | 是 | 🟠 中等 | `runner.rs:54-56`/`engine.rs:138-140` 用 `setsid()` 脱离会话、`engine.rs:307` 用 `killpg` 杀进程组、`lock.rs:73` 用 `libc::kill(pid,0)` 探活;`discover.rs:12-13,109-110` 硬编码 homebrew 路径找 `claude` CLI。Windows 需换 Job Object + `CREATE_NEW_PROCESS_GROUP`,PATH 发现改走 `where claude` | 3–5d |
| **md2pdf** | 后端 only | 是 | 🔴 重写 | 渲染链完全是 `WKWebView` + `WKPDFConfiguration` + `PDFKit`(`Cargo.toml:22-45`)。Windows 需改用 WebView2 的 `PrintToPdf`(需要在宿主进程内起 webview,插件是独立进程 → 架构上要么把 PDF 导出上收到宿主,要么内嵌 headless chromium / 换 typst-pdf 之类的纯 Rust 方案) | 5–8d |
| **ebook-import** | UI + Rust 后端 | 是 | 🔴 重写 | ①`core-graphics` **无条件依赖**,编译即失败;②PDF 光栅化用 Quartz(`src/ocr/quartz.rs`)—— 需换 pdfium-render / mupdf;③Calibre 路径写死 `/Applications/calibre.app/...`(`calibre.rs:104`);④`calibre.rs:202,244` 用 `setsid`+`killpg` 管理子进程;⑤微信 OCR 服务是局域网 HTTP(`settings.rs:11`),这部分反而跨平台 | 5–8d |
| **pos-log** | 后端 only | 是 | ⛔ 建议不做 | 依赖宿主 `host.location.get`,后端是 CoreLocation(`plugin_runtime/location.rs`,已有非 macOS 桩返回不支持)。Windows 需接 `Windows.Devices.Geolocation` WinRT + 权限模型 | 5d 或放弃 |

**小计**:不含 pos-log,插件移植 20–35 人日。**若首发砍掉 md2pdf + ebook-import,降到 9–15 人日。**

### 5.1 插件发布流水线也要改

`scripts/release-plugins.sh` 全程 `for triple in aarch64-apple-darwin x86_64-apple-darwin`(第 145/302/328/373/396 行),并用 macOS `codesign --options runtime --timestamp` 签名(第 309-315、380-386 行)。Windows 需要:

- 增加 `x86_64-pc-windows-msvc` 循环(必须在 Windows 机器上跑)
- 二进制用 `signtool` 签名(与 macOS codesign 完全不同的凭据体系)
- minisign 检出签名逻辑跨平台可用,`.notemdpkg` zip 格式不变 —— **这部分不用改**

好消息:市场索引(`gen-plugin-index.mjs`)按 `<arch>.notemdpkg` 文件名自动发现 arch,**无需改代码**;Worker 侧下载 API 也是 arch 透传。memory 里 `project_plugin_index_merge_publish` 记的"merge 而非替换"机制同样适用,新增 Windows 包不会挤掉 macOS 包。

---

## 6. 发布与分发流水线

`scripts/release.sh` 是一条 **100% macOS-only** 的流水线:

| 步骤 | 现状 | Windows 需要 |
|---|---|---|
| 构建 | `--target *-apple-darwin` ×2 | 在 Windows 机上 `--target x86_64-pc-windows-msvc` |
| 签名 | `codesign` + Developer ID(`APPLE_TEAM_ID`) | `signtool` + **OV/EV 代码签名证书**(需采购,EV 约 $300-500/年;OV 便宜但 SmartScreen 信誉需时间积累) |
| 公证 | `notarytool`(`APPLE_ID`/`APPLE_PASSWORD`) | 无对应物;但**SmartScreen 会在证书信誉建立前警告用户**,这是新发布的 Windows 应用绕不开的冷启动问题 |
| 打包 | `.dmg` ×2 | `.exe`(NSIS)或 `.msi` |
| 架构自检 | `lipo` 断言(memory `feedback_release_per_arch_verify` 的教训) | 需要等价的 PE 架构断言(`dumpbin /headers` 或纯 Rust PE 解析) |
| updater | `latest.json` 只有 `darwin-aarch64` / `darwin-x86_64`(`release.sh:406-408`) | 增加 `windows-x86_64` key;签名机制(minisign)本身跨平台,**不用换密钥** |

**关键决策点**:memory 里 `feedback_no_github_deploy` 记录了"部署一律本地执行,不建 GH Actions 流水线"。Windows 构建**必须**在 Windows 上跑,这与"本地执行"原则冲突。三种选项:

1. **Parallels/UTM 虚拟机 + 手工触发**(最贴合现有习惯,推荐)—— 把 `release.sh` 拆成 `release-macos.sh` / `release-windows.ps1`,发版时两边各跑一次,最后合并 `latest.json` 与 GitHub Release
2. 物理 Windows 机 + SSH 触发
3. GitHub Actions windows-latest(与既有原则冲突,且要往 CI 塞签名凭据)

估算 8–13 人日(不含证书采购的日历时间)。

**对外物料同步**:README(中/英)第 8、122、143-148、172-174 行都写着 "Native macOS app"、`.dmg`、`aarch64-apple-darwin`;官网 `website/` 与 `llms.txt` / `llms-full.txt` 同样需要更新(否则 agent 读到的公共约定是错的)。

---

## 7. 测试与验证

| 层 | 现状 | Windows 影响 |
|---|---|---|
| 前端单测 | 166 个 `.test.ts`(vitest + happy-dom) | 绝大多数与平台无关,**可直接在 Windows 跑**;但 §3.4 的路径工具需要补一批新用例 |
| Rust 单测/集成测 | `src-tauri/tests/` 12 个文件 | `cli_startup_timing.rs:22`、`cli_builtin_integration.rs:26,93` 用 `std::os::unix::process::CommandExt`,**Windows 上编译失败**;`installer.rs`、`agents_sync/mod.rs`、`cli/install.rs` 的测试大量用 `unix_symlink` / `PermissionsExt`;fixture 用 `#!/bin/sh` 脚本冒充插件(`cli/router.rs:434`),Windows 需 `.bat`/`.exe` 替身 |
| GUI 实机验证 | memory `reference_dev_gui_verification` 那套 osascript 驱动 + `/tmp/mdeditor.log` + screencapture | **在 Windows 上完全不可用**。需要另建一套(PowerShell + UIAutomation,或就接受纯手动)。考虑到 memory `feedback_no_ui_automation_user_tests`(用户自己测 GUI),现实做法是:我出手动测试清单,用户在 Windows 上实机验证 |

**验证清单必须覆盖**(每一条都对应上文一个具体风险):打字/IME/选区/Cmd+A、保存与外部修改检测、vault git 同步(含无 git 环境)、托盘四态与通知、文件关联双击打开、深链接、自动更新、插件窗口打开与 RPC、多显示器 DPI 缩放。

---

## 8. 分期路线图

### 阶段 0:决策(0.5 天)

拍板 §0.3 三个问题 + 选定构建机方案 + 启动代码签名证书采购(有 1–3 周日历周期,**先启动**)。

### 阶段 1:主程序可用(17–25 人日)

1. 构建修复:`git2` 收窄为 iOS-only、toolchain 加 target、`bundle.targets` 加 nsis、生成品牌 `icon.ico`(1d)
2. **最小可跑版本上机**:能打字、能保存、能开窗 —— 尽早暴露 WebView2 未知量(2d)
3. 路径抽象层 + 39 处替换 + 测试(3–5d)
4. 菜单/快捷键:`CmdOrCtrl` 化、Windows 菜单编排、`isMacOS` 动态化、全局热键换键位(2–3d)
5. 子进程 `CREATE_NO_WINDOW` 统一封装 + git 缺失引导(1–2d)
6. 托盘角标替代方案 + 窗口退出语义 + deep-link 注册 + 文件关联收敛(2–3d)
7. CLAUDE.md 符号链接降级、theme_reveal 统一到 opener、默认应用注册的 Windows 实现或明确不做(1–2d)
8. 字体栈 + 深浅色 + DPI 缩放(1–2d)
9. WebView2 编辑器行为回归与修补(3–5d,方差最大)

**里程碑**:Windows 上能装、能开、能编辑保存、vault 能同步。此时即可考虑发一个"无插件" Windows beta。

### 阶段 2:插件运行时(8–12 人日)

1. **`plugin://` 协议适配 + token 化鉴权**(5–8d,含安全评审)—— 建议把 token 方案同时应用到 macOS
2. `current_arch_triple` 跨平台化 + 安装器权限位 cfg(1–2d)
3. CLI 配置目录跨平台 + `is_cli_mode` 重做 + Rust 测试的 unix 假设清理(2d)

### 阶段 3:插件移植(9–35 人日,取决于阶段 0 决策)

优先级:4 个纯 UI(2.5d)→ roam-import(1–2d)→ claude-agent(3–5d)→ openclaw(3–5d)→ [可选] md2pdf(5–8d)、ebook-import(5–8d)。pos-log 标记 macOS-only。

### 阶段 4:发布流水线(8–13 人日)

`release-windows.ps1`、signtool 集成、`latest.json` 双平台合并、PE 架构自检、`release-plugins.sh` 的 Windows arch、README/官网/`llms.txt` 更新。

---

## 9. 建议明确"不做"的清单

写进文档、UI 里显式降级,而不是留着让用户踩:

- **pos-log 插件**:Windows 不上架
- **CLI 符号链接安装**:Windows 上改为安装器写 PATH,或不提供
- **"设为默认应用"按钮**:Windows 上引导到系统设置页,不自己写注册表(写注册表在 Win10+ 会被系统拒绝/回滚)
- **CLAUDE.md 符号链接**:Windows 上降级为复制或不生成
- **打包 Git**:不打包,引导安装
- **60+ 文件关联**:Windows 上收敛到 5 个

---

## 10. 风险清单

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| WebView2 下 ProseMirror/IME 行为与 WKWebView 差异过大,编辑体验劣化 | 中 | **高**(直接砸招牌) | 阶段 1 第 2 步就上机验证,把未知量前置 |
| `plugin://` 单一 origin 的安全回退在评审时被否,需要更大改造 | 中 | 高 | 直接做 token 方案,不做"Windows 特例" |
| SmartScreen 冷启动警告导致早期用户流失 | **高** | 中 | 尽早采购证书累积信誉;首发页面显式说明 |
| 文件锁/杀毒软件导致保存与同步偶发失败 | 中 | 中 | 原子写重试 + 退避;引导加排除项 |
| 双平台发布流程走样(memory 里已有 v6.720.1 架构错发事故的教训) | 中 | 高 | PE 架构断言 + 独立 worktree 发布(沿用 `feedback_release_isolated_worktree`) |
| macOS 侧因跨平台重构而回归 | 中 | 高 | 路径工具、协议适配、token 鉴权三项都需 macOS 侧全量回归 |

---

## 附:核实清单

本报告的关键论断均已对照源码验证,主要引用点:

- `src-tauri/Cargo.toml:69-73`(git2 vendored-openssl)、`rust-toolchain.toml`
- `src-tauri/tauri.conf.json`(bundle targets / icon / updater.windows)
- `src-tauri/src/plugin_runtime/protocol.rs:1-7,80,158,217-223,370,386`(plugin:// 鉴权)
- `src-tauri/src/plugin_runtime/windows.rs:193-199`(CustomProtocol URL)
- `src-tauri/src/plugin_runtime/discovery.rs:15-18`(arch triple)
- `src-tauri/src/plugin_runtime/process.rs:60-88`(stdio IPC,跨平台)
- `src-tauri/src/cli/mod.rs:34-70`(配置目录 / CLI 模式判定)
- `src-tauri/src/lib.rs:210-228,275-380,652,855,921,1008,1203,1957-2112`
- `src-tauri/src/vault_sync/git_ops.rs:9-30`、`git_history/mod.rs:116`、`okf/mod.rs:37`
- `src-tauri/src/agents_sync/mod.rs:59-78`、`themes/commands.rs:19-31`
- 上游:`wry-0.55.1/src/lib.rs:1172,1190`、`tauri-2.11.2/src/webview/mod.rs:2388`、`tauri-2.11.2/src/tray/mod.rs:532-538`
- 插件:各 `plugins-src/*/manifest.v2.json` 与 `backend/Cargo.toml`,详见 §5 表格内引用
- 脚本:`scripts/release.sh:388-408`、`scripts/release-plugins.sh:145,302-315,373-396`、`scripts/gen-plugin-index.mjs:65-88`
