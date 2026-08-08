# note.md PC 版(Windows / Linux)深度改造方案

> 日期:2026-08-08 · 代码基线:`main` @ 3dd75ff(v6.806.5)
> 前置文档:`docs/2026-08-08-windows-port-analysis.md`(Windows 兼容性分析,含全部问题点的 `文件:行号` 索引)
> 性质:**改造方案**(怎么改),不再重复"有什么问题"(那是前置文档的事)。所有平台事实均已对照 tauri 2.11.2 / wry 0.55.1 源码核实。

---

## 0. 方案总纲

### 0.1 一句话

**不做"Windows 分支"和"Linux 分支",做一次性的"平台端口层"改造:把 13 处 macOS 假设收编成两个抽象层(Rust `platform` 模块 + 前端 `paths`/`keymap` 模块),把插件鉴权从 origin 改成 token(三端统一、顺带加固 macOS),然后每个平台只剩打包与桩实现的活。**

### 0.2 关键新事实(相对 Windows 分析报告)

Linux 的加入**没有把工作量翻倍,反而摊薄了**,因为:

| 议题 | Windows | Linux | 结论 |
|---|---|---|---|
| `plugin://` 协议 | ❌ 塌成 `http://plugin.localhost` 单一 origin(wry `src/lib.rs:1172`) | ✅ webkitgtk **原生注册 scheme**(wry `src/webkitgtk/mod.rs:367`),origin 保持 `plugin://<id>` | origin 塌缩是 Windows 独有;但 token 方案仍应三端统一(见 §3) |
| Unix API(`symlink`/`PermissionsExt`/`setsid`/`killpg`/`UnixStream`) | ❌ 全部需替代 | ✅ **原样可用** | claude-agent / openclaw / roam-import 在 Linux 上近乎零改 |
| 外部 `git` | 默认没有 | 几乎必有 | Linux 免引导 |
| 托盘 `set_tooltip` | ✅ | ❌ Unsupported(tauri `tray/mod.rs:522`) | 需 capability 矩阵 |
| 托盘 `set_title`(角标) | ❌ Unsupported(`tray/mod.rs:537`) | ⚠️ 仅在有 icon 时显示(`:532`) | 同上 |
| 全局快捷键 | ✅ | ⚠️ **Wayland 下不可用**(X11 才行) | 需运行时探测 + 降级 |
| 桌面三件套依赖(single-instance / window-state / global-shortcut) | `Cargo.toml` 已按 `cfg(any(macos,linux,windows))` 声明 | 同左 | **无需改** —— 依赖侧早就是三端形态 |
| CLI 符号链接安装 | ❌ | ✅ `~/.local/bin` 免提权即可 | Linux 反而比 macOS 简单(不用 osascript) |

### 0.3 工作量总盘

| 块 | 内容 | 估算 |
|---|---|---|
| A. 共享改造(一次性,三端受益) | 平台端口层、token 鉴权、路径工具、菜单模型、arch triple、CLI 目录 | 22–30 人日 |
| B. Windows 增量 | WebView2 适配、打包/签名、子进程隐藏、注册表类桩 | 15–22 人日 |
| C. Linux 增量 | webkitgtk 验证、AppImage/deb 打包、appindicator、Wayland 降级 | 10–16 人日 |
| D. 插件移植(不含 md2pdf/ebook-import 重写) | 见 §8 矩阵 | 8–14 人日 |
| E. 可选:md2pdf + ebook-import 跨平台重写 | 见 §8.2 | 10–16 人日 |
| **合计(不含 E)** | | **55–82 人日** |

对比:单做 Windows 是 53–85 人日 —— **Linux 的边际成本只有 10–16 人日**,因为共享改造本来就要做。

### 0.4 三条设计原则(做决策时以此裁决)

1. **Capability,不是 if-else。** 平台差异全部收进端口层的能力声明(`supports_tray_badge()`、`supports_global_shortcut()`),业务代码只问"能不能",不问"是不是 Windows"。理由:第三个平台(Linux)已经证明了两分支 if-else 会变三分支,端口层不会。
2. **统一实现优于平台特例。** 凡是"为 Windows 做的替代方案在 macOS 上也更好"的(token 鉴权、opener 插件 reveal、spawn 封装),直接三端统一,删掉 macOS 特例。这样 macOS 主线永远在测跨平台代码路径,不会出现"Windows 分支半年没人跑"的腐烂。
3. **file-over-app 延伸到平台层。** vault 内容、`.notemd/` 配置、插件包格式、市场索引在三端逐字节同构 —— 一个 vault 在 mac/Win/Linux 间 git 同步必须无感。任何平台差异只允许存在于"应用如何呈现",不允许写进磁盘格式。(唯一例外:`settings.json` 里的机器本地路径,本来就是 per-machine 的。)

