# note.md 插件 v2 开发规范

> 权威参考。所有结论均已对照源码核实,带 `文件:行号` 引用。改动接口时请同步更新本文。
> 最后核对基线:2026-07-21(feat/attention-intervals)。

## 0. TL;DR — 三种插件形态

| 形态 | 例子 | 有 `binary` | 有 `ui` | 典型用途 |
|---|---|---|---|---|
| **纯前端** | `roam-import` | ❌ | ✅ | 开一个独立窗口做交互,读写 vault |
| **后端 + 前端** | `openclaw`、`claude-agent` | ✅ | ✅ | 后台进程(网络/计算)+ 流式 UI |
| **纯后端** | `pos-log` | ✅ | ❌ | 常驻后台采集,无界面 |

规则(`plugin-protocol/src/lib.rs:183-188`):
- 必须提供 `binary` 和/或 `ui` 之一,否则校验失败。
- `contributes.windows` 非空则**必须**设 `ui`。

**决策日志这类"看板 + 读写 .note.md"的插件 = 纯前端形态**,样板照抄 `roam-import`。

---

## 1. 架构:插件由三部分拼成

1. **manifest.v2.json** — 静态声明。宿主扫描它来注册菜单、窗口、能力。类型定义与校验在 `plugin-protocol/src/lib.rs`。
2. **JSON-RPC 2.0 host API**(NDJSON,一行一条)— 插件通过 `host.*` 方法向宿主要能力,每个方法按 capability 授权。分发与鉴权在 `src-tauri/src/plugin_runtime/host_api.rs`。
3. **运行时注册表**(仅主程序内)— 侧栏视图等在主程序启动时硬编码注册,**插件目前无法从 manifest 或桥接注册侧栏**(见 §7)。

前端插件窗口是**隔离 webview,没有 Tauri IPC**;一切宿主能力只能走注入的 `window.notemd.request(method, params)` 桥(`plugins-src/roam-import/src/lib/bridge.ts`)。

---

## 2. 目录结构

纯前端插件(以 `roam-import` 为准):

```
plugins-src/<name>/
├── manifest.v2.json        # 唯一声明文件(安装时重命名为 manifest.json)
├── package.json            # 独立 Svelte/Vite 前端工程
├── vite.config.ts
├── tsconfig.json
├── index.html              # 窗口入口(= WindowContribution.entry)
└── src/
    ├── main.ts             # 挂载 Svelte App
    ├── App.svelte
    └── lib/
        ├── bridge.ts       # window.notemd 桥的类型化封装(照抄)
        ├── strings.ts      # 插件自带 i18n 字符串表(见 §8)
        └── outline/        # ⚠️ 若要写 .note.md,把主程序的 outline 工具「复制」进来
            ├── frontmatter.ts
            ├── markdown.ts
            ├── model.ts
            └── slug.ts
```

**关键约束:插件 UI 跑在隔离 webview,不能 `import` 主程序 `src/` 的代码。** `roam-import` 的做法是把 `src/lib/outline/{frontmatter,markdown,model,slug}.ts` **复制**进自己的 `src/lib/outline/`。要生成 `.note.md` 就照此复制,别试图跨工程引用。

后端插件额外有 `backend/`(独立 Cargo crate,产出一个二进制,走 stdin/stdout NDJSON 与宿主通信),参考 `plugins-src/pos-log/backend/`。

---

## 3. manifest.v2.json 字段全表

类型源:`plugin-protocol/src/lib.rs:13-78`。`#[serde(deny_unknown_fields)]` — **写多余字段会解析失败**。

### 顶层字段

