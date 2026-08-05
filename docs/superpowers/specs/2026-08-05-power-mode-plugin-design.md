# Power Mode 插件设计（obsidian-power-mode → note.md）

> 日期：2026-08-05
> 源项目：`~/git/obsidian-power-mode`（Obsidian 插件，TypeScript + React 19）
> 移植方案基线：`~/.paseo/uploads/…/2026-08-05-power-mode-port-design.md`（用户上传，本文在其之上做「插件化」重设计）
> 目标：note.md 6.805.2+ / 插件 `notemd.power-mode` 1.0.0
> 状态：已实现并发布（宿主 v6.805.3，插件 notemd.power-mode@1.0.0，2026-08-05）。
> 落地偏离：震动默认关闭；强度与连击超时改为三档分段选择（轻/中/重、短/中/长），非滑块。

---

## 1. 目标与形态

给 note.md 加一个可安装的「狂暴模式」插件：打字时在光标处炸特效、右上角记连击、编辑区随敲击轻微震动。

用户可见形态：

- 插件菜单里一条 **「狂暴模式」**（英文 `Power Mode`）。
- 点开是一个设置窗口：**逐生效面的开关** + 特效参数 + 底部一块**实操编辑器**，改一格参数立刻在这块编辑器上看到效果。
- 生效面：**主编辑窗口一个开关，每个内嵌 Editor Kit 的插件窗口各一个开关**。主编辑窗口默认关，Idea Spark 默认开。

### 1.1 一个绕不开的架构事实

插件 UI 跑在隔离 webview 里，**无法把代码注入到别的窗口**。要让特效在主编辑窗口和 Idea Spark 窗口里生效，特效引擎就必须是**宿主代码**：

- 主编辑窗口用宿主自己的 moraya 编辑器（`src/components/RichEditor.svelte`）。
- Idea Spark 窗口用的是宿主下发的 Editor Kit（`plugin://<id>/__host__/assets/editor-kit-v1.js`，源在 `src/editor-kit/`）。

所以本项目 = **宿主加引擎 + 插件当控制台**，需要发一版主程序，不可能是纯插件。

### 1.2 插件间依赖关系

**power-mode 不依赖 idea-spark，idea-spark 一行代码都不用改。** 两者只在宿主 Editor Kit 这一层相遇：引擎进了 Kit，所有内嵌 Kit 的插件窗口自动具备能力，由配置里的 per-surface 开关决定是否真的开。将来任何新插件用了 Kit，设置窗口会自动多出一行开关。

---

## 2. 范围

### 2.1 做

| 能力 | 说明 |
|---|---|
| 光标爆炸 | 每 N 次输入在光标处生成一个动画 GIF 层 |
| Combo 连击 | 右上角计数 + 进度条 + 闪烁 + 每 10 连击的感叹词 |
| 屏幕震动 | 每次输入用 CSS `translate3d` 随机位移编辑区 |
| 设置窗口 | 生效面开关 + 9 个参数 + Editor Kit 实操区 |

预设 4 个：`particle`、`lightning`、`coin`、`confetti`（13 个 GIF，约 175 KB）。

### 2.2 不做（明确排除，不留后续口子）

| 项 | 原因 |
|---|---|
| **source 模式** | 彻底不做。source 模式是 textarea，没有 `coordsAtPos`，爆炸无法定位；连击/震动单独在 source 生效会造成「同一个编辑器切个模式行为不一致」的割裂感。富文本模式是唯一生效模式 |
| 其余 7 个预设（flame/rift/magic/firework/pikachu/shapes/spark） | 范围外，约 594 KB |
| 用户自定义图片特效 | 隔离 webview 引用本地图片要另起一整套路径/字节通道，主窗口与 Kit 窗口取字节路径还不同 |
| 整窗口物理震动 | `setPosition()` 是 async IPC，逐键调用掉帧；多窗口下语义也混乱 |
| 源项目 `src/components/**`（460 行 React 表单体系） | 重写为 Svelte |
| React / `@dnd-kit` ×3 / `json-edit-react` / `lodash` / `typesafe-i18n` | 一律不引入 |

**不新增任何 npm 依赖。**

---

## 3. 架构