---

## 1. 总体架构:平台端口层

### 1.1 Rust 侧:`src-tauri/src/platform/` 模块

新建模块,把散落的 13 处 macOS 假设收编。**不用 trait 对象,用 cfg 分文件 + 统一签名的自由函数** —— 与现有代码风格一致(`agents_sync/mod.rs:59-78` 已经是这个形态),避免为 3 个平台引入动态分发。

```
src-tauri/src/platform/
  mod.rs        // 统一 re-export + capability 查询
  macos.rs
  windows.rs
  linux.rs
  common.rs     // 三端共享的实现(spawn_hidden 的非 Windows 版等)
```

**API 清单(每条标注现有代码的迁移源)**:

```rust
// ── 路径 ──
/// CLI 与 GUI 必须同源:内部一律走 dirs::config_dir()/data_dir() + BUNDLE_ID。
/// 迁移自 cli/mod.rs:34-40(硬编码 ~/Library/Application Support)。
/// ⚠ macOS 上 dirs::config_dir() == "~/Library/Application Support" —— 与现状等价,零迁移;
///   Linux → ~/.config/net.notemd.app;Windows → %APPDATA%\net.notemd.app。
pub fn config_dir() -> PathBuf;
pub fn data_dir() -> PathBuf;      // 迁移自 cli/runner.rs:101、cli/builtin.rs:456
/// "~" 缩写:macOS/Linux 用 $HOME,Windows 用 %USERPROFILE%。迁移自 lib.rs:652。
pub fn abbreviate_home(path: &str) -> String;

// ── 进程 ──
/// 所有 Command 出口的唯一漏斗。Windows 加 CREATE_NO_WINDOW(0x08000000);
/// 迁移点:vault_sync/git_ops.rs:12,21、git_history/mod.rs:116、okf/mod.rs:37、
/// plugin_runtime/process.rs:67、themes/commands.rs:24。
pub fn command(program: impl AsRef<OsStr>) -> std::process::Command;
pub fn tokio_command(program: impl AsRef<OsStr>) -> tokio::process::Command;

// ── 文件系统 ──
/// 迁移自 agents_sync/mod.rs:61-78。Linux 直通 unix::symlink;
/// Windows 尝试 symlink_file,失败(无开发者模式)→ Fallback::CopyWatch。
pub fn symlink_or_fallback(target: &Path, link: &Path) -> Result<LinkKind, io::Error>;
/// 原子替换 + Windows 侧 "file in use" 重试(3 次,10/50/250ms 退避)。
/// 收编全仓 9 处 fs::rename / NamedTempFile::persist。
pub fn atomic_replace(tmp: &Path, dst: &Path) -> io::Result<()>;
/// 可执行位:unix 用 PermissionsExt;Windows no-op(按扩展名)。
/// 迁移自 plugin_runtime/installer.rs:201-203。
pub fn mark_executable(path: &Path) -> io::Result<()>;

// ── 系统集成 ──
/// 三端统一走 opener 插件的 reveal(前端已在用:sotvault.svelte.ts:148)。
/// 删掉 themes/commands.rs:22-31 的 open/Err 分叉。
pub fn reveal_in_file_manager(app: &AppHandle, path: &Path) -> Result<(), String>;
/// 默认应用注册:macOS = LaunchServices(迁移自 lib.rs:275-380);
/// Windows/Linux = Unsupported{hint}(引导到系统设置 / xdg 由打包器负责)。
pub fn register_default_app(exts: &[String]) -> Vec<ExtResult>;

// ── Capability 查询(托盘、快捷键等按此降级)──
pub struct Caps {
    pub tray_tooltip: bool,     // mac✅ win✅ linux❌
    pub tray_title_badge: bool, // mac✅ win❌ linux⚠(有 icon 才显示)
    pub global_shortcut: bool,  // mac✅ win✅ linux=运行时探测(Wayland ❌)
    pub cli_symlink_install: bool, // mac✅ linux✅ win❌
    pub app_menu: bool,         // mac✅(独立应用菜单) win/linux❌(并入 File/Help)
}
pub fn caps() -> &'static Caps;   // linux 的 global_shortcut 在 setup 时探测 XDG_SESSION_TYPE
```