| 字段 | 必填 | 类型 | 说明 |
|---|---|---|---|
| `manifest_version` | ✅ | `2` | 必须等于 2 |
| `id` | ✅ | string | 格式 `publisher.name`,只含 `[a-z0-9-]`,**恰好一个点**(`lib.rs:172-177`) |
| `name` | ✅ | string | 英文/中性显示名。**不支持 `%key%`**,本地化走 `i18n` 字段(§8) |
| `version` | ✅ | string | semver |
| `kind` | ✅ | `"native"` | `wasm` 是保留字,用了直接拒(`lib.rs:182`) |
| `engines` | ✅ | `{ notemd: string }` | semver range,如 `">=6.716.7"`;宿主版本不满足则不加载 |
| `description` | | string | |
| `binary` | | `{ "<triple>": "bin/xxx" }` | target triple → 包内相对路径。当前 triple:`aarch64-apple-darwin` / `x86_64-apple-darwin` |
| `ui` | | string | 前端目录,固定 `"ui/"`。有 `windows` 时必填 |
| `activation` | ✅ | `{ events: string[] }` | 见下 |
| `contributes` | | object | 见下 |
| `capabilities` | ✅ | string[] | 见 §5。空数组合法 |
| `request_timeout_seconds` | | number | 默认 30,上限 300 |
| `idle_shutdown_seconds` | | number | 进程空闲自动关闭秒数 |
| `i18n` | | object | 透传结构(§8),宿主不解释 |

### `activation.events` 合法值(`lib.rs:198-203`)

- `"*"` — 启动即激活
- `"onStartupFinished"` — 启动完成后
- `"onCommand:<cmd>"` — 菜单/命令触发
- `"onCli:<sub>"` — CLI 子命令触发
- `"onFileType:<ext>"` — 打开某类文件时

其它一律校验失败。开窗口的插件通常用 `["onCommand:open"]`。

### `contributes`(`lib.rs:53-78`)

```jsonc
"contributes": {
  "menus":         [ /* MenuEntry,语义同 v1,宿主透传 */ ],
  "context_menus": [ /* ContextMenuEntry */ ],
  "windows":       [ WindowContribution ],   // 见下
  "custom_editors":[ /* 自定义编辑器 */ ],
  "settings":      { /* 设置面板,语义同 v1 */ },
  "cli":           [ /* CliEntry */ ]
}
```

**MenuEntry**(在 `menus` 里,宿主经 adapter 透传 v1 语义):
```jsonc
{
  "location": "file|window|plugins|tab",   // 菜单归属
  "label": "Import from Roam Research…",    // 可被 i18n 覆盖
  "command": "open",                         // 触发的命令 id
  "submenu": "可选子菜单名",
  "enabled_when": "currentTab.kind == 'markdown'",  // 条件表达式(可选)
  "prompt": { "kind": "save-dialog", "default_filename": "{stem}.pdf",
              "filters": [{ "name": "PDF", "extensions": ["pdf"] }] }  // 可选
}
```

**WindowContribution**(`lib.rs:64-78`):
```jsonc
{
  "id": "main",                 // [a-z0-9-]+;实际窗口 label = plugin-<sanitized id>-main
  "entry": "index.html",        // ui/ 内相对路径,禁止含 ".."
  "title": "决策日志",          // 缺省用插件 name
  "width": 680.0, "height": 620.0,
  "min_width": 520.0, "min_height": 420.0,   // 可选
  "singleton": true,            // 默认 true
  "open_command": "open"        // ★ 命中此 command 的菜单项 = 打开本窗口(不走 command.execute)
}
```

`open_command` 是打开窗口的关键:菜单项的 `command` 与某窗口的 `open_command` 相等时,点菜单即开窗,无需插件进程参与。

---

## 4. 校验规则(`validate_manifest`,`lib.rs:170-206`)

被拒的情形(每条都有单测):
- `manifest_version != 2`
- `id` 不是 `publisher.name` 形状 / 含大写
- `version` 非 semver
- `engines.notemd` range 不匹配宿主版本
- `kind == "wasm"`
- 既无 `binary` 又无 `ui`
- 有 `windows` 但没设 `ui`
- window `entry` 含 `..`、window `id` 含大写
- 出现未知 `activation` 事件

本地可跑:`cargo test -p plugin-protocol`。

---

## 5. Capabilities — 能力授权表