```
宿主 (note.md)
├── src/lib/power-mode/                  ← 特效引擎（框架无关，零 Tauri IPC）
│   ├── plugin.ts        powerModePlugin(getConfig): ProseMirror Plugin —— 唯一 tick 入口
│   ├── overlay.ts       全屏 fixed overlay 容器管理
│   ├── explosion.ts     ← 移植自 src/explosion.ts
│   ├── combo.ts         ← 移植自 src/combo.ts
│   ├── shaker.ts        ← 移植自 src/screen-shaker.ts
│   ├── presets.ts       4 个预设常量（不含素材字节）
│   ├── config.ts        默认值 / 合并 / 生效面判定（纯函数，可单测）
│   ├── types.ts
│   └── power-mode.css   ← 移植自 styles.css 前 42 行
├── src/components/RichEditor.svelte     ← 挂载点 A：主编辑窗口
├── src/editor-kit/rich.ts + main.ts     ← 挂载点 B：所有插件窗口的 Editor Kit
├── src/lib/settings.svelte.ts           ← 配置读写（插件域）
├── src-tauri/src/plugin_runtime/        ← 两条新 RPC
└── public/assets/power-mode/**.gif      ← 13 个素材

插件 plugins-src/power-mode/             ← 菜单项 + 设置窗口 + 实操区
├── manifest.v2.json
└── src/{App.svelte, lib/{bridge,strings,editor-kit,config}.ts}
```

### 3.1 引擎接入两个挂载点

两处都已有「挂载后追加 ProseMirror 插件」的既有写法，照抄：

```ts
view.updateState(view.state.reconfigure({
  plugins: view.state.plugins.concat(powerModePlugin(getConfig)),
}))
```

- `RichEditor.svelte:1007-1021` 追加 wikilink 装饰插件就是这个形状。
- `src/editor-kit/rich.ts:88-97` 追加 placeholder 插件也是。

插件本体只依赖 `prosemirror-state` / `prosemirror-view`，**不碰任何 `@tauri-apps/*`**，才能进 Kit 的依赖图（Kit 的硬约束，见 `src/editor-kit/main.ts` 文件头）。配置以 `getConfig: () => EffectiveConfig | null` 回调注入，返回 `null` 即整体停用 —— 宿主与 Kit 各自决定怎么拿配置，引擎不关心。

### 3.2 素材寻址（关键约束）

`__host__` 只镜像 `dist/assets/` 这一个目录（`protocol.rs`，已有单测钉死）。所以：

1. GIF 放 `public/assets/power-mode/<preset>/<n>.gif` → Vite 原样拷到 `dist/assets/power-mode/`。
2. **不能用 `import gif from './x.gif'`** —— Vite 会注入绝对路径 `/assets/x-hash.gif`，在 Kit 窗口里解析成 `plugin://<id>/assets/…` 直接 404。
3. 必须相对模块自身 URL 解析：

```ts
const base = import.meta.env.DEV
  ? '/assets/power-mode/'
  : new URL(/* @vite-ignore */ './power-mode/', import.meta.url).href
```

主窗口 chunk 在 `/assets/xxx.js` → `/assets/power-mode/…` ✅
Kit 在 `plugin://<id>/__host__/assets/editor-kit-v1.js` → `plugin://<id>/__host__/assets/power-mode/…` ✅

这正是 `src/editor-kit/main.ts:97-106` 的 `injectKitCss()` 已验证过的套路。

`gifMode: 'restart'` 用 `?t=<ts>` 查询参数重放，源项目往 base64 字符串里插时间戳的 hack（`explosion.ts:68-75`）整段删掉。

### 3.3 状态隔离

源项目 `explosion.ts:5` 和 `combo.ts:6` 各有一个**模块级** `count`，`frequency` 门控依赖它。note.md 里同时存在主窗口编辑器 + 若干 Kit 实例，模块级状态会串。**移植时改为每个 ProseMirror 插件实例一份状态**（存在 plugin state / 闭包里）。

顺手修掉源项目 `explosion.ts:39` 的 `if (index > 0)`（应为 `>= 0`）：index 为 0 时数组项不被移除，留下陈旧条目占 `maxExplosions` 名额。

---

## 4. 配置

### 4.1 形状

存宿主 `settings.json` 的插件域，key 前缀 `notemd.power-mode.`（复用已有的 `getPluginScopedAll` / `mergePluginScoped`，`src/lib/settings.svelte.ts:280-380`）。