**迁移纪律**:改造完成后,在 CI/单测里加一条 lint(`scripts/` 下一个 grep 脚本即可):`src-tauri/src` 中除 `platform/` 与 `vault_ios/` 外,**禁止出现 `std::process::Command::new`、`std::os::unix`、`std::os::windows`、`Library/Application Support` 字样**。这是防止端口层被绕过的唯一长效手段。

### 1.2 前端侧:`src/lib/paths.ts` + `src/lib/keymap.ts`

**`paths.ts`** —— 收编 39 处 / 30 文件的 POSIX 假设(清单见前置文档 §3.4):

```ts
// 内部规范形:一律正斜杠。只在【进入系统 API】的边界转换。
export function norm(p: string): string        // 'C:\a\b' → 'C:/a/b'
export function join(...seg: string[]): string
export function dirname(p: string): string     // 迁移 folder-view.svelte.ts:29-40 parentDir
export function basename(p: string): string
export function relative(root: string, p: string): string | null  // 迁移 sotvault-logic.ts:81,89
export function isAbsolute(p: string): boolean // 兼容 '/x' 与 'C:/x' 与 '\\server\share'
export function abbreviateHome(p: string, home: string): string   // 迁移 recent-merge.ts:114-118
```

约定:**所有从 Tauri 命令返回的路径在 store 入口处立即 `norm()`**(tabs、folder-view、recent、sotvault 四个 store 的 setter),此后前端内部世界只有正斜杠;`invoke()` 传回 Rust 时不需要反转换(Windows API 接受正斜杠)。这样 39 处调用点大多数只需把 `split('/')` 换成 `paths.basename` 等,不需要每处理解两种分隔符。

**`keymap.ts`** —— 快捷键的**单一事实源**(现状:Rust 菜单 19 处 `Cmd+`、前端 19 处 `⌘` 展示、moraya-core 收 `isMacOS`、manifest 里插件热键,四处各写各的):

```ts
export type Mod = 'Mod' | 'Shift' | 'Alt' | 'ModAlt'   // Mod = ⌘/Ctrl
export function toAccelerator(spec: string): string      // 'Mod+S' → 'CmdOrCtrl+S'(Tauri 通吃三端)
export function display(spec: string, platform: Platform): string // '⌘S' / 'Ctrl+S'
export function matches(e: KeyboardEvent, spec: string, platform: Platform): boolean
```

`lib/outline/shortcuts.ts:45-54` 的 `displayShortcut` 已经是这个思路的雏形,升格推广。**两处硬编码 `isMacOS: true` 必须动态化**:`src/lib/editor-bridge.ts:13`、`src/editor-kit/rich.ts:73` 改为从 `platform()` 注入(moraya-core 的 `editor-props-plugin.ts:161,592,622` 按它决定键位与装饰,不改这两处 Windows/Linux 键位直接错乱)。

---

## 2. 决策清单(动工前拍板)

| # | 决策 | 建议 | 理由 |
|---|---|---|---|
| D1 | 首发范围 | **两阶段**:PC-beta = 编辑器 + vault 同步 + 纯 UI 插件;PC-1.0 = 全插件市场 | 把 WebView2/webkitgtk 编辑器行为这个最大方差前置到 beta 实测 |
| D2 | Linux 打包 | **AppImage(主,可自动更新)+ .deb(次)**;rpm 后置 | Tauri updater 在 Linux 只支持 AppImage 就地更新 |
| D3 | Linux 最低线 | webkit2gtk-4.1(libsoup3),即 Ubuntu 22.04+ / Debian 12+ / Fedora 36+ | wry 0.55 的硬依赖(`Cargo.toml:222-231`),没有选择余地 |
| D4 | md2pdf / ebook-import | PC-1.0 不含,标注 macOS-only;跨平台重写(§8.2)作为独立后续项目 | 纯重写,不应阻塞发行 |
| D5 | pos-log | 永久 macOS-only(manifest 加 `platforms` 字段) | 成本/价值不匹配 |
| D6 | 构建机 | Windows:Parallels VM 本地跑 `release-windows.ps1`;Linux:**Docker(ubuntu 22.04 镜像)在 mac 上跑** | 贴合"部署本地执行"原则(`feedback_no_github_deploy`);Linux 交叉构建用容器即可,不需要真机 |
| D7 | Windows 代码签名 | 立即采购 OV 证书(Azure Trusted Signing 亦可) | 1–3 周日历周期,是关键路径 |
| D8 | token 鉴权是否回灌 macOS | **是**,三端统一 | 原则 2;并消除 macOS 上 Origin 伪造面 |