**唯一真相:`host_api.rs:32-50` 的 `method_capability()`。** 未在表内的方法一律 `__unknown__` → 拒绝(`-32601`)。未声明对应 capability 的调用 → `-32001 ERR_CAPABILITY_DENIED`。

| capability token | 解锁的 `host.*` 方法 |
|---|---|
| *(无需声明)* | `host.log.info` / `host.log.warn` / `host.log.error` |
| `toast` | `host.toast` |
| `ui` | `host.ui.post`(插件进程 → 自己的窗口推消息) |
| `dialog` | `host.dialog.open` / `host.dialog.save` |
| `vault.read` | `host.vault.info` / `host.vault.read` / `host.vault.exists` / `host.vault.list` |
| `vault.write` | `host.vault.write` / `host.vault.mkdir` |
| `fs.read:dialog` | `host.fs.read_text` / `host.fs.read_bytes`(仅限本会话内经 dialog 选中的路径) |
| `clipboard.write` | `host.clipboard.write` |
| `location` | `host.location.get` |
| `editor.open` | `host.editor.open` |
| `renderer.html` | 渲染类(md2pdf 用) |

**通道差异(重要)**:`dialog.*` / `fs.*` / `clipboard.*` **只在 UI 桥可用**;后台进程通道(纯后端插件)即使声明了 `dialog` 也拿不到,会回 `-32601`(`host_api.rs:165-168`)。`vault.*` 和 `location.get` 两个通道都可用。

**决策日志需要的最小集合**:`["vault.read", "vault.write", "toast"]`(开窗口时天然可用,窗口本身不需要单独 capability)。

---

## 6. Host API 方法参考(确切参数/返回形状)

前端调用一律:`await window.notemd.request(method, params)` → 返回 `result`,出错 throw。类型化封装照抄 `bridge.ts`。

### vault(源:`ui_rpc.rs:525-576`)

| 方法 | 参数 | 返回 |
|---|---|---|
| `host.vault.info` | — | `{ root: string\|null, wiki_dir: string\|null, daily_dir: string\|null }` |
| `host.vault.read` | `{ path }` | `{ content: string }` |
| `host.vault.write` | `{ path, content }` | `{ ok: true }` |
| `host.vault.exists` | `{ path }` | `{ exists: bool }` |
| `host.vault.list` | `{ path }` | `{ entries: [{ name, is_dir }] }`(按 name 排序) |
| `host.vault.mkdir` | `{ path }` | `{ ok: true }` |

**路径规则(`resolve_in_vault`,`ui_rpc.rs:470-523`)**:
- `path` **必须 vault 相对**;绝对路径、任何 `..` 段都被拒(还防符号链接逃逸)。
- `host.vault.write` **自动创建父目录**,**没有 `create_dirs` 参数**(勿照搬旧文档)。
- 读写上限 **10 MB**(UTF-8 字节),超了报 `too_large`。
- 未配置 vault 时报 `vault_required: …`。

> ⚠️ 常见误区更正:`vault.write` 参数只有 `{path, content}`;`vault.info` 返回的是 `{root, wiki_dir, daily_dir}` 而非 `{root, subdirs}`。

### 其它

| 方法 | capability | 参数 → 返回 |
|---|---|---|
| `host.toast` | `toast` | `{ level: "success\|info\|warn\|error", message, detail? }` → `{ok}` |
| `host.log.*` | — | `{ message }` → 写入 `<id>.log` |
| `host.dialog.open` | `dialog` | `{ title?, multiple?, filters? }` → `{ paths: string[]\|null }` |
| `host.dialog.save` | `dialog` | `{ title?, default_filename?, filters? }` → `{ path: string\|null }` |
| `host.fs.read_text` | `fs.read:dialog` | `{ path }` → `{ content }`(仅 dialog 授权过的路径) |
| `host.clipboard.write` | `clipboard.write` | `{ text }` → `{ok}` |
| `host.location.get` | `location` | — → 位置对象 |
| `host.editor.open` | `editor.open` | `{ path }`(vault 相对)→ `{ ok: true }`;在主编辑器打开文件并聚焦主窗口。仅 UI 桥可用。 |