```ts
interface PowerModeConfig {
  /** 每个生效面一个开关。key: 'main' | <plugin-id>。缺省值见 4.2 */
  surfaces: Record<string, boolean>
  shake:     { enable: boolean; intensity: number; recoverTime: number }
  combo:     { enable: boolean; timeout: number; showExclamation: boolean; precisionInput: boolean }
  explosion: { enable: boolean; presetId: 'particle' | 'lightning' | 'coin' | 'confetti' }
  /** 用户在内置预设之上的改动。内置预设本身只存 id，参数从代码常量读 */
  overrides?: Partial<ExplosionConfig>
}

interface ExplosionConfig {
  maxExplosions: number
  size: number                 // 宽 size ch / 高 size rem
  frequency: number            // 每 N 次输入触发一次
  explosionOrder: 'random' | 'sequential' | number
  gifMode: 'continue' | 'restart'
  duration: number             // ms
  offset: number               // 上移 offset × size rem
  backgroundMode: 'mask' | 'image'
  imageList: string[]          // 相对 3.2 的 base 的路径
  customStyle?: Partial<CSSStyleDeclaration>
}
```

**内置预设绝不整份写进 settings.json**（源项目 `Panel.tsx:80-99` 会把含 base64 `imageList` 的整个 preset 拷进 `data.json`，搬过来会让全量读写的 settings.json 膨胀几百 KB）。

### 4.2 默认值

| 键 | 默认 |
|---|---|
| `surfaces.main` | `false` |
| `surfaces['notemd.idea-spark']` | `true` |
| `surfaces[<其它 Kit 插件>]` | `true`（与 Idea Spark 一致：Kit 窗口默认开） |
| `shake` | `{ enable: true, intensity: 5, recoverTime: 800 }` |
| `combo` | `{ enable: true, timeout: 10, showExclamation: true, precisionInput: false }` |
| `explosion` | `{ enable: true, presetId: 'particle' }` |

### 4.3 两条新 host RPC

沿用 `host.editor.open` 的「RPC → `HostServices` → 主窗口前端」转发路数（`ui_rpc.rs:859-863`，settings store 由前端持有）。

| 方法 | capability | 参数 → 返回 |
|---|---|---|
| `host.power_mode.config` | `editor.kit` | — → `{ config: EffectiveConfig \| null, surfaces: [{ id, name }] }` |
| `host.power_mode.update` | `power-mode`（新 token） | `{ config: PowerModeConfig }` → `{ ok: true }` |

两条都**只在 UI 桥可用**（进程通道 `-32601`），与 `host.theme.css` 同例。

`host.power_mode.config` 返回的是**宿主算好的生效值**：

- power-mode 插件被**停用或卸载** → `config: null`（全关）。这样不会出现「卸了还在炸」，也省掉「卸载时清理配置文件」的钩子。
- `surfaces` 清单由宿主扫描已装且启用的插件里声明了 `editor.kit` 的那些得出，外加固定的 `main`。插件窗口不需要自己去猜有哪些插件，也拿不到与此无关的插件清单。

`host.power_mode.update` 由 power-mode 插件独家声明 `power-mode` capability 使用。

> capability token 是 manifest 自声明的，不是安全边界；这里只是把「谁该调这条」写清楚。配置本身是装饰性偏好，不涉及敏感数据。

### 4.4 谁在什么时候读

| 生效面 | 读法 | 刷新时机 |
|---|---|---|
| 主编辑窗口 | 直接读 `getPluginScopedAll('notemd.power-mode')` | 订阅 `pluginScopedVersion`，改完立即生效 |
| Kit 插件窗口 | `window.notemd.request('host.power_mode.config')` | 挂载时 + **窗口获得焦点时**重拉一次 |
| 插件实操区 | 不读配置，直接用内存里正在编辑的那份 | 每次控件变动即时推 |

插件窗口没有 Tauri IPC，收不到 `settings://changed` 广播。用「focus 时重拉」代替推送通道：一次轻量 RPC，够用，不新增推送机制。

---

## 5. Editor Kit API 增补

Kit v1 的兼容约定是「不改既有成员、可以加成员」（`src/editor-kit/main.ts:27-33`）。本项目加两处：

```ts
interface KitOptions {
  /** 省略 = 由宿主配置决定（默认路径）；显式给值 = 调用方自管，供实操区用 */
  powerMode?: PowerModeConfig | null
}

interface KitEditor {
  /** 实时替换特效配置，不重挂编辑器（重挂会丢光标/选区/撤销栈） */
  setPowerMode(cfg: PowerModeConfig | null): void
}
```

默认路径（`opts.powerMode` 省略）：Kit 自己调 `host.power_mode.config`，按 `surfaces[window.notemd.pluginId]` 判定是否启用 —— Idea Spark 因此零改动就能生效。

实操区路径（显式给值）：完全由插件窗口自己控制，不受 `surfaces` 影响，改一格滑块就 `setPowerMode()` 推一次。

---

## 6. 插件窗口

### 6.1 manifest