---

## 3. 核心改造①:插件 UI 协议与鉴权(token 化)

**背景**(细节见前置文档 §3.1):Windows 上 wry 把 `plugin://<id>/x` 重写为 `http://plugin.localhost/<id>/x`,插件 id 从 host 掉进 path,且所有插件共享一个 origin → `protocol.rs:223` 的 Origin 鉴权与 `custom-editor-msg.ts` 的 targetOrigin 校验双双失效。Linux/macOS 不受影响,但方案按原则 2 三端统一。

### 3.1 URL 抽象

```rust
// platform/mod.rs
/// 构造插件窗口加载 URL。macOS/Linux: plugin://<id>/<entry>
/// Windows: http://plugin.localhost/<id>/<entry>(wry 的重写形,直接按它构造)
pub fn plugin_url(id: &str, entry: &str) -> Url;

/// 从协议请求中解出 (plugin_id, path)。
/// macOS/Linux: host = id;Windows: path 第一段 = id。
pub fn parse_plugin_request(uri: &Uri) -> Option<(String, String)>;
```

替换点:`plugin_runtime/windows.rs:195`(构造)、`protocol.rs:370-386`(解析)、`protocol.rs:158` `__host__` 前缀判定(改为相对 path 判定)。前端对称:`CustomEditorIframe.svelte:11-12`、`editor-kit/main.ts:114`、`power-mode/presets.ts:34-39` 改走 `pluginBaseUrl(id)` helper(由宿主经 window 注入,不在前端猜平台)。

### 3.2 Token 鉴权协议(替代 Origin 判身份)

**设计**:

1. 宿主为每个"活的插件窗口"生成 128-bit 随机 token,存 `HashMap<token, plugin_id>`(挂在 `plugin_runtime::STATE`,窗口销毁即回收 —— `windows.rs` 已有 `WindowEvent::Destroyed` 钩子可挂)。
2. 注入方式:`protocol.rs` 服务 `text/html` 时已有注入管线(`windows.rs:34` 注释:"protocol ALSO injects it into every served text/html response")——**沿用同一管线**,把 token 以 `<meta name="notemd-token">` 或 `window.__NOTEMD_TOKEN__` 注入。token 与窗口一次性绑定,刷新页面时协议层按窗口 label 重发同一 token。
3. `__rpc__` POST 必须带 `X-Notemd-Token` 头;宿主查表得到 plugin_id,**Origin 头降级为纵深防御**(macOS/Linux 上仍校验一致性,Windows 上只校验是 `plugin.localhost`)。
4. iframe 自定义编辑器(custom-editor-msg):postMessage 信封加 token 字段,父窗校验 token↔plugin_id;targetOrigin 在 Windows 上退化为 `http://plugin.localhost` 是可接受的(信封 token 补足了身份)。
5. **CSP 补强(Windows 专属)**:单一 origin 下 `'self'` 覆盖所有插件,所以 Windows 上把 `protocol.rs:80` 的 CSP 从 `'self'` 收紧为显式 `http://plugin.localhost` + 每响应 nonce,并在协议层按"请求路径的 id 段 ≠ token 的 id"直接 404,防止插件 A 的页面 fetch 插件 B 的静态资产。

**测试**:`protocol.rs:727-933` 已有整套鉴权单测(伪 Origin、跨插件调用),全部改写为 token 断言 + 三端 URL 形态参数化;新增"窗口销毁后 token 失效"、"token 跨插件混用被拒"两条。

**估算**:5–8 人日(含安全自审)。这是整个方案里唯一需要**设计评审**的改动。

---

## 4. 核心改造②:菜单、快捷键、托盘

### 4.1 声明式菜单模型

现状:`lib.rs:1957-2112` 用 builder 手写 macOS 形态菜单(独立应用菜单 + `Cmd+` ×19)。改为数据驱动:

```rust
// menu_model.rs(纯数据,可单测)
struct MenuSpec { id: &'static str, label_key: &'static str, accel: Option<&'static str> /* 'Mod+S' 形 */ , .. }
fn desktop_menu(platform: Platform, locale: &str) -> Vec<SubmenuSpec>;
```

平台差异收在 `desktop_menu` 一处:
- macOS:现状不变(应用菜单含 About/Hide/Preferences/Quit)。
- Windows/Linux:无应用菜单;`Preferences` → File 菜单尾部("Options…",Ctrl+,);`Quit`(Alt+F4/Ctrl+Q)→ File 尾部;`About` → Help;`app.hide` 不生成。
- accel 统一写 `Mod+` 形,渲染时经 `toAccelerator` 转 `CmdOrCtrl+`(Tauri 三端通吃,macOS 侧行为不变)。
- 现有的 rebuild-resets-enabled 补丁(`menu-rebuilt` resync,memory `reference_menu_rebuild_resets_enabled`)原样保留 —— 模型化不改变 set_menu 的运行时行为。