错误码(`lib.rs:106-110`):`-32001` 能力被拒 / `-32601` 方法不存在 / `-32000` 宿主执行失败(消息带 `"<kind>: <detail>"` 前缀)。

---

## 7. 侧栏(side panel)——当前无插件注册入口 ⚠️

侧栏注册表在**主程序**:`src/lib/side-panel/registry.svelte.ts` + `model.ts`。

`SideView` 接口(`model.ts:13-26`):
```ts
interface SideView {
  id: string
  side: 'left' | 'right'
  order: number
  title: () => string                      // i18n,语言切换会重算
  isAvailable: () => boolean               // 读某个 gate 的 enabled
  appliesTo: (tab: Tab | null) => boolean
  component: () => Promise<{ default: SideViewComponent }>
}
```

内置三个视图**硬编码**在 `registerBuiltinSideViews()`(`registry.svelte.ts:141-163`):`folder-view`、`outline-notes`、`git-history`,各自 `isAvailable` 读一个 gate。

**现状结论**:`registerSideView()` 是主程序内部 API,**没有暴露给插件的 manifest 声明或桥接方法**。所以插件要出 UI,当前两条路:

1. **独立窗口**(推荐,零核心改动)— 走 `contributes.windows` + `open_command`,和 `roam-import` 一样。**决策日志看板用这条。**
2. **进主程序当内置侧栏** — 在 `registry.svelte.ts` 加一个 `SideView` + 一个 gate,组件放主程序 `src/components/`。这是改核心、需发主程序版本,不属于"装插件"。

> 若未来要做"插件贡献侧栏",需要先在核心加一个注册桥,这本身是一项核心工作,不能在插件侧单方面完成。写 spec 时按现状(独立窗口)设计。

---

## 8. 国际化(i18n)

### 主程序
`src/lib/i18n/`(`en.ts`/`zh.ts`/`ja.ts`/`de.ts`),扁平点分键 + `t(key, params?)`(`store.svelte.ts`)。**英文 en.ts 是基准,其它语言目录作 Partial 覆盖。**

### 插件(隔离 webview,用不了主程序 t)
两条并存:

1. **UI 内字符串** — 插件自带 `src/lib/strings.ts`,结构照抄 `plugins-src/openclaw/src/lib/strings.ts`:一个 `MessageKey` 联合类型 + 每语言一张 catalog + 本地 `t()`,当前语言从 `bridge().locale`(`'en'|'zh'|'ja'|'de'`)拿。
2. **菜单/设置标签本地化** — manifest 顶层 `name` 用英文,再用 manifest 的 `i18n` 字段做 per-locale 覆盖(宿主透传,结构同 v1 PluginI18n):
   ```jsonc
   "name": "Decision Log",
   "i18n": { "zh": { "name": "决策日志", "menus": { "open": "打开决策日志…" } } }
   ```

---

## 9. 从插件生成 / 编辑 .note.md

`.note.md` = front-matter(YAML)+ bullet 大纲正文。主程序工具在 `src/lib/outline/`:
- `frontmatter.ts` — `splitFrontmatterBlock()` 拆首部 YAML;`touchFrontmatter()` 补 `title`/`created`/`updated`。
- `markdown.ts` — `parseOutline()` ↔ `serializeOutline()` 往返(保留未知键)。
- `model.ts` / `slug.ts` — 树模型与 slug。

**插件用法**:把这几个文件**复制**进插件 `src/lib/outline/`(见 §2),在 UI 内组装文本,再 `host.vault.write({ path, content })` 落盘。`roam-import` 就是这么把 Roam 导出转成 `.note.md` 的。

> 决策日志的 `open.decision.note.md` / `archive/*.note.md` 结构是 front-matter 里放 `decisions` 数组,正文是人类可读镜像——用复制过来的 `touchFrontmatter` + 自定义序列化即可,不必强套 outline 树。