```jsonc
{
  "manifest_version": 2,
  "id": "notemd.power-mode",
  "name": "Power Mode",
  "version": "1.0.0",
  "kind": "native",
  "engines": { "notemd": ">=6.805.3" },   // 以本次实际发布的宿主版本为准（日期版本号规则）
  "ui": "ui/",
  "activation": { "events": ["onCommand:open"] },
  "contributes": {
    "menus":   [{ "location": "plugins", "label": "Power Mode", "command": "open" }],
    "windows": [{ "id": "main", "entry": "index.html", "title": "Power Mode",
                  "width": 620, "height": 760, "min_width": 520, "min_height": 600,
                  "open_command": "open" }]
  },
  "capabilities": ["editor.kit", "power-mode"],
  "i18n": {
    "zh": { "name": "狂暴模式", "menus": { "open": "狂暴模式" } },
    "ja": { "name": "パワーモード", "menus": { "open": "パワーモード" } },
    "de": { "name": "Power-Modus", "menus": { "open": "Power-Modus" } }
  }
}
```

不需要 `vault.read` / `vault.write`：配置走 RPC，实操区的文档是内存里的示例文本，不落盘。

### 6.2 布局

```
┌─ 狂暴模式 ─────────────────────────────┐
│ 生效范围                                │
│   ☐ 主编辑窗口                          │
│   ☑ 奇思妙想 (Idea Spark)               │  ← 由 RPC 的 surfaces 清单动态渲染
│ ───────────────────────────────────── │
│ 光标爆炸  ☑        预设 [particle ▾]    │
│ 屏幕震动  ☑   强度 ▓▓▓░ 5   恢复 800 ms │
│ 连击计数  ☑   超时 10 s                 │
│               ☑ 感叹词  ☐ 精确输入      │
│ ───────────────────────────────────── │
│ 试试看                                  │
│ ┌─────────────────────────────────┐   │
│ │ (Editor Kit 富文本编辑器，示例文本) │   │
│ └─────────────────────────────────┘   │
└────────────────────────────────────────┘
```

- 生效范围之外的参数是**全局**的，对所有生效面同时起作用（不做 per-surface 参数，避免设置爆炸）。
- 任一控件变动 → 立即 `setPowerMode()` 推给实操区 + `host.power_mode.update` 落盘（debounce 300 ms）。
- Kit 加载失败（宿主过老 / 能力缺失）时实操区降级为一行提示，窗口其余部分照常可用 —— 与 idea-spark 的降级约定一致。
- 实操区容器必须有确定高度（Kit 的硬要求，`main.ts:114-123`）：用 flex 子项 + `min-height: 0`。

### 6.3 i18n

插件自带 `src/lib/strings.ts`（en/zh/ja/de 四张 catalog + 本地 `t()`，照抄 `plugins-src/openclaw/src/lib/strings.ts` 的结构），语言从 `bridge().locale` 取。菜单名走 manifest `i18n`。配一份 `strings.test.ts` 断言四语种 key 齐平（见 `docs/` 的插件 i18n 审计结论）。

---

## 7. 引擎行为细节

### 7.1 tick 入口

```ts
Plugin({ view: () => ({ update(view, prevState) {
  if (view.state.doc.eq(prevState.doc)) return   // 只对文档变更计数
  tick(view)
} }) })
```

对照源项目 `workspace.on('editor-change')`（`main.ts:32`）。

### 7.2 爆炸

- 坐标：`view.coordsAtPos(view.state.selection.head)` → 视口坐标，**直接用，不做滚动修正**。
- 容器：全屏 `position: fixed; inset: 0; pointer-events: none` overlay，z-index 低于模态框层；生命周期跟随插件实例，`destroy()` 时整体移除。
- 已存在的特效在滚动时不跟随文本移动 —— 与原实现行为一致。
- 尺寸 `width: ${size}ch; height: ${size}rem`。note.md 的编辑器字体与 Obsidian 不同，**四个预设的 `size` 需要逐个目视调**（见 §9）。
- `particle` 是唯一用 `mask` 模式的（`background-color: currentColor` + `mask-image`），自动跟随主题文字色；也是唯一 `gifMode: 'continue'` 的，需要 `preload()`。
- `lightning` 带 `mixBlendMode: 'color-dodge'`，在 fixed overlay 这个新层叠上下文里表现需实测。

### 7.3 连击