全局热键:`lib.rs:1203` 的 `Cmd+Ctrl+M/N` 保持 macOS 不变;Windows/Linux 注册 `Ctrl+Alt+M/N`;Linux 上 `caps().global_shortcut == false`(Wayland)时**跳过注册并在托盘菜单项上去掉快捷键展示**,功能仍可从托盘点击触达。插件热键(`idea-spark` 的 `Cmd+Ctrl+I`)同理:宿主解析器(`lib.rs:60-65`)已做别名归一,补一条 "Cmd→Ctrl+Alt on pc" 的映射规则即可,**插件 manifest 不用改**。

### 4.2 托盘/通知的 capability 降级

统一通知基建(`notifications.rs`)的呈现层按 `caps()` 分派:

| 能力 | macOS | Windows | Linux | 降级动作 |
|---|---|---|---|---|
| 角标数字 | `set_title(n)` | ❌ | ⚠ | **预渲染 badge 图标**:`tray-icon-badge-{1..9,9plus}.png` × 四态色,`set_icon` 切换(Windows 主路径;Linux 同用,因为 title 展示不稳定) |
| tooltip | ✅ | ✅ | ❌ | Linux 把"last synced…"合并进托盘菜单首行(已有 status_item 机制,`lib.rs:893`,零新代码) |
| 四态色点 | ✅ | ✅ | ✅ | 不变;但 Linux 需 `libayatana-appindicator` 运行依赖,deb 声明 Depends,AppImage 打进包 |

badge 图标由 `scripts/gen-tray-badges.mjs` 从现有 `tray-icon-*.png` 叠加数字生成(一次性脚本,约 40 张,勿手画)。

---

## 5. 核心改造③:CLI 与文件系统

### 5.1 CLI

- `cli/mod.rs:34-40` `resolve_config_dir` → `platform::config_dir()`(macOS 结果逐字节不变,`cli/runner.rs:446` 的等价性测试改为三端参数化)。
- `is_cli_mode`(`cli/mod.rs:52-70`):判据从"路径含 `.app/Contents/MacOS/`"改为**显式意图**——`--cli` flag、`NOTEMD_CLI=1` 环境变量、或 argv[0] 基名 ∈ {`notemd`,`mdedit`} 且**不等于 GUI 可执行名**。Windows 上 GUI 是 `notemd.exe`:为避免歧义,CLI 入口在 Windows 上**只认 `--cli`**(NSIS 装一个 `notemd-cli.cmd` 垫片:`@"...\notemd.exe" --cli %*`,并写入 PATH)。Linux 上 `.deb`/AppImage 的 desktop 启动是绝对路径 `/usr/bin/notemd` —— 需要新判据:**由打包器给 GUI 桌面入口加 `--gui` flag**,显式压过 CLI 判定(比路径嗅探可靠)。
- CLI 安装(`cli/install.rs`):macOS 保持 osascript;Linux 装到 `~/.local/bin`(免提权,不需要任何对话框);Windows `caps().cli_symlink_install=false`,设置页该项隐藏。

### 5.2 文件系统与同步

- **原子写**:9 处 rename 全部过 `platform::atomic_replace`(带 Windows 重试)。重点回归 `vault_sync` 的 commit-first 流(memory `reference_vault_sync_no_worktree_revert`)—— Windows 的锁语义使"编辑器持有 + git 操作"窗口更宽,压测项见 §9。
- **git 探测**:`git_ops.rs:9-18` 的 `version()` 保持;Windows 上探测失败时,`vault_sync` 状态进入现有的"git unavailable"黄灯,托盘菜单项加"下载 Git for Windows"动作(`opener` 打开下载页)。**不打包 MinGit**(D 决策:体积卖点)。
- **watcher**:`notify` 的 feature `macos_fsevent` 不阻断其他平台(后端按 target 自动选择);但需在 Windows(ReadDirectoryChangesW)与 Linux(inotify,注意 `fs.inotify.max_user_watches` 上限,大 vault 需在诊断日志里提示)各做一轮事件语义实测:重命名对、编辑器保存的 write-then-rename 序列、`.git/` 目录风暴过滤。
- **Defender/杀软**:首启检测 vault 在系统盘且未排除时,onboarding 提示(仅提示,不自动改)。