---

## 10. 构建、安装、发现、发布

### 本地 dev 安装(`scripts/dev-install-plugin.sh`)
```bash
scripts/dev-install-plugin.sh [--release] [md2pdf|roam-import|openclaw|cef|pos-log|decision-log|weekly-review|claude-agent]
```
它构建 UI(Vite → `dist/`)和/或后端二进制,拷进安装根,并把 `state.json` 里 `installed[<id>] = {version, enabled:true}` 打开。**新插件需在此脚本加一个分支。**

### 安装布局与发现(`discovery.rs:1-72`)
- 安装根:`$HOME/Library/Application Support/net.notemd.app/plugins/`
- 每插件:`<id>/<version>/`(含 `manifest.json`、`bin/`、`ui/`)+ 符号链接 `<id>/current → <version>`
- 启动时:对 `state.json` 里每个 `enabled` 项,读 `<root>/<id>/current/manifest.json`,校验通过才加载;缺当前架构二进制的会被跳过(纯 UI 插件无 `binary`,不受影响)。

### 独立窗口 capabilities
`src-tauri/capabilities/default.json` 是**主应用**的 Tauri 权限。**插件窗口不需要在这里登记**——插件窗口是宿主用受限 webview 打开的,能力完全由 manifest 的 `capabilities` 数组 + `window.notemd` 桥决定。(这点与"主程序自建的独立 Tauri 窗口需进 capabilities allowlist"不同,别混。)

### 发布
- `scripts/release-plugins.sh` — 打包 `.notemdpkg`(ZIP)到 `dist-plugins/<id>/<version>/`。
- `scripts/gen-plugin-index.mjs` — 扫描产物生成 `index.json`(含 SHA256/size),供 plugins.notemd.net 市场服务。

---

## 11. 新建一个纯前端插件的最小骨架清单

1. `plugins-src/<name>/` 建工程(照抄 `roam-import` 的 `package.json`/`vite.config.ts`/`tsconfig.json`/`index.html`/`src/main.ts`)。
2. 写 `manifest.v2.json`:`id`、`ui: "ui/"`、`activation: ["onCommand:open"]`、`contributes.menus`(一个 `command:"open"`)+ `contributes.windows`(一个 `open_command:"open"`)、`capabilities`。
3. 复制 `bridge.ts`;需要写 .note.md 再复制 `outline/`。
4. 写 `src/lib/strings.ts`(i18n)。
5. 在 `scripts/dev-install-plugin.sh` 加分支。
6. `scripts/dev-install-plugin.sh <name>` → 重启 app → 从菜单打开验证。

---

## 附:核对基线速查

| 主题 | 文件 | 关键行 |
|---|---|---|
| Manifest 类型 + 校验 | `plugin-protocol/src/lib.rs` | 13-78, 170-206 |
| 能力表 / 鉴权 / 通道差异 | `src-tauri/src/plugin_runtime/host_api.rs` | 32-50, 165-184 |
| vault 方法形状 + 路径安全 | `src-tauri/src/plugin_runtime/ui_rpc.rs` | 470-576 |
| UI 桥 `window.notemd` | `plugins-src/roam-import/src/lib/bridge.ts` | 全文 |
| 侧栏模型 / 注册表 | `src/lib/side-panel/model.ts`, `registry.svelte.ts` | 13-26, 141-163 |
| .note.md 工具 | `src/lib/outline/{frontmatter,markdown,model,slug}.ts` | — |
| 插件 i18n 样例 | `plugins-src/openclaw/src/lib/strings.ts` | 全文 |
| dev 安装 | `scripts/dev-install-plugin.sh` | 全文 |
| 发现 / 安装布局 | `src-tauri/src/plugin_runtime/discovery.rs` | 1-72 |
| 纯前端样板 | `plugins-src/roam-import/` | manifest + src |
| 纯后端样板 | `plugins-src/pos-log/` | backend/ |