- 右上角计数 + 进度条 + 每 10 连击感叹词，`timeout` 秒无输入则清零。
- `precisionInput: true` 时只在文档变长时计数：文档长度取 `view.state.doc.content.size`（对照源项目 `info.editor.getValue().length`）。
- 长度基线的 key：主窗口用当前文件路径；Kit 窗口没有文件概念，用挂载实例 id。
- 源项目用 Obsidian 的 `HTMLElement.createDiv({cls, attr})`（5 处），全部换成原生 `createElement` + `classList`。

### 7.4 震动

CSS `transform: translate3d()` 作用于 `view.dom.parentElement`，`intensity` px 随机位移，`recoverTime` ms 内衰减回零。

---

## 8. 测试

| 层 | 内容 |
|---|---|
| vitest（宿主引擎） | `config.ts` 的默认值合并 / 预设 overrides 合并 / `surfaces` 判定（含未知插件 id 走默认 true、`config: null` 全关）；`frequency` 门控计数；连击状态机（累加 / 超时清零 / precisionInput 只在变长时计）；爆炸数组裁剪（钉死 `>= 0` 的修正）；素材 URL 拼接（dev / prod 两条分支） |
| vitest（Kit） | `powerMode` 省略 vs 显式给值两条路径；`setPowerMode` 不触发重挂 |
| vitest（插件） | `strings.test.ts` 四语种 key 齐平；配置读写与 debounce |
| cargo test | 两条新 RPC 的参数/返回形状；capability 拒绝（无 `editor.kit` 调 config → `-32001`，无 `power-mode` 调 update → `-32001`）；进程通道 → `-32601`；插件停用时 config 返回 null |
| cargo test -p plugin-protocol | manifest 校验通过 |

GUI 由用户实机验证（本项目不做 UI 自动化）。

---

## 9. 风险与验证点

| 风险 | 处理 / 验证 |
|---|---|
| 尺寸单位失真（`ch`/`rem` 依赖字体度量） | 四个预设逐个对照源项目 `screenshots/` 目视调 `size` |
| `mask` 模式下 GIF 在 WKWebView 里是否动画 | 单独测 particle |
| `color-dodge` 在 fixed overlay 新层叠上下文里的表现 | 单独测 lightning，深浅色主题各一次 |
| 用了 Kit 的插件联调必须先 `pnpm build` | dev 下 `__host__` 读磁盘 `dist/`，不热更新（规范 §5 已注明）；写进实施计划的验证步骤 |
| 未装插件的用户也背着 175 KB 素材 | 接受。换来主窗口与 Kit 共用一套素材、零跨源问题 |
| 引擎误引入 Tauri 依赖会炸掉整个 Kit | 引擎只 import `prosemirror-state`/`prosemirror-view`；Kit 已有的依赖约束注释里补一句 |
| 高频输入下 overlay 节点堆积 | `maxExplosions` 裁剪 + `duration` 到期 `remove()`；连续快速输入 30 s 观察 DOM 节点数 |

---

## 10. 交付顺序

每步可独立验证：

1. **素材提取** — 脚本把 4 个预设的 13 个 base64 解码到 `public/assets/power-mode/**`，逐个确认能播放。
2. **引擎骨架 + 震动** — `plugin.ts` tick 链路 + `shaker.ts`，先只挂主窗口，写死配置跑通。
3. **连击** — `combo.ts`，纯 DOM。
4. **爆炸** — `overlay.ts` + `explosion.ts` + `presets.ts` + 素材寻址（§3.2）。四预设目视调 `size`。
5. **配置通道** — settings 插件域 + 两条 RPC + 主窗口订阅 `pluginScopedVersion`。
6. **Kit 接入** — `KitOptions.powerMode` / `setPowerMode`，Idea Spark 窗口验证零改动生效。
7. **插件** — `plugins-src/power-mode/` 工程 + 设置窗口 + 实操区 + i18n；`scripts/dev-install-plugin.sh` 加分支。
8. **发布** — 宿主版本 bump + 插件打包上架市场（注意 `gen-plugin-index.mjs` 默认 merge 线上索引）。

---

## 11. 工作量估算

| 模块 | 规模 |
|---|---|
| 素材提取脚本（一次性） | ~40 行 |
| 引擎（plugin/overlay/explosion/combo/shaker/presets/config/types） | ~520 行 |
| CSS | ~45 行 |
| 两个挂载点接线 | ~50 行 |
| 两条 RPC（Rust）+ 前端转发 | ~200 行 |
| Kit API 增补 | ~40 行 |
| 插件工程（App.svelte + lib + manifest + i18n） | ~420 行 |
| 测试 | ~400 行 |

合计约 **1700 行 + 175 KB 静态素材**，不新增 npm 依赖。