### 5.3 构建修复(P0,半天)

`git2` 收窄为 iOS-only(桌面端零使用,`vendored-openssl` 在 windows-msvc 需 Perl+NASM,Linux 容器里也纯属拖累);`rust-toolchain.toml` 加 `x86_64-pc-windows-msvc`、`x86_64-unknown-linux-gnu`;`ebook-import/backend/Cargo.toml:24-26` 的 `core-graphics` 三件套挪进 `[target.'cfg(target_os="macos")']`(哪怕 D4 决定不移植,也要让 workspace 在 PC 上能整体编译)。

---

## 6. Windows 专属工程

(问题背景见前置文档,此处只列改法与验收)

1. **打包**:`tauri.conf.json` 加 per-platform 覆盖文件 `tauri.windows.conf.json`(Tauri 原生支持 merge):`targets:["nsis"]`、`icon.ico`(用 `gen-macos-icon.sh` 的源图重制 16/32/48/256 多尺寸,现有 ico 是脚手架默认图标)、`windows.webviewInstallMode: downloadBootstrapper`、文件关联收敛为 `md/markdown/mdown/mkd/txt`。
2. **签名**:signtool + OV 证书(D7);`release-windows.ps1` 内嵌 PE 架构断言(读 PE 头 Machine 字段,对应 memory `feedback_release_per_arch_verify` 的 lipo 断言)。
3. **子进程**:`platform::command()` 全量替换后,验收方式 = 开着 vault 自动同步观察 10 分钟无控制台闪现。
4. **窗口语义**:`should_prevent_exit`(`lib.rs:210-228`)接设置项 `closeToTray`(Windows/Linux 默认 **false**=关窗即退,macOS 默认 true 维持现状);托盘退出路径回归 memory `reference_tauri_exit_prevent` 的坑。
5. **deep-link**:`tauri.conf.json` 补 `plugins.deep-link.desktop.schemes`(Windows 写注册表 / Linux 进 .desktop,Tauri 打包器自动处理;macOS 现状 Info.plist 不动)。
6. **updater**:`latest.json` 增加 `windows-x86_64` 平台键;minisign 密钥不变;`installMode: passive` 已配好。

## 7. Linux 专属工程

1. **打包**:`tauri.linux.conf.json`:`targets:["appimage","deb"]`;deb `depends` 声明 `libwebkit2gtk-4.1-0, libayatana-appindicator3-1, git`(git 声明为 Recommends 而非 Depends);AppImage 把 appindicator 打进去。文件关联/URL scheme 由 `.desktop` MIME 条目承载(Tauri 从 `fileAssociations` 自动生成,但同样建议收敛清单)。
2. **构建容器**:`scripts/docker/linux-build.Dockerfile`(ubuntu 22.04 + rust + webkit2gtk-4.1-dev + tauri-cli),`release-linux.sh` 在 mac 上 `docker run` 完成构建与签名(minisign 部分),产物回传宿主机上传。这是 D6 的落地,让 Linux 发版不需要任何真机。
3. **Wayland**:setup 时读 `XDG_SESSION_TYPE`,`wayland` → `caps().global_shortcut=false`(§4.1 降级);已知 webkitgtk+NVIDIA 的渲染问题,诊断文档写明 `WEBKIT_DISABLE_DMABUF_RENDERER=1` 逃生阀,不默认设置。
4. **单实例**:tauri-plugin-single-instance Linux 走 DBus,与 memory `feedback_gui_verify_isolation` 的隔离手法(改 identifier)天然兼容,无需处理。
5. **updater**:AppImage 更新键 `linux-x86_64`;deb 用户不走自动更新(latest.json 不含,应用内提示"从官网下载新版 deb")—— 在设置页明示。
6. **字体**:app.css 字体栈补 `'Segoe UI'`(Win)与 `'Ubuntu','Cantarell','Noto Sans CJK SC'`(Linux);等宽栈三处只有 `Menlo` 的(`editor-base.css:239,303,619`)统一为 `ui-monospace,'SF Mono',Menlo,Consolas,'DejaVu Sans Mono',monospace`。

---

## 8. 插件改造矩阵(三平台)

### 8.1 逐插件方案

| 插件 | Win | Linux | 改造内容 | 估算 |
|---|---|---|---|---|
| decision-log / weekly-review / power-mode | ✅ | ✅ | 零改(随 §3 协议适配自动获得);power-mode 需 WebView2/webkitgtk 特效性能实测 | 1d(验证) |
| idea-spark | ✅ | ✅ | 热键随 §4.1 宿主侧映射,manifest 不改 | 0.5d |
| roam-import | 🟡 | ✅ | `discover.rs:20-21` 路径候选表按 OS 分支(Win: `%LOCALAPPDATA%\roam` + `where`);`PermissionsExt` 三处走 `platform::mark_executable` 同款判定;Linux 零改 | 1–2d |
| claude-agent | 🟠 | ✅ | Linux:`setsid/killpg/kill(pid,0)` 原样可用,只补 `discover.rs` 的 Linux 路径候选(`~/.local/bin` 等)。Win:进程组管理抽 `sdk::proc` 模块 —— unix 用现状,Windows 用 Job Object(`CREATE_SUSPENDED`+Assign+Resume)、探活用 OpenProcess;`claude` 发现走 `where` + `%APPDATA%\npm` | Linux 1d;Win 3–4d |
| openclaw-chat | 🟠 | ✅ | Linux:UDS 原样可用,零改。Win:`uds_client.rs` 抽传输 trait,Windows 实现**优先命名管道**(openclaw gateway 若不支持,则退 TCP loopback + 现有 HMAC 层作认证 —— 取决于 openclaw 端能力,需先确认) | Linux 0.5d;Win 3–5d |
| md2pdf | ⛔→E | ⛔→E | 见 §8.2 | — |
| ebook-import | ⛔→E | ⛔→E | 见 §8.2(但 Cargo target 化必须先做,§5.3) | — |
| pos-log | ⛔ | ⛔ | manifest 加 `platforms:["macos"]`(见 §8.3) | 0.5d |

### 8.2 (可选项目 E)md2pdf / ebook-import 跨平台重写要点

- **md2pdf**:放弃"每平台一个 webview 打印后端"的路线(WebView2 的 PrintToPdf 要在插件进程里起 webview,复杂度高)。推荐改为 **Typst 或 chromium-headless 之外的纯 Rust 路线**:markdown → HTML 已有,HTML→PDF 换 `weasyprint` 级别的实现在 Rust 生态里最现实的是 **typst-pdf(重排版)** 或维持 macOS-only、PC 上提供"导出 HTML + 系统打印"降级。建议后者进 PC-1.0,前者做独立项目。
- **ebook-import**:PDF 光栅化从 Quartz 换 **pdfium-render**(pdfium 动态库随插件包分发,per-arch `.notemdpkg` 机制正好承载,回到当年 5.28M 的体积——这是当初换 CoreGraphics 省掉的,跨平台就得付回来);Calibre 候选路径加 `C:\Program Files\Calibre2\` 与 `/usr/bin/ebook-convert`;`setsid/killpg` 走 §8.1 claude-agent 同款 `sdk::proc`;微信/百度 OCR 是 HTTP,天然跨平台。

### 8.3 协议与市场的配套改动

1. **manifest v2 加 `platforms` 字段**(可选,缺省 = 全平台):`["macos","windows","linux"]`。消费侧宽容(未知字段忽略,老宿主不受影响);`discovery.rs` 与市场 UI 按当前平台过滤。`plugin-protocol` schema + `gen-plugin-protocol.sh` 再生成。
2. **arch triple**:`discovery.rs:15-18` 改为 OS×ARCH 矩阵(`x86_64-pc-windows-msvc`、`x86_64-unknown-linux-gnu`、…)。市场索引与 Worker 是 arch-keyed 透传(`gen-plugin-index.mjs:65-76`),**零改动**;`release-plugins.sh` 的 triple 循环扩为按插件 `platforms` × 支持 triple 展开,Windows 二进制在 VM 里构建后回传签名(minisign 签名本身在 mac 上做,统一密钥)。
3. **SDK**:`notemd-plugin-sdk` 无 unix 依赖(纯 stdio,已核实),新增上述 `sdk::proc` 进程组模块供 claude-agent/ebook-import 复用。

---

## 9. 测试与验收

1. **单测跨平台化**:`cli_startup_timing.rs:22`、`cli_builtin_integration.rs:26,93` 的 `unix::CommandExt` 加 cfg;fixture 假插件从 `#!/bin/sh` 脚本改为**编译一个 10 行的 rust 测试二进制**(三端同源,比 .bat/.sh 双份可靠);`agents_sync`/`installer`/`install` 的 symlink 测试按 `LinkKind` 参数化。
2. **CI 形态**:遵守"不建部署流水线"原则,但**编译+单测不是部署** —— 建议加一条 GH Actions 仅跑 `cargo test`(windows-latest / ubuntu-22.04)+ `pnpm test`,不接触任何凭据。若仍不接受,则退化为发版脚本里远程触发 VM/容器跑测试。此项需用户拍板(与 memory `feedback_no_github_deploy` 的边界确认)。
3. **实机验证清单**(用户执行,循 `feedback_no_ui_automation_user_tests`):打字/IME(微软拼音、fcitx5)/选区/Ctrl+A、保存与外部修改检测、无 git 环境的黄灯与引导、托盘 badge 四态、关窗语义与 closeToTray、文件关联双击、deep-link、自动更新端到端(NSIS passive / AppImage)、插件窗口开启+RPC+窗口销毁后 token 失效、HiDPI(150%/200% 缩放)、Wayland 与 X11 各一轮。
4. **发布前架构断言**:PE Machine 字段(Win)/ ELF e_machine(Linux)写进各自 release 脚本,对应 lipo 断言的教训。

---

## 10. 分期与里程碑

```
M0  决策周(D1–D8)+ 证书采购启动 + Windows VM / Linux 容器就绪          0.5d + 日历等待
M1  三端可编译:git2 收窄、toolchain、ebook-import target 化、
    tauri.{windows,linux}.conf.json、图标                                  2–3d
M2  最小可跑:Windows + Linux 上"能开窗、能打字、能保存"                    3–5d   ← 方差探针,最早暴露 webview 差异
M3  平台端口层落地(Rust platform/ + 前端 paths/keymap)+ 迁移 + lint 门禁  8–12d
M4  菜单/快捷键/托盘/窗口语义/CLI(§4、§5.1)                              6–9d
M5  token 鉴权 + plugin_url 抽象(§3,含评审)                             5–8d
M6  同步与文件系统压测(§5.2)+ 实机验证清单第一轮                          3–5d
M7  插件移植(§8.1,不含 E)+ release-plugins 扩 triple                    8–14d
M8  发布工程:release-windows.ps1 / release-linux.sh(容器)/
    latest.json 三平台合并 / README+官网+llms.txt                          6–9d
──────────────────────────────────────────────────────────
    PC-beta(无插件市场)= M1–M4 + M6 + M8 裁剪版 ≈ 25–35d
    PC-1.0(含插件)   = 全部        ≈ 55–82d
```

依赖关系:M5 不阻塞 M2–M4(beta 可以无插件);M7 依赖 M5;证书(D7)只阻塞 M8 的对外发布,不阻塞开发。

---

## 11. 明确不做(写进 FAQ/设置页,显式降级)

- pos-log:macOS-only(manifest `platforms`)
- md2pdf / ebook-import:PC-1.0 不含(项目 E 另行立项)
- Windows CLI 符号链接安装(用 NSIS 写 PATH 垫片替代)
- "设为默认应用"按钮在 Windows/Linux:跳转系统设置/由打包器登记,不自写注册表
- 打包 Git;打包 MinGit
- Windows 上 60+ 文件关联(收敛为 5)
- Linux deb 的应用内自动更新(提示手动下载)
- Wayland 下的全局快捷键(降级到托盘触达)

## 12. 风险台账(相对前置文档的增量)

| 风险 | 概率 | 缓解 |
|---|---|---|
| webkitgtk 的 ProseMirror/IME 行为是**第三套**方言(≠WebKit macOS ≠WebView2),M2 才能知道深浅 | 中 | M2 前置;Linux IME 重点测 fcitx5/ibus 两家 |
| openclaw Windows 传输方案受制于 gateway 端能力 | 中 | M0 期先向 openclaw 端确认命名管道/TCP 支持,再定 §8.1 路线 |
| 平台端口层迁移引发 macOS 回归 | 中 | lint 门禁 + macOS 全量回归排进 M3/M4 的验收;`config_dir` 等选型已保证 macOS 结果逐字节不变 |
| token 化改动触碰 `protocol.rs` 这个安全边界 | 低-中 | §3.2 单测清单 + 独立评审;Origin 校验保留为纵深 |
| Docker 构建的 AppImage 在真实发行版上的 glibc/appindicator 兼容 | 中 | 容器钉在 ubuntu 22.04(=最低线 glibc);发布前在 Fedora/Arch 各冒烟一次 |
| 双倍发布矩阵(3 OS × 2 arch mac + 1 arch win/linux)出错面扩大 | 中 | 架构断言三端齐;沿用独立 worktree 发布纪律;latest.json 生成收敛到单脚本合并三平台产物 |
