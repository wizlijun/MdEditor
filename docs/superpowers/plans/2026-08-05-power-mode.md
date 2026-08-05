# Power Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 note.md 加「狂暴模式」：打字时在光标处炸特效、右上角记连击、编辑区随敲击轻微震动；由一个可安装插件 `notemd.power-mode` 提供菜单项、设置窗口和实操区。

**Architecture:** 特效引擎是**宿主代码**（`src/lib/power-mode/`），因为隔离 webview 无法向别的窗口注入代码。引擎以 ProseMirror 插件形态挂在两处：主窗口的 `RichEditor.svelte`，以及宿主下发给所有插件窗口的 Editor Kit（`src/editor-kit/`）。配置存宿主 `settings.json` 的插件域，插件窗口经两条新 host RPC 读写。插件本体只是控制台：菜单项 + 设置窗口 + 一块内嵌 Editor Kit 的实操区。

**Tech Stack:** Svelte 5 + Vite 6 + ProseMirror（`prosemirror-state` / `prosemirror-view`）+ Rust/Tauri 2 + vitest + cargo test。

**Spec:** `docs/superpowers/specs/2026-08-05-power-mode-plugin-design.md`
**移植源:** `~/git/obsidian-power-mode`（只读参考，不要修改它）

## Global Constraints

- **不新增任何 npm 依赖。** 源项目用到的 `lodash.random` / `lodash.sample` 手写几行替代，`@dnd-kit`/`react`/`json-edit-react`/`typesafe-i18n` 一律不引入。
- **source 模式彻底不做。** 只在富文本（ProseMirror）模式生效。不要为 textarea 补任何 fallback。
- **`src/lib/power-mode/**` 与 `src/editor-kit/**` 的依赖图里不得出现 `@tauri-apps/*`、`src/lib/editor-bridge.ts`、tabs、insights、adapters。** 插件 webview 没有 Tauri IPC，引入即炸掉整个 Editor Kit。唯一例外是 `src/lib/power-mode/host-config.svelte.ts`（只给主窗口用，Kit 不 import 它）。
- **素材必须落在 `dist/assets/power-mode/` 下，且用相对 `import.meta.url` 寻址。** `__host__` 只镜像 `dist/assets/`；用 Vite 的 `import x from './a.gif'` 会注入绝对路径 `/assets/…`，在插件窗口解析成 `plugin://<id>/assets/…` 直接 404。
- **插件 id 含点。** `mergePluginScoped()` / `getPluginScopedKey()` 按**第一个点**切分 fq key，对 `notemd.power-mode.config` 会切成 id=`notemd`。禁止对本插件使用这两个函数；用 Task 6 新增的 `getPluginScopedValue(pluginId, key)` / `setPluginScopedValue(pluginId, key, value)`。
- **jsdom 没有 `Element.prototype.animate`。** 所有 `el.animate(...)` 一律写成 `el.animate?.(...)`，否则单测炸。
- **jsdom 没有布局。** 不要给 `view.coordsAtPos()` 写单测；把可测的纯逻辑抽成独立函数。
- 预设固定 4 个：`particle`、`lightning`、`coin`、`confetti`。
- 宿主测试命令：`pnpm test`（vitest）、`pnpm check`（svelte-check）、`cd src-tauri && cargo test`。
- 提交信息用中文，末尾附 `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`。
- **共享 worktree：每次 commit 只精确 `git add` 本任务列出的文件，绝不 `git add -A`。**

## File Structure

| 文件 | 职责 |
|---|---|
| `scripts/extract-power-mode-assets.mjs` | 一次性：把源项目 4 个预设里的 base64 GIF 解码到 `public/assets/power-mode/` |
| `public/assets/power-mode/<preset>/<n>.gif` | 13 个素材（Vite 原样拷进 `dist/assets/power-mode/`） |
| `src/lib/power-mode/types.ts` | `PresetId` / `ExplosionConfig` / `PowerModeConfig` |
| `src/lib/power-mode/presets.ts` | 4 个预设参数常量 + 素材 URL 拼装（`assetBase()`） |
| `src/lib/power-mode/config.ts` | 默认值、`normalizeConfig`、`isSurfaceEnabled`、`resolveExplosion` |
| `src/lib/power-mode/shaker.ts` | 震动 |
| `src/lib/power-mode/combo.ts` | 连击计数器 |
| `src/lib/power-mode/overlay.ts` | 全屏 fixed overlay 容器（引用计数） |
| `src/lib/power-mode/explosion.ts` | 爆炸层 |
| `src/lib/power-mode/plugin.ts` | ProseMirror 插件 + runtime（唯一 tick 入口） |
| `src/lib/power-mode/host-config.svelte.ts` | **仅主窗口**：从 settings 读配置 + 监听插件窗口的更新事件 |
| `src/styles/power-mode.css` | 运行时样式（由 `editor-base.css` `@import`，主窗口与 Kit 同时拿到） |
| `src/lib/settings.svelte.ts` | 加 `getPluginScopedValue` / `setPluginScopedValue`（点号安全） |
| `src/components/RichEditor.svelte` | 挂载点 A |
| `src/editor-kit/{main,rich}.ts` + `power-mode-config.ts` | 挂载点 B + Kit API 增补 |
| `src-tauri/src/plugin_runtime/power_mode.rs` | 两条 RPC 的纯逻辑 |
| `src-tauri/src/plugin_runtime/{host_api,ui_rpc}.rs` | capability 表 + dispatch 接线 |
| `plugins-src/power-mode/**` | 插件工程 |
| `scripts/dev-install-plugin.sh` | 加 `power-mode` 分支 |

---

### Task 1: 素材提取

**Files:**
- Create: `scripts/extract-power-mode-assets.mjs`
- Create: `public/assets/power-mode/{particle/1..8.gif, lightning/1..3.gif, coin/1.gif, confetti/1.gif}`

**Interfaces:**
- Consumes: 无
- Produces: 13 个 GIF，路径形如 `public/assets/power-mode/<preset>/<n>.gif`（n 从 1 起）。后续 `presets.ts` 按 `${base}${preset}/${i+1}.gif` 拼装。

- [ ] **Step 1: 写提取脚本**

Create `scripts/extract-power-mode-assets.mjs`:

```js
#!/usr/bin/env node
// 一次性脚本:把 obsidian-power-mode 的 base64 GIF 解码成静态文件。
//
// 源项目把 13 个 GIF 以 base64 内嵌在 TS 源码里(约 233 KB)。note.md 这边要
// 的是文件:GIF 必须落在 dist/assets/power-mode/ 下,才能同时被主窗口
// (/assets/…) 和插件窗口的 Editor Kit (plugin://<id>/__host__/assets/…) 取到。
//
// 用法: node scripts/extract-power-mode-assets.mjs [源项目路径]
//   默认源路径 ~/git/obsidian-power-mode
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { homedir } from 'node:os'

const SRC = process.argv[2] ?? join(homedir(), 'git/obsidian-power-mode')
const OUT = resolve('public/assets/power-mode')
const PRESETS = { particle: 8, lightning: 3, coin: 1, confetti: 1 }

let total = 0
for (const [preset, expected] of Object.entries(PRESETS)) {
  const file = join(SRC, 'src/presets/explosion', `${preset}.ts`)
  const text = readFileSync(file, 'utf8')
  const matches = [...text.matchAll(/data:image\/gif;base64,([A-Za-z0-9+/=]+)/g)]
  if (matches.length !== expected) {
    throw new Error(`${preset}: expected ${expected} gifs, found ${matches.length}`)
  }
  const dir = join(OUT, preset)
  mkdirSync(dir, { recursive: true })
  matches.forEach((m, i) => {
    const buf = Buffer.from(m[1], 'base64')
    // GIF 魔数自检:解码错了就地失败,别产出一堆坏文件。
    const magic = buf.subarray(0, 6).toString('latin1')
    if (magic !== 'GIF87a' && magic !== 'GIF89a') {
      throw new Error(`${preset}/${i + 1}: not a GIF (magic=${magic})`)
    }
    writeFileSync(join(dir, `${i + 1}.gif`), buf)
    total++
  })
  console.log(`${preset}: ${matches.length} gifs`)
}
console.log(`✓ ${total} gifs → ${OUT}`)
```

- [ ] **Step 2: 跑脚本**

Run: `node scripts/extract-power-mode-assets.mjs`
Expected: 打印 `particle: 8 / lightning: 3 / coin: 1 / confetti: 1` 和 `✓ 13 gifs → …`

- [ ] **Step 3: 核对产物**

Run:
```bash
ls -R public/assets/power-mode && file public/assets/power-mode/*/*.gif | grep -c "GIF image data"
```
Expected: 13 个文件，`file` 全部识别为 GIF image data。总体积约 175 KB。

- [ ] **Step 4: 目视确认能播放**

Run: `open public/assets/power-mode/lightning/1.gif`
Expected: 预览里是一段闪电动画（不是静止的坏图）。其余随机抽两个再看一眼。

- [ ] **Step 5: Commit**

```bash
git add scripts/extract-power-mode-assets.mjs public/assets/power-mode
git commit -m "$(cat <<'EOF'
feat(power-mode): 提取 4 个预设的 13 个 GIF 素材

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: 引擎类型、预设与配置合并（纯函数）

**Files:**
- Create: `src/lib/power-mode/types.ts`
- Create: `src/lib/power-mode/presets.ts`
- Create: `src/lib/power-mode/config.ts`
- Test: `src/lib/power-mode/config.test.ts`

**Interfaces:**
- Consumes: Task 1 的素材目录布局。
- Produces:
  - `type PresetId = 'particle' | 'lightning' | 'coin' | 'confetti'`
  - `interface ExplosionConfig`、`interface PowerModeConfig`（见下）
  - `PRESET_PARAMS: Record<PresetId, PresetParams>`
  - `assetBase(): string`
  - `presetConfig(id: PresetId, base?: string): ExplosionConfig`
  - `DEFAULT_CONFIG: PowerModeConfig`
  - `normalizeConfig(raw: unknown): PowerModeConfig`
  - `isSurfaceEnabled(cfg: PowerModeConfig | null, surfaceId: string): boolean`
  - `resolveExplosion(cfg: PowerModeConfig, base?: string): ExplosionConfig`

- [ ] **Step 1: 写类型**

Create `src/lib/power-mode/types.ts`:

```ts
// Power Mode 的数据形状。移植自 ~/git/obsidian-power-mode 的 type.d.ts,
// 去掉了 shakeWindow / useCustom / customEffect(本项目明确不做)。

export type PresetId = 'particle' | 'lightning' | 'coin' | 'confetti'

export interface ExplosionConfig {
  /** 同屏最大特效数,超出则移除最旧的。 */
  maxExplosions: number
  /** 宽 = size ch,高 = size rem。依赖字体度量,预设值需按本项目字体调。 */
  size: number
  /** 每 N 次输入触发一次。 */
  frequency: number
  explosionOrder: 'random' | 'sequential' | number
  /** 'restart' = 每次重放 GIF(加 ?t= 时间戳);'continue' = 复用浏览器缓存里正在播的那帧。 */
  gifMode: 'continue' | 'restart'
  /** 特效存活毫秒。 */
  duration: number
  /** 上移 offset × size rem。 */
  offset: number
  /** 'mask' = 用 currentColor 填充 + mask-image(自动跟随主题文字色)。 */
  backgroundMode: 'mask' | 'image'
  imageList: string[]
  /** 直接抹到 element.style 上的额外样式(lightning 用它上 mix-blend-mode)。 */
  customStyle?: Record<string, string>
}

export interface PowerModeConfig {
  /**
   * 每个生效面一个开关。key 是 'main'(主编辑窗口)或插件 id。
   * 缺省语义见 `isSurfaceEnabled`:'main' 默认关,插件窗口默认开。
   */
  surfaces: Record<string, boolean>
  shake: { enable: boolean; intensity: number; recoverTime: number }
  combo: { enable: boolean; timeout: number; showExclamation: boolean; precisionInput: boolean }
  explosion: { enable: boolean; presetId: PresetId }
  /** 用户在内置预设之上的改动。内置预设本身只存 id,参数从代码常量读。 */
  overrides?: Partial<ExplosionConfig>
}
```

- [ ] **Step 2: 写预设的失败测试**

Create `src/lib/power-mode/config.test.ts`:

```ts
import { describe, it, expect } from 'vitest'
import { PRESET_PARAMS, presetConfig } from './presets'

describe('presets', () => {
  it('carries the four upstream presets verbatim', () => {
    expect(Object.keys(PRESET_PARAMS).sort()).toEqual(['coin', 'confetti', 'lightning', 'particle'])
    expect(PRESET_PARAMS.particle).toMatchObject({
      maxExplosions: 3, size: 10, frequency: 1, explosionOrder: 'random',
      gifMode: 'continue', duration: 400, offset: 0.25, backgroundMode: 'mask', frameCount: 8,
    })
    expect(PRESET_PARAMS.lightning).toMatchObject({
      maxExplosions: 15, size: 15, frequency: 2, explosionOrder: 'sequential',
      gifMode: 'restart', duration: 1000, offset: 0.2, backgroundMode: 'image', frameCount: 3,
    })
    expect(PRESET_PARAMS.lightning.customStyle).toEqual({ mixBlendMode: 'color-dodge' })
    expect(PRESET_PARAMS.coin).toMatchObject({ maxExplosions: 5, size: 8, frequency: 4, duration: 1500, offset: 0.66, frameCount: 1 })
    expect(PRESET_PARAMS.confetti).toMatchObject({ maxExplosions: 5, size: 26, frequency: 3, duration: 1200, offset: 0.32, frameCount: 1 })
  })

  it('builds 1-based frame urls under the given base', () => {
    const cfg = presetConfig('particle', 'https://host/assets/power-mode/')
    expect(cfg.imageList).toHaveLength(8)
    expect(cfg.imageList[0]).toBe('https://host/assets/power-mode/particle/1.gif')
    expect(cfg.imageList[7]).toBe('https://host/assets/power-mode/particle/8.gif')
    // frameCount 是拼装用的,不该漏进运行时配置
    expect('frameCount' in cfg).toBe(false)
  })
})
```

- [ ] **Step 3: 跑测试确认失败**

Run: `pnpm test src/lib/power-mode/config.test.ts`
Expected: FAIL —— `Failed to resolve import "./presets"`

- [ ] **Step 4: 写 presets.ts**

Create `src/lib/power-mode/presets.ts`:

```ts
import type { ExplosionConfig, PresetId } from './types'

export type PresetParams = Omit<ExplosionConfig, 'imageList'> & { frameCount: number }

/**
 * 四个预设的参数,逐字来自 ~/git/obsidian-power-mode/src/presets/explosion/。
 * 素材路径不在这里:见 `presetConfig`。
 */
export const PRESET_PARAMS: Record<PresetId, PresetParams> = {
  particle: {
    maxExplosions: 3, size: 10, frequency: 1, explosionOrder: 'random',
    gifMode: 'continue', duration: 400, offset: 0.25, backgroundMode: 'mask', frameCount: 8,
  },
  lightning: {
    maxExplosions: 15, size: 15, frequency: 2, explosionOrder: 'sequential',
    gifMode: 'restart', duration: 1000, offset: 0.2, backgroundMode: 'image', frameCount: 3,
    customStyle: { mixBlendMode: 'color-dodge' },
  },
  coin: {
    maxExplosions: 5, size: 8, frequency: 4, explosionOrder: 'random',
    gifMode: 'restart', duration: 1500, offset: 0.66, backgroundMode: 'image', frameCount: 1,
  },
  confetti: {
    maxExplosions: 5, size: 26, frequency: 3, explosionOrder: 'random',
    gifMode: 'restart', duration: 1200, offset: 0.32, backgroundMode: 'image', frameCount: 1,
  },
}

/**
 * 素材根 URL。
 *
 * 必须相对**本模块自己的 URL** 解析,不能用 `import x from './a.gif'`:
 * - 主窗口:本模块在 `/assets/<chunk>.js` → `/assets/power-mode/`
 * - 插件窗口的 Editor Kit:在 `plugin://<id>/__host__/assets/editor-kit-v1.js`
 *   → `plugin://<id>/__host__/assets/power-mode/`(而 `__host__` 只镜像
 *   `dist/assets/`,所以这条路径正好命中)
 *
 * Vite 注入的绝对路径 `/assets/…` 在插件窗口里会解析成
 * `plugin://<id>/assets/…`(插件自己的 ui/ 目录)→ 404。
 *
 * dev 分支:主窗口由 Vite dev server 服务,本模块的 URL 是
 * `/src/lib/power-mode/presets.ts`,相对解析会指错;publicDir 在 dev 下挂在根,
 * 所以直接写绝对路径。built 出来的 Kit 里 `import.meta.env.DEV` 是 false,
 * 两条分支不会互相干扰。
 */
export function assetBase(): string {
  if (import.meta.env.DEV) return '/assets/power-mode/'
  return new URL(/* @vite-ignore */ './power-mode/', import.meta.url).href
}

/** 预设参数 + 素材路径 = 可直接喂给 exploder 的配置。 */
export function presetConfig(id: PresetId, base: string = assetBase()): ExplosionConfig {
  const { frameCount, ...rest } = PRESET_PARAMS[id]
  return {
    ...rest,
    imageList: Array.from({ length: frameCount }, (_, i) => `${base}${id}/${i + 1}.gif`),
  }
}
```

- [ ] **Step 5: 跑测试确认通过**

Run: `pnpm test src/lib/power-mode/config.test.ts`
Expected: PASS（2 个用例）

- [ ] **Step 6: 追加 config 的失败测试**

Append to `src/lib/power-mode/config.test.ts`:

```ts
import { DEFAULT_CONFIG, normalizeConfig, isSurfaceEnabled, resolveExplosion } from './config'

describe('normalizeConfig', () => {
  it('fills every branch from defaults when given junk', () => {
    for (const junk of [null, undefined, 42, 'x', []]) {
      expect(normalizeConfig(junk)).toEqual(DEFAULT_CONFIG)
    }
  })

  it('deep-merges partial input without dropping sibling keys', () => {
    const out = normalizeConfig({ combo: { timeout: 3 }, explosion: { presetId: 'coin' } })
    expect(out.combo).toEqual({ enable: true, timeout: 3, showExclamation: true, precisionInput: false })
    expect(out.explosion).toEqual({ enable: true, presetId: 'coin' })
    expect(out.shake).toEqual(DEFAULT_CONFIG.shake)
  })

  it('rejects an unknown presetId and falls back to the default', () => {
    expect(normalizeConfig({ explosion: { presetId: 'pikachu' } }).explosion.presetId).toBe('particle')
  })

  it('keeps user surface flags and merges them over the defaults', () => {
    const out = normalizeConfig({ surfaces: { main: true } })
    expect(out.surfaces.main).toBe(true)
    expect(out.surfaces['notemd.idea-spark']).toBe(true)
  })

  it('drops a non-object overrides but keeps a real one', () => {
    expect(normalizeConfig({ overrides: 'nope' }).overrides).toBeUndefined()
    expect(normalizeConfig({ overrides: { size: 20 } }).overrides).toEqual({ size: 20 })
  })
})

describe('isSurfaceEnabled', () => {
  it('is false for every surface when the config is null', () => {
    expect(isSurfaceEnabled(null, 'main')).toBe(false)
    expect(isSurfaceEnabled(null, 'notemd.idea-spark')).toBe(false)
  })

  it('defaults main off and any unknown plugin surface on', () => {
    const cfg = normalizeConfig({ surfaces: {} })
    expect(isSurfaceEnabled(cfg, 'main')).toBe(false)
    expect(isSurfaceEnabled(cfg, 'notemd.somebody-new')).toBe(true)
  })

  it('honours an explicit flag either way', () => {
    const cfg = normalizeConfig({ surfaces: { main: true, 'notemd.idea-spark': false } })
    expect(isSurfaceEnabled(cfg, 'main')).toBe(true)
    expect(isSurfaceEnabled(cfg, 'notemd.idea-spark')).toBe(false)
  })
})

describe('resolveExplosion', () => {
  it('returns the preset when there are no overrides', () => {
    const cfg = normalizeConfig({ explosion: { presetId: 'coin' } })
    expect(resolveExplosion(cfg, 'B/')).toEqual(presetConfig('coin', 'B/'))
  })

  it('lets overrides win over the preset', () => {
    const cfg = normalizeConfig({ explosion: { presetId: 'coin' }, overrides: { size: 42, frequency: 1 } })
    const out = resolveExplosion(cfg, 'B/')
    expect(out.size).toBe(42)
    expect(out.frequency).toBe(1)
    expect(out.duration).toBe(1500) // 预设值原样保留
    expect(out.imageList).toEqual(presetConfig('coin', 'B/').imageList)
  })
})
```

- [ ] **Step 7: 跑测试确认失败**

Run: `pnpm test src/lib/power-mode/config.test.ts`
Expected: FAIL —— `Failed to resolve import "./config"`

- [ ] **Step 8: 写 config.ts**

Create `src/lib/power-mode/config.ts`:

```ts
import type { ExplosionConfig, PowerModeConfig, PresetId } from './types'
import { PRESET_PARAMS, presetConfig, assetBase } from './presets'

/**
 * 出厂默认。
 *
 * `surfaces.main` 默认关、插件窗口默认开:狂暴模式在主编辑窗口是干扰,在
 * 「随手写一条」的插件窗口里才是那点仪式感。装了插件就该看到效果,所以
 * 「配置从未写过」不等于「全关」——全关只由「插件没装/停用」表示(见 Task 6)。
 */
export const DEFAULT_CONFIG: PowerModeConfig = {
  surfaces: { main: false, 'notemd.idea-spark': true },
  shake: { enable: true, intensity: 5, recoverTime: 800 },
  combo: { enable: true, timeout: 10, showExclamation: true, precisionInput: false },
  explosion: { enable: true, presetId: 'particle' },
}

function obj(v: unknown): Record<string, unknown> {
  return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : {}
}
function bool(v: unknown, fallback: boolean): boolean {
  return typeof v === 'boolean' ? v : fallback
}
function num(v: unknown, fallback: number): number {
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback
}

/** 把磁盘/RPC 上来的任意 JSON 收敛成一份完整配置。永不抛。 */
export function normalizeConfig(raw: unknown): PowerModeConfig {
  const r = obj(raw)
  const shake = obj(r.shake)
  const combo = obj(r.combo)
  const explosion = obj(r.explosion)
  const presetId = explosion.presetId
  const overrides = r.overrides && typeof r.overrides === 'object' && !Array.isArray(r.overrides)
    ? (r.overrides as Partial<ExplosionConfig>)
    : undefined
  const surfaces: Record<string, boolean> = { ...DEFAULT_CONFIG.surfaces }
  for (const [k, v] of Object.entries(obj(r.surfaces))) {
    if (typeof v === 'boolean') surfaces[k] = v
  }
  return {
    surfaces,
    shake: {
      enable: bool(shake.enable, DEFAULT_CONFIG.shake.enable),
      intensity: num(shake.intensity, DEFAULT_CONFIG.shake.intensity),
      recoverTime: num(shake.recoverTime, DEFAULT_CONFIG.shake.recoverTime),
    },
    combo: {
      enable: bool(combo.enable, DEFAULT_CONFIG.combo.enable),
      timeout: num(combo.timeout, DEFAULT_CONFIG.combo.timeout),
      showExclamation: bool(combo.showExclamation, DEFAULT_CONFIG.combo.showExclamation),
      precisionInput: bool(combo.precisionInput, DEFAULT_CONFIG.combo.precisionInput),
    },
    explosion: {
      enable: bool(explosion.enable, DEFAULT_CONFIG.explosion.enable),
      presetId: (typeof presetId === 'string' && presetId in PRESET_PARAMS)
        ? (presetId as PresetId)
        : DEFAULT_CONFIG.explosion.presetId,
    },
    ...(overrides ? { overrides } : {}),
  }
}

/**
 * 某个生效面是否开着。
 *
 * `cfg === null` = 插件没装/被停用 → 一律关。未列出的插件窗口默认开(与
 * Idea Spark 一致),未列出的 'main' 默认关。
 */
export function isSurfaceEnabled(cfg: PowerModeConfig | null, surfaceId: string): boolean {
  if (!cfg) return false
  const explicit = cfg.surfaces[surfaceId]
  if (typeof explicit === 'boolean') return explicit
  return surfaceId !== 'main'
}

/** 预设 + 用户覆写 = 实际用于渲染的爆炸配置。 */
export function resolveExplosion(cfg: PowerModeConfig, base: string = assetBase()): ExplosionConfig {
  return { ...presetConfig(cfg.explosion.presetId, base), ...(cfg.overrides ?? {}) }
}
```

- [ ] **Step 9: 跑测试确认通过**

Run: `pnpm test src/lib/power-mode/config.test.ts`
Expected: PASS（全部用例）

- [ ] **Step 10: Commit**

```bash
git add src/lib/power-mode/types.ts src/lib/power-mode/presets.ts src/lib/power-mode/config.ts src/lib/power-mode/config.test.ts
git commit -m "$(cat <<'EOF'
feat(power-mode): 引擎类型、4 个预设常量与配置合并

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2b: 运行时样式

**Files:**
- Create: `src/styles/power-mode.css`
- Modify: `src/styles/editor-base.css`（文件末尾加一行 `@import`）

**Interfaces:**
- Produces: class `.power-mode-combo` / `.power-mode-combo-text` / `.power-mode-combo-progress` / `.power-mode-combo-exclamation` / `.power-mode-overlay` / `.power-mode-explosion` / `.power-mode-explosion-mask` / `.power-mode-explosion-image`，Task 3/4 直接用这些名字。

- [ ] **Step 1: 写样式**

Create `src/styles/power-mode.css`:

```css
/* Power Mode 运行时样式。移植自 ~/git/obsidian-power-mode/styles.css 前 42 行
   (设置面板那 74 行随面板重写丢弃)。

   由 editor-base.css @import 引入 —— 那个文件主窗口 (App.svelte) 和 Editor Kit
   (kit.css) 都吃,所以两个生效面用同一份样式,不需要各自接一次。 */

.power-mode-overlay {
  position: fixed;
  inset: 0;
  pointer-events: none;
  /* 低于模态框/弹窗层。note.md 的对话框在 1000 以上。 */
  z-index: 60;
  overflow: hidden;
}

.power-mode-combo {
  position: fixed;
  display: none;
  right: 6%;
  top: 120px;
  font-weight: 900;
  pointer-events: none;
  z-index: 61;
  font-size: 40px;
  flex-direction: column;
  align-items: flex-end;
  font-family: monospace;
}

.power-mode-combo-progress {
  height: 12px;
  background-color: white;
  border: solid 1px white;
}

.power-mode-combo-text {
  color: white;
}

.power-mode-combo-exclamation {
  font-size: 24px;
}

.power-mode-explosion {
  position: absolute;
  pointer-events: none;
  transform: translateX(-50%);
}

.power-mode-explosion-mask {
  background-color: currentColor;
  -webkit-mask-repeat: no-repeat;
  -webkit-mask-size: contain;
  mask-repeat: no-repeat;
  mask-size: contain;
  filter: saturate(150%);
}

.power-mode-explosion-image {
  background-repeat: no-repeat;
  background-size: contain;
}
```

- [ ] **Step 2: 挂到共享样式上**

Modify `src/styles/editor-base.css` — 在**文件最末尾**追加：

```css

/* Power Mode 的运行时样式。放在这里而不是各挂一次,是因为本文件正好是主窗口
   (App.svelte) 与 Editor Kit (kit.css) 唯一共用的样式入口。 */
@import './power-mode.css';
```

> ⚠️ CSS 的 `@import` 必须出现在其它规则之前才符合标准,但 Vite 在构建时会把
> `@import` 内联展开,顺序不受影响。如果 `pnpm build` 报 `@import must precede
> all other statements`,改为放在 `editor-base.css` 的**文件开头**(现有 `@import`
> 之后)。

- [ ] **Step 3: 构建确认样式进了两个产物**

Run:
```bash
pnpm build && grep -c "power-mode-explosion" dist/assets/editor-kit-v1.css && grep -rl "power-mode-explosion" dist/assets/*.css
```
Expected: `editor-kit-v1.css` 里命中 ≥1；主窗口的 `index-*.css` 也在列表里。

- [ ] **Step 4: Commit**

```bash
git add src/styles/power-mode.css src/styles/editor-base.css
git commit -m "$(cat <<'EOF'
feat(power-mode): 运行时样式,经 editor-base 同时供主窗口与 Editor Kit

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: 震动与连击

**Files:**
- Create: `src/lib/power-mode/shaker.ts`
- Create: `src/lib/power-mode/combo.ts`
- Test: `src/lib/power-mode/shaker.test.ts`
- Test: `src/lib/power-mode/combo.test.ts`

**Interfaces:**
- Consumes: `PowerModeConfig`（Task 2）
- Produces:
  - `randomOffset(intensity: number, rnd?: () => number): { x: number; y: number }`
  - `createShaker(el: HTMLElement, rnd?: () => number): { shake(cfg: PowerModeConfig): void; destroy(): void }`
  - `shouldCount(precisionInput: boolean, prev: number | undefined, cur: number): boolean`
  - `comboColor(count: number): string`
  - `createCombo(root: HTMLElement, rnd?: () => number): { hit(cfg: PowerModeConfig, docSize: number, docKey: string): void; destroy(): void }`

- [ ] **Step 1: 写震动的失败测试**

Create `src/lib/power-mode/shaker.test.ts`:

```ts
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { randomOffset, createShaker } from './shaker'
import { normalizeConfig } from './config'

describe('randomOffset', () => {
  it('maps rnd() 0 / 0.5 / 1 onto -i / 0 / +i', () => {
    expect(randomOffset(5, () => 0)).toEqual({ x: -5, y: -5 })
    expect(randomOffset(5, () => 0.5)).toEqual({ x: 0, y: 0 })
    expect(randomOffset(4, () => 1)).toEqual({ x: 4, y: 4 })
  })
})

describe('createShaker', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('translates on shake and clears after recoverTime', () => {
    const el = document.createElement('div')
    const s = createShaker(el, () => 1)
    s.shake(normalizeConfig({ shake: { intensity: 3, recoverTime: 800 } }))
    expect(el.style.transform).toBe('translate3d(3px, 3px, 0)')
    vi.advanceTimersByTime(799)
    expect(el.style.transform).toBe('translate3d(3px, 3px, 0)')
    vi.advanceTimersByTime(1)
    expect(el.style.transform).toBe('')
    s.destroy()
  })

  it('does nothing when shake is disabled', () => {
    const el = document.createElement('div')
    const s = createShaker(el, () => 1)
    s.shake(normalizeConfig({ shake: { enable: false } }))
    expect(el.style.transform).toBe('')
    s.destroy()
  })

  it('destroy clears the pending recovery and resets the transform', () => {
    const el = document.createElement('div')
    const s = createShaker(el, () => 1)
    s.shake(normalizeConfig({}))
    s.destroy()
    expect(el.style.transform).toBe('')
    // 定时器已被取消:再走完也不该抛
    vi.advanceTimersByTime(5000)
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test src/lib/power-mode/shaker.test.ts`
Expected: FAIL —— `Failed to resolve import "./shaker"`

- [ ] **Step 3: 写 shaker.ts**

Create `src/lib/power-mode/shaker.ts`:

```ts
import type { PowerModeConfig } from './types'

/**
 * 位移量。lodash 的 `random(-i, i)` 是**整数**均匀分布,这里用连续值:视觉上
 * 没差别,而且省一个依赖。
 */
export function randomOffset(intensity: number, rnd: () => number = Math.random): { x: number; y: number } {
  const pick = () => Number(((rnd() * 2 - 1) * intensity).toFixed(4))
  return { x: pick(), y: pick() }
}

export interface Shaker {
  shake(cfg: PowerModeConfig): void
  destroy(): void
}

/**
 * 编辑区抖动。只做 CSS transform —— 整窗口物理震动(`setPosition`)是 async IPC,
 * 逐键调用掉帧,本项目明确不做。
 */
export function createShaker(el: HTMLElement, rnd: () => number = Math.random): Shaker {
  let timer: ReturnType<typeof setTimeout> | undefined

  const clear = () => {
    if (timer !== undefined) {
      clearTimeout(timer)
      timer = undefined
    }
  }

  return {
    shake(cfg) {
      if (!cfg.shake.enable) return
      clear()
      const { x, y } = randomOffset(cfg.shake.intensity, rnd)
      el.style.transform = `translate3d(${x}px, ${y}px, 0)`
      timer = setTimeout(() => {
        el.style.transform = ''
        timer = undefined
      }, cfg.shake.recoverTime)
    },
    destroy() {
      clear()
      el.style.transform = ''
    },
  }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test src/lib/power-mode/shaker.test.ts`
Expected: PASS（4 个用例）

- [ ] **Step 5: 写连击的失败测试**

Create `src/lib/power-mode/combo.test.ts`:

```ts
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { shouldCount, comboColor, createCombo } from './combo'
import { normalizeConfig } from './config'

describe('shouldCount', () => {
  it('always counts when precisionInput is off', () => {
    expect(shouldCount(false, undefined, 10)).toBe(true)
    expect(shouldCount(false, 20, 10)).toBe(true)
  })

  it('counts only non-shrinking edits when precisionInput is on', () => {
    expect(shouldCount(true, 10, 11)).toBe(true)
    expect(shouldCount(true, 10, 10)).toBe(true)
    expect(shouldCount(true, 11, 10)).toBe(false)
  })

  it('skips the very first edit of a document (no baseline yet)', () => {
    expect(shouldCount(true, undefined, 10)).toBe(false)
  })
})

describe('comboColor', () => {
  it('walks the hue down as the streak grows', () => {
    expect(comboColor(1)).toBe('hsl(198.8, 100%, 70%)')
    expect(comboColor(10)).toBe('hsl(188, 100%, 70%)')
  })
})

describe('createCombo', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => { vi.useRealTimers(); document.body.innerHTML = '' })

  const cfg = (over = {}) => normalizeConfig({ combo: { timeout: 10, ...over } })

  it('renders the counter into the root and increments per hit', () => {
    const c = createCombo(document.body, () => 0.9)
    c.hit(cfg(), 1, 'doc')
    const text = document.body.querySelector('.power-mode-combo-text')!
    expect(text.textContent).toBe('1×')
    c.hit(cfg(), 2, 'doc')
    expect(text.textContent).toBe('2×')
    c.destroy()
  })

  it('hides and resets after the timeout', () => {
    const c = createCombo(document.body, () => 0.9)
    c.hit(cfg(), 1, 'doc')
    const el = document.body.querySelector('.power-mode-combo') as HTMLElement
    expect(el.style.display).toBe('flex')
    vi.advanceTimersByTime(10_000)
    expect(el.style.display).toBe('none')
    c.hit(cfg(), 2, 'doc')
    expect(document.body.querySelector('.power-mode-combo-text')!.textContent).toBe('1×')
    c.destroy()
  })

  it('emits an exclamation every 10 hits when enabled', () => {
    const c = createCombo(document.body, () => 0)
    for (let i = 1; i <= 9; i++) c.hit(cfg(), i, 'doc')
    expect(document.body.querySelectorAll('.power-mode-combo-exclamation')).toHaveLength(0)
    c.hit(cfg(), 10, 'doc')
    expect(document.body.querySelectorAll('.power-mode-combo-exclamation')).toHaveLength(1)
    c.destroy()
  })

  it('never emits an exclamation when showExclamation is off', () => {
    const c = createCombo(document.body, () => 0)
    for (let i = 1; i <= 10; i++) c.hit(cfg({ showExclamation: false }), i, 'doc')
    expect(document.body.querySelectorAll('.power-mode-combo-exclamation')).toHaveLength(0)
    c.destroy()
  })

  it('keeps a per-document length baseline for precisionInput', () => {
    const p = cfg({ precisionInput: true })
    const c = createCombo(document.body, () => 0.9)
    c.hit(p, 100, 'a')          // 建立基线,不计数
    c.hit(p, 101, 'a')          // 变长 → 计数
    expect(document.body.querySelector('.power-mode-combo-text')!.textContent).toBe('1×')
    c.hit(p, 50, 'b')           // 换文档,重新建基线,不计数
    expect(document.body.querySelector('.power-mode-combo-text')!.textContent).toBe('1×')
    c.hit(p, 40, 'b')           // 变短 → 不计数
    expect(document.body.querySelector('.power-mode-combo-text')!.textContent).toBe('1×')
    c.destroy()
  })

  it('destroy removes the counter from the DOM', () => {
    const c = createCombo(document.body, () => 0.9)
    c.hit(cfg(), 1, 'doc')
    c.destroy()
    expect(document.body.querySelector('.power-mode-combo')).toBeNull()
  })

  it('does nothing when combo is disabled', () => {
    const c = createCombo(document.body, () => 0.9)
    c.hit(normalizeConfig({ combo: { enable: false } }), 1, 'doc')
    expect(document.body.querySelector('.power-mode-combo')).toBeNull()
    c.destroy()
  })
})
```

- [ ] **Step 6: 跑测试确认失败**

Run: `pnpm test src/lib/power-mode/combo.test.ts`
Expected: FAIL —— `Failed to resolve import "./combo"`

- [ ] **Step 7: 写 combo.ts**

Create `src/lib/power-mode/combo.ts`:

```ts
import type { PowerModeConfig } from './types'

/** 移植自源项目 combo.ts。刻意不做 i18n:这是游戏音效性质的彩蛋,不是 UI 文案。 */
export const EXCLAMATIONS: readonly string[] = [
  'Super!', 'Fantastic!', 'Great!', 'OMG', 'Whoah!', ':O', 'Nice!',
  'Splendid!', 'Grand!', 'Impressive!', 'Stupendous!', 'Extreme!', 'Awesome!',
]

/**
 * 这一次编辑是否该记进连击。
 *
 * `precisionInput` 打开时只认「文档没变短」的编辑(删除不算连击)。文档第一次
 * 被编辑时还没有基线,源项目在这种情况下不计数 —— 保持一致。
 */
export function shouldCount(precisionInput: boolean, prev: number | undefined, cur: number): boolean {
  if (!precisionInput) return true
  return prev !== undefined && prev <= cur
}

/** 连击色:连得越久越偏青。 */
export function comboColor(count: number): string {
  return `hsl(${200 - count * 1.2}, 100%, 70%)`
}

export interface Combo {
  hit(cfg: PowerModeConfig, docSize: number, docKey: string): void
  destroy(): void
}

/**
 * 右上角连击计数器。
 *
 * 每个引擎实例一份 —— 源项目用的是模块级单例,主窗口 + 若干 Kit 窗口同时存在
 * 时会互相串号。
 *
 * 所有 `animate()` 都写成可选调用:jsdom 没有 Web Animations API。
 */
export function createCombo(root: HTMLElement, rnd: () => number = Math.random): Combo {
  let count = 0
  let timer: ReturnType<typeof setTimeout> | undefined
  let flickerTimer: ReturnType<typeof setTimeout> | undefined
  let el: HTMLElement | undefined
  let textEl: HTMLElement
  let progressEl: HTMLElement
  const lengthMap = new Map<string, number>()

  function ensure(): void {
    if (el) return
    el = document.createElement('div')
    el.className = 'power-mode-combo'
    textEl = document.createElement('div')
    textEl.className = 'power-mode-combo-text'
    progressEl = document.createElement('div')
    progressEl.className = 'power-mode-combo-progress'
    el.append(textEl, progressEl)
    root.appendChild(el)
  }

  function flickAnimate(target: HTMLElement): void {
    target.animate?.(
      [
        { opacity: 1, filter: 'invert(0)' },
        { opacity: 0.3, filter: 'invert(0.6)' },
        { opacity: 1, filter: 'invert(0)' },
      ],
      { duration: 30 },
    )
  }

  function flicker(): void {
    flickAnimate(progressEl)
    if (rnd() < 0.5) flickAnimate(textEl)
    flickerTimer = setTimeout(flicker, 100 + rnd() * 700)
  }

  function stopFlicker(): void {
    if (flickerTimer !== undefined) {
      clearTimeout(flickerTimer)
      flickerTimer = undefined
    }
  }

  function reset(): void {
    if (timer !== undefined) {
      clearTimeout(timer)
      timer = undefined
    }
    stopFlicker()
    count = 0
    if (el) el.style.display = 'none'
  }

  function exclaim(color: string): void {
    const node = document.createElement('div')
    node.className = 'power-mode-combo-exclamation'
    node.textContent = EXCLAMATIONS[Math.floor(rnd() * EXCLAMATIONS.length)] ?? EXCLAMATIONS[0]
    node.style.color = color
    el!.appendChild(node)
    node.animate?.(
      [
        { transform: 'translate3d(0,0,0)', opacity: 1 },
        { transform: `translate3d(${Math.round((rnd() * 2 - 1) * 20)}%, 200%, 0)`, opacity: 0 },
      ],
      { duration: 2000 },
    )
    setTimeout(() => node.remove(), 2000)
  }

  function active(cfg: PowerModeConfig): void {
    count++
    if (count === 1) flicker()
    el!.style.display = 'flex'
    const color = comboColor(count)
    textEl.style.textShadow = `0 0 15px ${color}, 0 1px ${color}, 1px 0 ${color}, 0 -1px ${color}, -1px 0 ${color}`
    textEl.textContent = `${count}×`
    progressEl.style.boxShadow = `0 0 15px ${color}`
    progressEl.style.borderColor = color
    progressEl.style.width = `${count * 10}%`
    progressEl.animate?.([{ width: '80px' }, { width: '0px' }], { duration: cfg.combo.timeout * 1000 })
    textEl.animate?.([{ transform: 'scale(1.5)' }, { transform: 'scale(1)' }], { duration: 150 })
    if (cfg.combo.showExclamation && count % 10 === 0) exclaim(color)
    if (timer !== undefined) clearTimeout(timer)
    timer = setTimeout(reset, cfg.combo.timeout * 1000)
  }

  return {
    hit(cfg, docSize, docKey) {
      if (!cfg.combo.enable) return
      if (shouldCount(cfg.combo.precisionInput, lengthMap.get(docKey), docSize)) {
        ensure()
        active(cfg)
      }
      lengthMap.set(docKey, docSize)
    },
    destroy() {
      reset()
      el?.remove()
      el = undefined
      lengthMap.clear()
    },
  }
}
```

- [ ] **Step 8: 跑测试确认通过**

Run: `pnpm test src/lib/power-mode/combo.test.ts`
Expected: PASS（全部用例）

- [ ] **Step 9: Commit**

```bash
git add src/lib/power-mode/shaker.ts src/lib/power-mode/shaker.test.ts src/lib/power-mode/combo.ts src/lib/power-mode/combo.test.ts
git commit -m "$(cat <<'EOF'
feat(power-mode): 屏幕震动与连击计数器(每实例独立状态)

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: 全屏 overlay 与光标爆炸

**Files:**
- Create: `src/lib/power-mode/overlay.ts`
- Create: `src/lib/power-mode/explosion.ts`
- Test: `src/lib/power-mode/overlay.test.ts`
- Test: `src/lib/power-mode/explosion.test.ts`

**Interfaces:**
- Consumes: `ExplosionConfig`（Task 2）、`.power-mode-overlay` / `.power-mode-explosion*` 样式（Task 2b）
- Produces:
  - `acquireOverlay(root?: HTMLElement): HTMLElement`
  - `releaseOverlay(root?: HTMLElement): void`
  - `pickImage(list: string[], order: ExplosionConfig['explosionOrder'], count: number, rnd?: () => number): string`
  - `restartUrl(url: string, ts: number): string`
  - `preloadFrames(list: string[]): void`
  - `createExploder(overlay: HTMLElement, rnd?: () => number, now?: () => number): { fire(left: number, top: number, cfg: ExplosionConfig): void; destroy(): void }`

- [ ] **Step 1: 写 overlay 的失败测试**

Create `src/lib/power-mode/overlay.test.ts`:

```ts
import { describe, it, expect, afterEach } from 'vitest'
import { acquireOverlay, releaseOverlay } from './overlay'

describe('overlay', () => {
  afterEach(() => { document.body.innerHTML = '' })

  it('creates one node and hands the same one to every caller', () => {
    const a = acquireOverlay()
    const b = acquireOverlay()
    expect(a).toBe(b)
    expect(document.querySelectorAll('.power-mode-overlay')).toHaveLength(1)
    releaseOverlay(); releaseOverlay()
  })

  it('removes the node only when the last holder releases it', () => {
    acquireOverlay(); acquireOverlay()
    releaseOverlay()
    expect(document.querySelector('.power-mode-overlay')).not.toBeNull()
    releaseOverlay()
    expect(document.querySelector('.power-mode-overlay')).toBeNull()
  })

  it('ignores a release with no outstanding acquire', () => {
    releaseOverlay()
    expect(document.querySelector('.power-mode-overlay')).toBeNull()
    // 之后仍然能正常创建
    acquireOverlay()
    expect(document.querySelector('.power-mode-overlay')).not.toBeNull()
    releaseOverlay()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test src/lib/power-mode/overlay.test.ts`
Expected: FAIL —— `Failed to resolve import "./overlay"`

- [ ] **Step 3: 写 overlay.ts**

Create `src/lib/power-mode/overlay.ts`:

```ts
/**
 * 爆炸特效的宿主容器。
 *
 * 全屏 `position: fixed`,直接吃 `view.coordsAtPos()` 的视口坐标 —— 源项目把
 * div 插进编辑器容器再减 `getScrollInfo().top` 做修正,那套修正与 Obsidian
 * `coordsAtPos(pos, true)` 的 local 语义耦合,ProseMirror 没有对应模式。
 *
 * 一个窗口里可能同时有多个编辑器实例(主窗口的编辑器、Kit 实例),共用一个
 * overlay,用引用计数决定何时摘掉。
 */
const OVERLAY_CLASS = 'power-mode-overlay'

let node: HTMLElement | null = null
let holders = 0

export function acquireOverlay(root: HTMLElement = document.body): HTMLElement {
  if (!node || !node.isConnected) {
    node = document.createElement('div')
    node.className = OVERLAY_CLASS
    root.appendChild(node)
  }
  holders++
  return node
}

export function releaseOverlay(_root: HTMLElement = document.body): void {
  if (holders === 0) return
  holders--
  if (holders === 0 && node) {
    node.remove()
    node = null
  }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test src/lib/power-mode/overlay.test.ts`
Expected: PASS（3 个用例）

- [ ] **Step 5: 写爆炸的失败测试**

Create `src/lib/power-mode/explosion.test.ts`:

```ts
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { pickImage, restartUrl, createExploder } from './explosion'
import { presetConfig } from './presets'
import type { ExplosionConfig } from './types'

describe('pickImage', () => {
  const list = ['a', 'b', 'c']

  it('random picks by rnd()', () => {
    expect(pickImage(list, 'random', 0, () => 0)).toBe('a')
    expect(pickImage(list, 'random', 0, () => 0.99)).toBe('c')
  })

  it('sequential walks the list by the tick count', () => {
    expect(pickImage(list, 'sequential', 0)).toBe('a')
    expect(pickImage(list, 'sequential', 4)).toBe('b')
  })

  it('a numeric order pins one frame, out-of-range falls back to the first', () => {
    expect(pickImage(list, 1, 7)).toBe('b')
    expect(pickImage(list, 9, 7)).toBe('a')
  })
})

describe('restartUrl', () => {
  it('appends a cache-busting stamp with the right separator', () => {
    expect(restartUrl('http://x/a.gif', 42)).toBe('http://x/a.gif?t=42')
    expect(restartUrl('http://x/a.gif?v=1', 42)).toBe('http://x/a.gif?v=1&t=42')
  })
})

describe('createExploder', () => {
  let overlay: HTMLElement
  beforeEach(() => {
    vi.useFakeTimers()
    overlay = document.createElement('div')
    document.body.appendChild(overlay)
  })
  afterEach(() => { vi.useRealTimers(); document.body.innerHTML = '' })

  const cfg = (over: Partial<ExplosionConfig> = {}): ExplosionConfig =>
    ({ ...presetConfig('coin', 'B/'), ...over })

  it('places a layer at the given viewport coordinates', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    e.fire(100, 50, cfg())
    const el = overlay.querySelector('.power-mode-explosion') as HTMLElement
    expect(el).not.toBeNull()
    expect(el.style.left).toBe('100px')
    expect(el.style.top).toBe('50px')
    expect(el.style.width).toBe('8ch')
    expect(el.style.height).toBe('8rem')
    expect(el.style.marginTop).toBe('-5.28rem')   // -offset(0.66) * size(8)
    e.destroy()
  })

  it('uses backgroundImage in image mode and maskImage in mask mode', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    e.fire(0, 0, cfg({ backgroundMode: 'image', gifMode: 'continue', imageList: ['B/x.gif'] }))
    const img = overlay.querySelector('.power-mode-explosion') as HTMLElement
    expect(img.classList.contains('power-mode-explosion-image')).toBe(true)
    expect(img.style.backgroundImage).toBe('url(B/x.gif)')

    e.fire(0, 0, cfg({ backgroundMode: 'mask', gifMode: 'continue', imageList: ['B/y.gif'] }))
    const mask = overlay.querySelectorAll('.power-mode-explosion')[1] as HTMLElement
    expect(mask.classList.contains('power-mode-explosion-mask')).toBe(true)
    expect(mask.style.maskImage).toBe('url(B/y.gif)')
    e.destroy()
  })

  it('stamps the url in restart mode only', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    e.fire(0, 0, cfg({ gifMode: 'restart', imageList: ['B/x.gif'] }))
    expect((overlay.firstElementChild as HTMLElement).style.backgroundImage).toBe('url(B/x.gif?t=7)')
    e.destroy()
  })

  it('applies customStyle', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    e.fire(0, 0, cfg({ customStyle: { mixBlendMode: 'color-dodge' } }))
    expect((overlay.firstElementChild as HTMLElement).style.mixBlendMode).toBe('color-dodge')
    e.destroy()
  })

  it('caps the live layers at maxExplosions, dropping the oldest', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    for (let i = 0; i < 5; i++) e.fire(i, 0, cfg({ maxExplosions: 2, duration: 10_000 }))
    expect(overlay.querySelectorAll('.power-mode-explosion')).toHaveLength(2)
    e.destroy()
  })

  it('removes a layer when its duration expires, freeing its slot', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    // 上游 bug:index 为 0 (最旧那个) 时不从数组里摘,陈旧条目占着
    // maxExplosions 名额,后来的活跃特效被提前裁掉。这里钉死修正后的行为。
    e.fire(0, 0, cfg({ maxExplosions: 3, duration: 100 }))
    vi.advanceTimersByTime(100)
    expect(overlay.querySelectorAll('.power-mode-explosion')).toHaveLength(0)
    for (let i = 0; i < 3; i++) e.fire(i, 0, cfg({ maxExplosions: 3, duration: 10_000 }))
    expect(overlay.querySelectorAll('.power-mode-explosion')).toHaveLength(3)
    e.destroy()
  })

  it('does nothing when the image list is empty', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    e.fire(0, 0, cfg({ imageList: [] }))
    expect(overlay.children).toHaveLength(0)
    e.destroy()
  })

  it('destroy clears every live layer and its timer', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    e.fire(0, 0, cfg({ duration: 10_000 }))
    e.destroy()
    expect(overlay.children).toHaveLength(0)
    vi.advanceTimersByTime(20_000)
  })
})
```

- [ ] **Step 6: 跑测试确认失败**

Run: `pnpm test src/lib/power-mode/explosion.test.ts`
Expected: FAIL —— `Failed to resolve import "./explosion"`

- [ ] **Step 7: 写 explosion.ts**

Create `src/lib/power-mode/explosion.ts`:

```ts
import type { ExplosionConfig } from './types'

/** 按 explosionOrder 选一帧。`count` 是本引擎实例迄今的触发次数。 */
export function pickImage(
  list: string[],
  order: ExplosionConfig['explosionOrder'],
  count: number,
  rnd: () => number = Math.random,
): string {
  if (order === 'random') return list[Math.floor(rnd() * list.length)] ?? list[0]
  if (order === 'sequential') return list[count % list.length]
  return list[order] ?? list[0]
}

/**
 * 让 GIF 从头播:加一个时间戳查询参数,换一个新的资源 URL。
 *
 * 源项目对 base64 是往字符串里插 `t=…;base64,`(一个 hack);本项目素材是外部
 * 文件,查询参数就够了。
 */
export function restartUrl(url: string, ts: number): string {
  return `${url}${url.includes('?') ? '&' : '?'}t=${ts}`
}

/** `gifMode: 'continue'` 依赖素材已在缓存里,否则第一次触发只看得到白框。 */
export function preloadFrames(list: string[]): void {
  if (typeof Image === 'undefined') return
  for (const src of list) {
    const img = new Image()
    img.src = src
  }
}

export interface Exploder {
  /** `left`/`top` 是视口坐标,直接来自 `view.coordsAtPos()`。 */
  fire(left: number, top: number, cfg: ExplosionConfig): void
  destroy(): void
}

export function createExploder(
  overlay: HTMLElement,
  rnd: () => number = Math.random,
  now: () => number = Date.now,
): Exploder {
  let count = -1
  const active: { el: HTMLElement; clock: ReturnType<typeof setTimeout> }[] = []
  const preloaded = new Set<string>()

  function drop(entry: { el: HTMLElement; clock: ReturnType<typeof setTimeout> }): void {
    const i = active.indexOf(entry)
    // 上游写的是 `if (index > 0)`,最旧那个(index 0)永远摘不掉,陈旧条目
    // 占着 maxExplosions 名额。`>= 0` 才对 —— 这里用 indexOf + splice 直接表达。
    if (i >= 0) active.splice(i, 1)
    entry.el.remove()
    clearTimeout(entry.clock)
  }

  return {
    fire(left, top, cfg) {
      count++
      if (cfg.imageList.length === 0) return

      if (cfg.gifMode === 'continue') {
        const cold = cfg.imageList.filter((u) => !preloaded.has(u))
        if (cold.length) {
          preloadFrames(cold)
          for (const u of cold) preloaded.add(u)
        }
      }

      const el = document.createElement('div')
      el.classList.add('power-mode-explosion', `power-mode-explosion-${cfg.backgroundMode}`)
      el.style.left = `${left}px`
      el.style.top = `${top}px`
      el.style.width = `${cfg.size}ch`
      el.style.height = `${cfg.size}rem`
      el.style.marginTop = `${-(cfg.offset || 0) * cfg.size}rem`

      let url = pickImage(cfg.imageList, cfg.explosionOrder, count, rnd)
      if (cfg.gifMode === 'restart') url = restartUrl(url, now())
      if (cfg.backgroundMode === 'image') {
        el.style.backgroundImage = `url(${url})`
      } else {
        el.style.webkitMaskImage = `url(${url})`
        el.style.maskImage = `url(${url})`
      }
      for (const [k, v] of Object.entries(cfg.customStyle ?? {})) {
        el.style.setProperty(k.replace(/[A-Z]/g, (m) => `-${m.toLowerCase()}`), v)
      }

      overlay.appendChild(el)
      const entry = { el, clock: setTimeout(() => drop(entry), cfg.duration) }
      active.push(entry)

      while (cfg.maxExplosions > 0 && active.length > cfg.maxExplosions) {
        const oldest = active[0]
        if (!oldest) break
        drop(oldest)
      }
    },
    destroy() {
      while (active.length) drop(active[0]!)
      preloaded.clear()
    },
  }
}
```

- [ ] **Step 8: 跑测试确认通过**

Run: `pnpm test src/lib/power-mode/explosion.test.ts`
Expected: PASS（全部用例）

> 若 `mixBlendMode` 那条挂了：jsdom 对 `style.setProperty('mix-blend-mode', …)` 支持正常，但 `el.style.mixBlendMode` 的读回依赖驼峰↔连字符映射。断言改读 `el.style.getPropertyValue('mix-blend-mode')` 即可，实现不用改。

- [ ] **Step 9: Commit**

```bash
git add src/lib/power-mode/overlay.ts src/lib/power-mode/overlay.test.ts src/lib/power-mode/explosion.ts src/lib/power-mode/explosion.test.ts
git commit -m "$(cat <<'EOF'
feat(power-mode): 全屏 overlay 与光标爆炸(修上游 index>0 的清理缺陷)

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: ProseMirror 插件入口 + 主窗口接线

**Files:**
- Create: `src/lib/power-mode/plugin.ts`
- Create: `src/lib/power-mode/plugin.test.ts`
- Create: `src/lib/power-mode/host-config.svelte.ts`
- Modify: `src/lib/settings.svelte.ts`（加两个点号安全的读写函数）
- Test: `src/lib/settings-plugin-scope.test.ts`
- Modify: `src/components/RichEditor.svelte`（挂载点 A）
- Modify: `src/App.svelte`（启动时初始化配置）
- Test: `src/lib/power-mode/plugin.test.ts`

**Interfaces:**
- Consumes: Task 2/3/4 全部导出。
- Produces:
  - `createRuntime(getConfig: ConfigGetter, deps: RuntimeDeps): PowerModeRuntime`
  - `powerModePlugin(getConfig: ConfigGetter, docKey?: () => string): Plugin`
  - `type ConfigGetter = () => PowerModeConfig | null`
  - `initPowerModeHost(): Promise<void>`、`mainWindowConfig(): PowerModeConfig | null`
  - `getPluginScopedValue(pluginId: string, key: string): unknown`
  - `setPluginScopedValue(pluginId: string, key: string, value: unknown): Promise<void>`

- [ ] **Step 1: 写 runtime 的失败测试**

Create `src/lib/power-mode/plugin.test.ts`:

```ts
import { describe, it, expect, vi } from 'vitest'
import { createRuntime } from './plugin'
import { normalizeConfig } from './config'
import type { PowerModeConfig } from './types'

function deps() {
  return {
    shaker: { shake: vi.fn(), destroy: vi.fn() },
    combo: { hit: vi.fn(), destroy: vi.fn() },
    exploder: { fire: vi.fn(), destroy: vi.fn() },
    coords: vi.fn(() => ({ left: 11, top: 22 })),
    docSize: vi.fn(() => 100),
    docKey: vi.fn(() => 'doc-1'),
    assetBase: 'B/',
  }
}

describe('createRuntime', () => {
  const on = (over: Partial<PowerModeConfig> = {}) => normalizeConfig({ ...over })

  it('drives all three effects on a tick', () => {
    const d = deps()
    createRuntime(() => on(), d).tick()
    expect(d.shaker.shake).toHaveBeenCalledTimes(1)
    expect(d.combo.hit).toHaveBeenCalledWith(expect.anything(), 100, 'doc-1')
    expect(d.exploder.fire).toHaveBeenCalledWith(11, 22, expect.objectContaining({ size: 10 }))
  })

  it('does nothing at all when the config is null', () => {
    const d = deps()
    createRuntime(() => null, d).tick()
    expect(d.shaker.shake).not.toHaveBeenCalled()
    expect(d.combo.hit).not.toHaveBeenCalled()
    expect(d.exploder.fire).not.toHaveBeenCalled()
    expect(d.coords).not.toHaveBeenCalled()
  })

  it('gates the explosion by frequency but never the shake or combo', () => {
    const d = deps()
    // coin: frequency 4 → 第 1、5 次触发
    const rt = createRuntime(() => on({ explosion: { enable: true, presetId: 'coin' } }), d)
    for (let i = 0; i < 5; i++) rt.tick()
    expect(d.shaker.shake).toHaveBeenCalledTimes(5)
    expect(d.combo.hit).toHaveBeenCalledTimes(5)
    expect(d.exploder.fire).toHaveBeenCalledTimes(2)
  })

  it('skips each effect its own switch turns off', () => {
    const d = deps()
    createRuntime(() => normalizeConfig({
      shake: { enable: false }, combo: { enable: false }, explosion: { enable: false },
    }), d).tick()
    // shake/combo 的开关由各自模块内部判定,这里只钉「爆炸关了就不算坐标」
    expect(d.exploder.fire).not.toHaveBeenCalled()
    expect(d.coords).not.toHaveBeenCalled()
  })

  it('survives a coords lookup that throws', () => {
    const d = deps()
    d.coords.mockImplementation(() => { throw new Error('no layout') })
    expect(() => createRuntime(() => on(), d).tick()).not.toThrow()
    expect(d.shaker.shake).toHaveBeenCalledTimes(1)
  })

  it('destroy tears down every effect exactly once', () => {
    const d = deps()
    const rt = createRuntime(() => on(), d)
    rt.destroy()
    rt.destroy()
    expect(d.shaker.destroy).toHaveBeenCalledTimes(1)
    expect(d.combo.destroy).toHaveBeenCalledTimes(1)
    expect(d.exploder.destroy).toHaveBeenCalledTimes(1)
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test src/lib/power-mode/plugin.test.ts`
Expected: FAIL —— `Failed to resolve import "./plugin"`

- [ ] **Step 3: 写 plugin.ts**

Create `src/lib/power-mode/plugin.ts`:

```ts
import { Plugin, PluginKey } from 'prosemirror-state'
import type { EditorView } from 'prosemirror-view'
import type { PowerModeConfig } from './types'
import { resolveExplosion } from './config'
import { assetBase } from './presets'
import { createShaker, type Shaker } from './shaker'
import { createCombo, type Combo } from './combo'
import { createExploder, type Exploder } from './explosion'
import { acquireOverlay, releaseOverlay } from './overlay'

export type ConfigGetter = () => PowerModeConfig | null

export const powerModeKey = new PluginKey('powerMode')

export interface RuntimeDeps {
  shaker: Pick<Shaker, 'shake' | 'destroy'>
  combo: Pick<Combo, 'hit' | 'destroy'>
  exploder: Pick<Exploder, 'fire' | 'destroy'>
  /** 光标在视口里的位置。抛异常时静默跳过爆炸(布局还没稳时会抛)。 */
  coords: () => { left: number; top: number }
  docSize: () => number
  docKey: () => string
  assetBase: string
}

export interface PowerModeRuntime {
  tick(): void
  destroy(): void
}

/**
 * 一次文档变更 = 一次 tick。
 *
 * 计数器是**每实例**的:源项目用模块级 `count`,主窗口 + 若干 Kit 实例并存时
 * frequency 门控会互相串。
 */
export function createRuntime(getConfig: ConfigGetter, deps: RuntimeDeps): PowerModeRuntime {
  let count = -1
  let dead = false

  return {
    tick() {
      if (dead) return
      const cfg = getConfig()
      if (!cfg) return
      count++
      deps.shaker.shake(cfg)
      deps.combo.hit(cfg, deps.docSize(), deps.docKey())
      if (!cfg.explosion.enable) return
      const explosion = resolveExplosion(cfg, deps.assetBase)
      if (count % Math.max(1, explosion.frequency) !== 0) return
      try {
        const { left, top } = deps.coords()
        deps.exploder.fire(left, top, explosion)
      } catch {
        // coordsAtPos 在布局尚未成型时会抛;跳过这一发,别把输入链路带崩。
      }
    },
    destroy() {
      if (dead) return
      dead = true
      deps.shaker.destroy()
      deps.combo.destroy()
      deps.exploder.destroy()
    },
  }
}

/**
 * 把引擎接到一个 ProseMirror 编辑器上。
 *
 * `getConfig` 返回 null = 这个生效面关着(判定在调用方,引擎不认识生效面)。
 * `docKey` 用于 precisionInput 的每文档长度基线;主窗口传文件路径,Kit 传实例 id。
 */
export function powerModePlugin(getConfig: ConfigGetter, docKey: () => string = () => 'default'): Plugin {
  return new Plugin({
    key: powerModeKey,
    view(view: EditorView) {
      const overlay = acquireOverlay()
      const rt = createRuntime(getConfig, {
        shaker: createShaker(view.dom.parentElement ?? view.dom),
        combo: createCombo(document.body),
        exploder: createExploder(overlay),
        coords: () => {
          const c = view.coordsAtPos(view.state.selection.head)
          return { left: c.left, top: c.top }
        },
        docSize: () => view.state.doc.content.size,
        docKey,
        assetBase: assetBase(),
      })
      return {
        update(v, prevState) {
          if (v.state.doc.eq(prevState.doc)) return
          rt.tick()
        },
        destroy() {
          rt.destroy()
          releaseOverlay()
        },
      }
    },
  })
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test src/lib/power-mode/plugin.test.ts`
Expected: PASS（6 个用例）

- [ ] **Step 5: 给 settings 加点号安全的读写**

Modify `src/lib/settings.svelte.ts` — 在 `mergePluginScoped` 之后追加：

```ts
/**
 * 读某个插件域下的一个键。
 *
 * 与 `getPluginScopedKey(fqKey)` 的区别:插件 id 单独传。后者按**第一个点**切
 * 分,对 v2 的 `publisher.name` 形状 id(如 `notemd.power-mode`)会切成
 * `notemd`,读到别处去。新代码一律用这个。
 */
export function getPluginScopedValue(pluginId: string, key: string): unknown {
  return pluginScoped[pluginId]?.[key]
}

/** 写某个插件域下的一个键并落盘。点号安全,理由同 `getPluginScopedValue`。 */
export async function setPluginScopedValue(pluginId: string, key: string, value: unknown): Promise<void> {
  if (!pluginScoped[pluginId]) pluginScoped[pluginId] = {}
  pluginScoped[pluginId][key] = value
  pluginScopedVersion.value++
  await saveSettings()
}
```

- [ ] **Step 6: 为点号安全写测试**

Create `src/lib/settings-plugin-scope.test.ts`（独立文件，不动既有的 `settings.test.ts`，免得踩它对 Tauri store 的 mock 布置）：

```ts
import { describe, it, expect } from 'vitest'
import { getPluginScopedKey } from './settings.svelte'

describe('dotted plugin ids', () => {
  it('documents why getPluginScopedKey must not be used for v2 ids', () => {
    // 这不是「期望的行为」,是钉住已知陷阱:fq key 按第一个点切分,
    // v2 id 一律含点,所以新代码用 getPluginScopedValue(pluginId, key)。
    expect(getPluginScopedKey('notemd.power-mode.config')).toBeUndefined()
  })
})
```

- [ ] **Step 7: 跑测试**

Run: `pnpm test src/lib/settings`
Expected: PASS

- [ ] **Step 8: 写主窗口的配置源**

Create `src/lib/power-mode/host-config.svelte.ts`:

```ts
// 主窗口专用的 Power Mode 配置源。
//
// ⚠️ 这个文件碰 Tauri IPC,Editor Kit **绝不能** import 它 —— 插件 webview 没有
// IPC,引入即炸掉整个 kit。Kit 侧的配置源是 src/editor-kit/power-mode-config.ts。
import { listen } from '@tauri-apps/api/event'
import { getPluginScopedValue, setPluginScopedValue } from '../settings.svelte'
import { normalizeConfig, isSurfaceEnabled } from './config'
import type { PowerModeConfig } from './types'

export const POWER_MODE_PLUGIN_ID = 'notemd.power-mode'
const CONFIG_KEY = 'config'

/** null = 插件没装/停用,或配置从未写过。 */
let cached: PowerModeConfig | null = null

/** 主编辑窗口该用的配置;生效面关着时返回 null。 */
export function mainWindowConfig(): PowerModeConfig | null {
  return isSurfaceEnabled(cached, 'main') ? cached : null
}

function hydrate(): void {
  const raw = getPluginScopedValue(POWER_MODE_PLUGIN_ID, CONFIG_KEY)
  cached = raw === undefined ? null : normalizeConfig(raw)
}

/**
 * 启动时调一次。
 *
 * 插件窗口没有 Tauri IPC,它的写入走 `host.power_mode.update` → 宿主 emit
 * `power-mode://update` → 这里落盘。settings.json 由主窗口独家持有,所以写入
 * 必须回到这一侧,不能让 Rust 直接改文件。
 */
export async function initPowerModeHost(): Promise<void> {
  hydrate()
  await listen<unknown>('power-mode://update', async (e) => {
    const next = normalizeConfig(e.payload)
    cached = next
    try {
      await setPluginScopedValue(POWER_MODE_PLUGIN_ID, CONFIG_KEY, next)
    } catch (err) {
      console.warn('[power-mode] persist failed:', err)
    }
  })
}
```

- [ ] **Step 9: 接主窗口启动**

Modify `src/App.svelte` — 在 `await loadSettings()` 那一段里，`loadLocale()` 之后插一行（`src/App.svelte:235` 附近）：

```ts
      try { await loadLocale() } catch (e) { console.warn('[App] loadLocale:', e) }
      // Power Mode:配置从 settings 插件域读,并监听插件窗口经 RPC 推来的更新。
      try {
        await (await import('./lib/power-mode/host-config.svelte')).initPowerModeHost()
      } catch (e) { console.warn('[App] initPowerModeHost:', e) }
```

- [ ] **Step 10: 挂主窗口编辑器**

Modify `src/components/RichEditor.svelte` — 在追加 wikilink 等插件的那个 `try` 块里（`src/components/RichEditor.svelte:1007-1032`），把 power-mode 插件加进 `concat` 列表。改动两处：

在 `const { adoptAnswer } = await import('../lib/note-anno/adopt-answer')` 之后加：

```ts
          const { powerModePlugin } = await import('../lib/power-mode/plugin')
          const { mainWindowConfig } = await import('../lib/power-mode/host-config.svelte')
```

在 `placeholderPlugin(t('editor.emptyPlaceholder')),` 之后加：

```ts
                // Power Mode:getter 每次击键现取,配置改了下一次输入就生效;
                // 生效面关着时返回 null,引擎整条链路直接短路。
                powerModePlugin(mainWindowConfig, () => tab.filePath ?? 'untitled'),
```

- [ ] **Step 11: 类型检查 + 全量测试**

Run: `pnpm check && pnpm test`
Expected: 两条都通过（`pnpm check` 允许有既存的历史告警，但不得出现 `power-mode` 相关的新错误）

- [ ] **Step 12: 手动验证主窗口（需临时打开开关）**

Run:
```bash
node -e "
const fs=require('fs');const p=process.env.HOME+'/Library/Application Support/net.notemd.app/settings.json';
const s=JSON.parse(fs.readFileSync(p,'utf8'));
s.plugins=s.plugins||{};
s.plugins['notemd.power-mode']={config:{surfaces:{main:true}}};
fs.writeFileSync(p,JSON.stringify(s,null,2)+'\n');console.log('main surface ON');
"
pnpm tauri dev
```
Expected: 打开任意 `.md`，在富文本模式下敲字 → 编辑区轻微抖动、右上角出现 `1× 2× 3×` 连击、光标处冒出 particle 粒子。source 模式下什么都不发生（预期）。

> 验证完把这段临时配置删掉：同样的脚本把 `s.plugins['notemd.power-mode']` 删掉即可。真正的开关由 Task 8 的插件窗口提供。

- [ ] **Step 13: Commit**

```bash
git add src/lib/power-mode/plugin.ts src/lib/power-mode/plugin.test.ts src/lib/power-mode/host-config.svelte.ts src/lib/settings.svelte.ts src/lib/settings-plugin-scope.test.ts src/components/RichEditor.svelte src/App.svelte
git commit -m "$(cat <<'EOF'
feat(power-mode): ProseMirror 插件入口与主编辑窗口接线

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: 两条 host RPC

**Files:**
- Create: `src-tauri/src/plugin_runtime/power_mode.rs`
- Modify: `src-tauri/src/plugin_runtime/mod.rs`（注册模块）
- Modify: `src-tauri/src/plugin_runtime/host_api.rs`（capability 表）
- Modify: `src-tauri/src/plugin_runtime/ui_rpc.rs`（dispatch 接线）
- Modify: `docs/plugin-v2-development.md`（§5 能力表 + §6 方法表）

**Interfaces:**
- Consumes: Task 5 的 `power-mode://update` 事件契约、settings.json 的 `plugins.<id>.config` 布局。
- Produces:
  - RPC `host.power_mode.config`（capability `editor.kit`）→ `{ config: object|null, surfaces: [{ id, name, names }] }`
  - RPC `host.power_mode.update`（capability `power-mode`）→ `{ ok: true }`
  - Rust: `power_mode::PLUGIN_ID`、`power_mode::PluginBrief`、`power_mode::effective()`、`power_mode::surfaces()`、`power_mode::config_from_settings()`

- [ ] **Step 1: 写 Rust 纯逻辑的失败测试 + 实现**

Create `src-tauri/src/plugin_runtime/power_mode.rs`:

```rust
//! `host.power_mode.*` —— Power Mode 的配置读写通道。
//!
//! 为什么需要它:特效引擎跑在宿主里(主窗口编辑器 + 下发给插件窗口的 Editor
//! Kit),而配置由 power-mode 插件的窗口编辑。插件窗口是隔离 webview,既没有
//! Tauri IPC 也够不到 settings.json,只能经这两条 RPC 走。
//!
//! 读侧返回的是**生效后**的值:插件没装/被停用就直接 `null`,所以不需要在卸载
//! 路径上补「清理残留配置」的钩子 —— 卸了就不炸。

use serde_json::{json, Value};

/// 本插件的 id。settings.json 里的键、生效面清单的排除项都用它。
pub const PLUGIN_ID: &str = "notemd.power-mode";

/// 一个已加载插件的最小画像,只取判定生效面需要的字段。
#[derive(Debug, Clone)]
pub struct PluginBrief {
    pub id: String,
    pub name: String,
    pub capabilities: Vec<String>,
    /// manifest 的 `i18n` 原样透传(形如 `{"zh": {"name": "…"}}`)。
    pub i18n: Option<Value>,
}

/// 从 settings.json 的整份 JSON 里取出 `plugins.<PLUGIN_ID>.config`。
pub fn config_from_settings(settings: &Value) -> Option<Value> {
    settings.get("plugins")?.get(PLUGIN_ID)?.get("config").cloned()
}

/// 生效后的配置。
///
/// - 插件没装/停用 → `null`(前端据此整体关闭)
/// - 装了但从没配过 → `{}`(前端用默认值:Idea Spark 开、主窗口关)
pub fn effective(installed: bool, settings: &Value) -> Value {
    if !installed {
        return Value::Null;
    }
    config_from_settings(settings).unwrap_or_else(|| json!({}))
}

/// 可作为生效面的插件:已加载、声明了 `editor.kit`、且不是 power-mode 自己
/// (它自己的窗口是实操区,不受生效面开关管)。
///
/// `names` 是 manifest `i18n.<locale>.name` 的映射;插件 UI 按自己的 locale 挑。
pub fn surfaces(plugins: &[PluginBrief]) -> Vec<Value> {
    let mut out: Vec<Value> = plugins
        .iter()
        .filter(|p| p.id != PLUGIN_ID)
        .filter(|p| p.capabilities.iter().any(|c| c == "editor.kit"))
        .map(|p| {
            let mut names = serde_json::Map::new();
            if let Some(Value::Object(map)) = p.i18n.as_ref() {
                for (locale, entry) in map {
                    if let Some(Value::String(n)) = entry.get("name") {
                        names.insert(locale.clone(), Value::String(n.clone()));
                    }
                }
            }
            json!({ "id": p.id, "name": p.name, "names": Value::Object(names) })
        })
        .collect();
    out.sort_by(|a, b| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brief(id: &str, caps: &[&str], i18n: Option<Value>) -> PluginBrief {
        PluginBrief {
            id: id.into(),
            name: id.into(),
            capabilities: caps.iter().map(|s| s.to_string()).collect(),
            i18n,
        }
    }

    #[test]
    fn effective_is_null_when_the_plugin_is_not_loaded() {
        let settings = json!({ "plugins": { PLUGIN_ID: { "config": { "surfaces": { "main": true } } } } });
        assert_eq!(effective(false, &settings), Value::Null);
    }

    #[test]
    fn effective_is_empty_object_when_installed_but_never_configured() {
        assert_eq!(effective(true, &json!({})), json!({}));
        assert_eq!(effective(true, &json!({ "plugins": {} })), json!({}));
    }

    #[test]
    fn effective_returns_the_stored_config_verbatim() {
        let settings = json!({ "plugins": { PLUGIN_ID: { "config": { "surfaces": { "main": true } } } } });
        assert_eq!(effective(true, &settings), json!({ "surfaces": { "main": true } }));
    }

    #[test]
    fn surfaces_keeps_only_editor_kit_plugins_and_drops_power_mode_itself() {
        let list = vec![
            brief("notemd.idea-spark", &["editor.kit", "vault.read"], None),
            brief("notemd.roam-import", &["vault.read"], None),
            brief(PLUGIN_ID, &["editor.kit", "power-mode"], None),
        ];
        let out = surfaces(&list);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], "notemd.idea-spark");
    }

    #[test]
    fn surfaces_carries_the_localized_names() {
        let list = vec![brief(
            "notemd.idea-spark",
            &["editor.kit"],
            Some(json!({ "zh": { "name": "奇思妙想" }, "ja": { "name": "アイデアスパーク" } })),
        )];
        let out = surfaces(&list);
        assert_eq!(out[0]["names"]["zh"], "奇思妙想");
        assert_eq!(out[0]["names"]["ja"], "アイデアスパーク");
        assert_eq!(out[0]["name"], "notemd.idea-spark");
    }

    #[test]
    fn surfaces_is_sorted_by_id_for_a_stable_ui_order() {
        let list = vec![
            brief("notemd.zeta", &["editor.kit"], None),
            brief("notemd.alpha", &["editor.kit"], None),
        ];
        let out = surfaces(&list);
        assert_eq!(out[0]["id"], "notemd.alpha");
        assert_eq!(out[1]["id"], "notemd.zeta");
    }
}
```

- [ ] **Step 2: 注册模块**

Modify `src-tauri/src/plugin_runtime/mod.rs` — 模块声明是字母序的（`src-tauri/src/plugin_runtime/mod.rs:6-19`），在 `pub mod market;` 与 `pub mod process;` 之间插入：

```rust
pub mod power_mode;
```

- [ ] **Step 3: 跑 Rust 测试确认通过**

Run: `cd src-tauri && cargo test -p notemd power_mode`
Expected: 6 个新用例 PASS（若 crate 名不是 `notemd`，去掉 `-p`，直接 `cargo test power_mode`）

- [ ] **Step 4: 加 capability 表条目**

Modify `src-tauri/src/plugin_runtime/host_api.rs` — 在 `method_capability()` 里 `"host.theme.css" => Some("editor.kit"),` 之后加：

```rust
        // Power Mode 配置通道。读侧挂 editor.kit —— 需要它的正是内嵌了 Editor Kit
        // 的插件窗口;写侧单独一个 token,只有 power-mode 插件声明。两条都只在
        // UI 桥可用(在 ui_rpc::dispatch 里单独处理,进程通道回 -32601)。
        "host.power_mode.config" => Some("editor.kit"),
        "host.power_mode.update" => Some("power-mode"),
```

- [ ] **Step 5: 接 dispatch**

Modify `src-tauri/src/plugin_runtime/ui_rpc.rs` — 在处理 `host.theme.css` 的那个 `if` 块之后（`src-tauri/src/plugin_runtime/ui_rpc.rs:292-297` 附近）加：

```rust
    // host.power_mode.* 与 theme.css 同理:要活的 AppHandle(读 app 配置目录下的
    // settings.json、枚举已加载插件、向主窗口 emit),而注入式 HostServices 刻意
    // 不带 AppHandle。gate 用同一张能力表,手工施加。
    if req.method == "host.power_mode.config" || req.method == "host.power_mode.update" {
        if let Some(denial) = capability_denial(&req.method, capabilities, req.id) {
            return denial;
        }
        return if req.method == "host.power_mode.config" {
            ok(req.id, power_mode_payload(app))
        } else {
            match power_mode_update(app, &req.params) {
                Ok(v) => ok(req.id, v),
                Err(detail) => err(req.id, proto::ERR_INTERNAL, detail),
            }
        };
    }
```

在同一文件的「Method bodies」区（`fn editor_open` 附近）加两个函数：

```rust
/// `host.power_mode.config` —— 生效后的配置 + 可配置的生效面清单。
fn power_mode_payload<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> serde_json::Value {
    use super::power_mode::{self, PluginBrief};
    use tauri::Manager;

    // settings.json 由主窗口前端(tauri-plugin-store)独家持有。这里只读:写入
    // 走 power_mode_update → emit → 前端落盘,避免两个写者打架。
    let settings = app
        .path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("settings.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let (installed, briefs) = match super::STATE.read() {
        Ok(st) => {
            let installed = st.plugins.contains_key(power_mode::PLUGIN_ID);
            let briefs: Vec<PluginBrief> = st
                .plugins
                .iter()
                .map(|(id, (manifest, _dir))| PluginBrief {
                    id: id.clone(),
                    name: manifest.name.clone(),
                    capabilities: manifest.capabilities.clone(),
                    i18n: manifest.i18n.clone(),
                })
                .collect();
            (installed, briefs)
        }
        Err(_) => (false, Vec::new()),
    };

    serde_json::json!({
        "config": power_mode::effective(installed, &settings),
        "surfaces": power_mode::surfaces(&briefs),
    })
}

/// `host.power_mode.update` —— 把新配置转给主窗口前端落盘。
fn power_mode_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Emitter;
    let cfg = params
        .get("config")
        .filter(|v| v.is_object())
        .ok_or_else(|| "bad_request: config must be an object".to_string())?;
    app.emit_to("main", "power-mode://update", cfg.clone())
        .map_err(|e| format!("io: emit failed: {e}"))?;
    Ok(serde_json::json!({ "ok": true }))
}
```

> 字段类型已核实（`plugin-protocol/src/lib.rs:15-39`）：`name: String`、`capabilities: Vec<String>`、`i18n: Option<serde_json::Value>`，上面的构造可直接用。`STATE.plugins` 是 `BTreeMap<String, (ManifestV2, PathBuf)>`（`src-tauri/src/plugin_runtime/mod.rs:23-32`），本来就按 id 有序 —— `surfaces()` 里的排序是为了让那个纯函数自成一体、可单测，不是多余的兜底。

- [ ] **Step 6: 写 capability 拒绝的测试**

Append to `src-tauri/src/plugin_runtime/ui_rpc.rs` 的 `#[cfg(test)] mod tests`（照文件里 `editor_open_resolves_vault_path_and_records_call` 的 `run(...)` 写法）：

```rust
    #[tokio::test]
    async fn power_mode_config_requires_editor_kit() {
        let s = StubServices::default();
        let r = run(&s, &["vault.read"], "host.power_mode.config", serde_json::json!({})).await;
        let e = r.error.expect("expected a denial");
        assert_eq!(e.code, proto::ERR_CAPABILITY_DENIED);
        assert!(e.message.contains("editor.kit"), "{}", e.message);
    }

    #[tokio::test]
    async fn power_mode_update_requires_its_own_token() {
        let s = StubServices::default();
        let r = run(&s, &["editor.kit"], "host.power_mode.update", serde_json::json!({})).await;
        let e = r.error.expect("expected a denial");
        assert_eq!(e.code, proto::ERR_CAPABILITY_DENIED);
        assert!(e.message.contains("power-mode"), "{}", e.message);
    }
```

> 这两条走的是 `dispatch_with`（注入式 services），因此只验证能力门；带 AppHandle 的那半（`power_mode_payload` / `power_mode_update`）由 `power_mode.rs` 的纯函数测试覆盖 + Task 9 的实机验证。

- [ ] **Step 7: 跑 Rust 测试**

Run: `cd src-tauri && cargo test`
Expected: 全绿（含 8 个新用例）

- [ ] **Step 8: 更新开发规范文档**

Modify `docs/plugin-v2-development.md`：

在 §5 的 capability 表末尾加一行：

```
| `power-mode` | `host.power_mode.update`(写 Power Mode 配置)。读侧 `host.power_mode.config` 挂在 `editor.kit` 下 —— 需要读它的正是内嵌 Editor Kit 的插件窗口 |
```

在 §6 的「其它」方法表末尾加两行：

```
| `host.power_mode.config` | `editor.kit` | — → `{ config: object\|null, surfaces: [{id, name, names}] }`;`config: null` = power-mode 插件没装/停用(整体关闭),`{}` = 装了但没配过(用默认值)。仅 UI 桥可用 |
| `host.power_mode.update` | `power-mode` | `{ config }` → `{ ok: true }`;宿主转给主窗口前端落进 settings.json 的插件域。仅 UI 桥可用 |
```

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/plugin_runtime/power_mode.rs src-tauri/src/plugin_runtime/mod.rs src-tauri/src/plugin_runtime/host_api.rs src-tauri/src/plugin_runtime/ui_rpc.rs docs/plugin-v2-development.md
git commit -m "$(cat <<'EOF'
feat(power-mode): host.power_mode.config/update 两条 RPC 与能力门

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: Editor Kit 接入

**Files:**
- Create: `src/editor-kit/power-mode-config.ts`
- Create: `src/editor-kit/power-mode-config.test.ts`
- Modify: `src/editor-kit/main.ts`（`KitOptions.powerMode` + `KitEditor.setPowerMode`）
- Modify: `src/editor-kit/rich.ts`（挂插件）
- Modify: `src/editor-kit/main.test.ts`（补断言）

**Interfaces:**
- Consumes: Task 5 的 `powerModePlugin`、Task 2 的 `normalizeConfig` / `isSurfaceEnabled`、Task 6 的 `host.power_mode.config`。
- Produces:
  - `loadSurfaceConfig(): Promise<PowerModeConfig | null>`
  - `watchSurfaceFocus(reload: () => void): () => void`
  - `KitOptions.powerMode?: PowerModeConfig | null`
  - `KitEditor.setPowerMode(cfg: PowerModeConfig | null): void`
  - `mountRich(host, initial, vaultRoot, onChange, placeholder, getPowerMode?)`

- [ ] **Step 1: 写 Kit 配置源的失败测试**

Create `src/editor-kit/power-mode-config.test.ts`:

```ts
import { describe, it, expect, vi, afterEach } from 'vitest'
import { loadSurfaceConfig, watchSurfaceFocus } from './power-mode-config'

function stubBridge(pluginId: string, result: unknown) {
  const request = vi.fn().mockResolvedValue(result)
  ;(window as any).notemd = { pluginId, locale: 'zh', theme: 'x', request, onMessage() {} }
  return request
}

describe('loadSurfaceConfig', () => {
  afterEach(() => { delete (window as any).notemd })

  it('returns null with no bridge (kit mounted outside a plugin window)', async () => {
    expect(await loadSurfaceConfig()).toBeNull()
  })

  it('returns null when the host reports the plugin is off', async () => {
    stubBridge('notemd.idea-spark', { config: null, surfaces: [] })
    expect(await loadSurfaceConfig()).toBeNull()
  })

  it('defaults an installed-but-unconfigured host to enabled for a plugin window', async () => {
    stubBridge('notemd.idea-spark', { config: {}, surfaces: [] })
    const cfg = await loadSurfaceConfig()
    expect(cfg).not.toBeNull()
    expect(cfg!.explosion.presetId).toBe('particle')
  })

  it('honours an explicit off flag for this very window', async () => {
    stubBridge('notemd.idea-spark', { config: { surfaces: { 'notemd.idea-spark': false } }, surfaces: [] })
    expect(await loadSurfaceConfig()).toBeNull()
  })

  it('returns null instead of throwing when the RPC fails', async () => {
    const request = vi.fn().mockRejectedValue(new Error('nope'))
    ;(window as any).notemd = { pluginId: 'x', locale: 'en', theme: 'y', request, onMessage() {} }
    expect(await loadSurfaceConfig()).toBeNull()
  })
})

describe('watchSurfaceFocus', () => {
  it('reloads on window focus and stops after unsubscribe', () => {
    const reload = vi.fn()
    const off = watchSurfaceFocus(reload)
    window.dispatchEvent(new Event('focus'))
    expect(reload).toHaveBeenCalledTimes(1)
    off()
    window.dispatchEvent(new Event('focus'))
    expect(reload).toHaveBeenCalledTimes(1)
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test src/editor-kit/power-mode-config.test.ts`
Expected: FAIL —— `Failed to resolve import "./power-mode-config"`

- [ ] **Step 3: 写 power-mode-config.ts**

Create `src/editor-kit/power-mode-config.ts`:

```ts
// Kit 侧的 Power Mode 配置源。
//
// 与主窗口的 host-config.svelte.ts 是两条独立通道:插件 webview 没有 Tauri IPC,
// 只能走 window.notemd 桥。这个文件因此不 import 任何 @tauri-apps/*。
import { normalizeConfig, isSurfaceEnabled } from '../lib/power-mode/config'
import type { PowerModeConfig } from '../lib/power-mode/types'

interface HostBridge {
  pluginId: string
  request(method: string, params?: unknown): Promise<any>
}

function bridge(): HostBridge | null {
  const b = (window as unknown as { notemd?: HostBridge }).notemd
  return b && typeof b.request === 'function' ? b : null
}

/**
 * 本窗口该用的配置,已算过生效面;不该开时返回 null。
 *
 * 任何失败(宿主太老没这条 RPC、插件没声明 editor.kit、桥不在)都降级成 null:
 * 特效是装饰,不该把编辑器拖下水。
 */
export async function loadSurfaceConfig(): Promise<PowerModeConfig | null> {
  const b = bridge()
  if (!b) return null
  try {
    const res = await b.request('host.power_mode.config')
    const raw = (res as { config?: unknown } | null)?.config
    if (raw === null || raw === undefined) return null
    const cfg = normalizeConfig(raw)
    return isSurfaceEnabled(cfg, b.pluginId) ? cfg : null
  } catch {
    return null
  }
}

/**
 * 插件窗口收不到 Tauri 的 `settings://changed` 广播(没有 IPC),所以用「窗口重新
 * 获得焦点」当刷新时机:用户在 Power Mode 设置窗口改完,切回来就是新的。
 */
export function watchSurfaceFocus(reload: () => void): () => void {
  window.addEventListener('focus', reload)
  return () => window.removeEventListener('focus', reload)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test src/editor-kit/power-mode-config.test.ts`
Expected: PASS（6 个用例）

- [ ] **Step 5: 让 rich.ts 挂插件**

Modify `src/editor-kit/rich.ts`：

在 import 区加：

```ts
import { powerModePlugin, type ConfigGetter } from '../lib/power-mode/plugin'
```

把 `mountRich` 的签名与插件追加改成：

```ts
export async function mountRich(
  host: HTMLElement,
  initial: string,
  vaultRoot: string,
  onChange: (md: string) => void,
  placeholder?: string,
  getPowerMode?: ConfigGetter,
): Promise<MorayaEditorInstance> {
```

以及函数末尾那段 `const extra = richPlugins(placeholder)` 改为：

```ts
  // Append the placeholder plugin after mount, same construction as the main
  // window's editor-append plugins in RichEditor.svelte (`view.updateState(
  // view.state.reconfigure(...))`).
  //
  // Power Mode 也在这里接:getter 每次击键现取,所以 setPowerMode() 换配置不需要
  // 重挂编辑器(重挂会丢光标、选区和撤销栈)。
  const extra: Plugin[] = richPlugins(placeholder)
  if (getPowerMode) extra.push(powerModePlugin(getPowerMode, () => 'kit'))
  if (extra.length) {
```

- [ ] **Step 6: 给 main.ts 加 API**

Modify `src/editor-kit/main.ts`：

（a）在 import 区加：

```ts
import { loadSurfaceConfig, watchSurfaceFocus } from './power-mode-config'
import type { PowerModeConfig } from '../lib/power-mode/types'
```

（b）`KitEditor` 接口里，`focus(): void` 之前加：

```ts
  /**
   * 换掉特效配置,不重挂编辑器。
   *
   * v1 是「不改既有成员、可以加成员」的冻结口径,`setPlaceholder` 当年也是这么
   * 进来的 —— 加一个成员不影响任何既有消费方。
   */
  setPowerMode(cfg: PowerModeConfig | null): void
```

（c）`KitOptions` 接口里，`baseDir?: string` 之后加：

```ts
  /**
   * 特效配置。
   *
   * **省略**(默认)= kit 自己向宿主要 `host.power_mode.config`,按本窗口的插件 id
   * 判定生效面,并在窗口重新获得焦点时重拉 —— Idea Spark 因此零改动就生效。
   *
   * **显式给值**(含 `null`)= 调用方自管,不看生效面、不自动刷新。Power Mode 插件
   * 自己的实操区走这条:改一格滑块就 setPowerMode() 推一次。
   */
  powerMode?: PowerModeConfig | null
```

（d）`mountMarkdownEditor` 里，`let placeholder = opts.placeholder` 之后加：

```ts
  // 显式给值 = 调用方自管;省略 = 走宿主配置 + focus 刷新。
  const selfManaged = 'powerMode' in opts
  let powerMode: PowerModeConfig | null = opts.powerMode ?? null
  let stopFocusWatch: (() => void) | null = null
  if (!selfManaged) {
    powerMode = await loadSurfaceConfig()
    stopFocusWatch = watchSurfaceFocus(() => {
      void loadSurfaceConfig().then((c) => { powerMode = c })
    })
  }
```

（e）`mountCurrent()` 里的 rich 分支改成：

```ts
    if (mode === 'rich') rich = await mountRich(host, markdown, root, emit, placeholder, () => powerMode)
```

（f）返回对象里，`focus:` 之前加：

```ts
    setPowerMode: (cfg) => { powerMode = cfg },
```

（g）`destroy()` 里，`flush()` 之后加：

```ts
      stopFocusWatch?.()
      stopFocusWatch = null
```

- [ ] **Step 7: 给 main.test.ts 补断言**

Append to `src/editor-kit/main.test.ts`（照文件现有的 mock 组织方式；关键是**不要**真的挂 ProseMirror）：

```ts
import { describe, it, expect } from 'vitest'

describe('KitOptions.powerMode contract', () => {
  it('distinguishes "omitted" from "explicit null"', () => {
    // 这条钉的是 main.ts 里 `'powerMode' in opts` 的判据:显式传 null 表示
    // 「调用方自管、别去问宿主」,与省略不是一回事。
    expect('powerMode' in ({ initialMarkdown: '' } as Record<string, unknown>)).toBe(false)
    expect('powerMode' in ({ initialMarkdown: '', powerMode: null } as Record<string, unknown>)).toBe(true)
  })
})
```

- [ ] **Step 8: 跑测试 + 类型检查 + 构建**

Run: `pnpm test src/editor-kit && pnpm check && pnpm build`
Expected: 全通过；`pnpm build` 末尾的 `check-editor-kit-build.mjs` 也过。

- [ ] **Step 9: 确认素材路径能被 Kit 寻址**

Run:
```bash
ls dist/assets/power-mode/particle/1.gif && grep -o "power-mode/" dist/assets/editor-kit-v1.js | head -1
```
Expected: 文件存在；`editor-kit-v1.js` 里出现 `power-mode/` 字面量（说明 `new URL('./power-mode/', import.meta.url)` 没有被 Vite 改写成绝对路径）。

> 若 `editor-kit-v1.js` 里出现的是 `/assets/power-mode/` 这种绝对路径，说明 `@vite-ignore` 没生效 —— 检查 `presets.ts` 里 `assetBase()` 的注释位置是否在 `new URL(` 的第一个实参前。

- [ ] **Step 10: Commit**

```bash
git add src/editor-kit/power-mode-config.ts src/editor-kit/power-mode-config.test.ts src/editor-kit/main.ts src/editor-kit/main.test.ts src/editor-kit/rich.ts
git commit -m "$(cat <<'EOF'
feat(power-mode): Editor Kit 接入(KitOptions.powerMode + setPowerMode)

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: 插件工程

**Files:**
- Create: `plugins-src/power-mode/{package.json, vite.config.ts, tsconfig.json, index.html, manifest.v2.json}`
- Create: `plugins-src/power-mode/src/{main.ts, App.svelte}`
- Create: `plugins-src/power-mode/src/lib/{bridge.ts, strings.ts, editor-kit.ts, config.ts, types.ts}`
- Test: `plugins-src/power-mode/src/lib/{strings.test.ts, config.test.ts}`
- Modify: `scripts/dev-install-plugin.sh`

**Interfaces:**
- Consumes: Task 6 的两条 RPC、Task 7 的 Kit API。
- Produces: 插件 `notemd.power-mode@1.0.0`，Plugins 菜单一条「狂暴模式」。

- [ ] **Step 1: 建工程骨架**

Create `plugins-src/power-mode/package.json`:

```json
{
  "name": "power-mode",
  "version": "1.0.0",
  "type": "module",
  "private": true,
  "scripts": {
    "build": "vite build",
    "check": "svelte-check --tsconfig ./tsconfig.json",
    "test": "vitest run"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "^5",
    "svelte": "^5",
    "svelte-check": "^4",
    "typescript": "^5",
    "vite": "^6",
    "vitest": "^4.1.5"
  }
}
```

Create `plugins-src/power-mode/vite.config.ts`（与 idea-spark 逐字相同）:

```ts
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Standalone plugin UI bundle. Served by the host under `plugin://<id>/…`, so
// asset URLs MUST be relative (`base: './'`).
export default defineConfig({
  plugins: [svelte()],
  base: './',
  build: {
    target: 'safari15',
    minify: 'esbuild',
    sourcemap: false,
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: { index: 'index.html' },
    },
  },
})
```

Create `plugins-src/power-mode/tsconfig.json` —— 逐字复制 `plugins-src/idea-spark/tsconfig.json`。

Create `plugins-src/power-mode/index.html`:

```html
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Power Mode</title>
  </head>
  <body>
    <div id="power-mode-app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

Create `plugins-src/power-mode/src/main.ts`:

```ts
import { mount } from 'svelte'
import App from './App.svelte'

const target = document.getElementById('power-mode-app')
if (!target) throw new Error('power-mode-app root missing')
mount(App, { target })
```

- [ ] **Step 2: 写 manifest**

Create `plugins-src/power-mode/manifest.v2.json`:

```json
{
  "manifest_version": 2,
  "id": "notemd.power-mode",
  "name": "Power Mode",
  "version": "1.0.0",
  "kind": "native",
  "engines": { "notemd": ">=6.805.3" },
  "description": "Cursor explosions, a combo meter and a little screen shake while you type.",
  "ui": "ui/",
  "activation": { "events": ["onCommand:open"] },
  "contributes": {
    "menus": [
      { "location": "plugins", "label": "Power Mode", "command": "open" }
    ],
    "windows": [
      {
        "id": "main",
        "entry": "index.html",
        "title": "Power Mode",
        "width": 620,
        "height": 760,
        "min_width": 520,
        "min_height": 600,
        "open_command": "open"
      }
    ]
  },
  "capabilities": ["editor.kit", "power-mode"],
  "i18n": {
    "zh": { "name": "狂暴模式", "menus": { "open": "狂暴模式" } },
    "ja": { "name": "パワーモード", "menus": { "open": "パワーモード" } },
    "de": { "name": "Power-Modus", "menus": { "open": "Power-Modus" } }
  }
}
```

> `engines.notemd` 填本次实际发布的宿主版本（日期版本号规则：`(年-2020).(月*100+日).(当日第几次)`）。Task 9 发布时若版本不同，回来改这一行。

- [ ] **Step 3: 复制 bridge + 配置模型**

Create `plugins-src/power-mode/src/lib/bridge.ts`:

```ts
// 照抄 plugins-src/idea-spark/src/lib/bridge.ts 的骨架,只保留本插件用到的方法。
export interface NotemdBridge {
  pluginId: string
  locale: string
  theme: string
  request(method: string, params?: unknown): Promise<any>
  onMessage(cb: (payload: unknown) => void): void
}

declare global {
  interface Window { notemd: NotemdBridge }
}

export function bridge(): NotemdBridge {
  const b = window.notemd
  if (!b) throw new Error('window.notemd bridge missing (not running inside a plugin window)')
  return b
}

export interface SurfaceEntry {
  id: string
  /** manifest 的英文名。 */
  name: string
  /** locale → 本地化名。 */
  names: Record<string, string>
}

export interface PowerModeConfigPayload {
  /** null = 插件没装/停用(理论上打不开本窗口);{} = 装了但没配过。 */
  config: Record<string, unknown> | null
  surfaces: SurfaceEntry[]
}

/** `host.power_mode.config` → 生效配置 + 可配置生效面清单。 */
export function loadPowerMode(): Promise<PowerModeConfigPayload> {
  return bridge().request('host.power_mode.config')
}

/** `host.power_mode.update` —— 宿主转给主窗口落进 settings.json 的插件域。 */
export function savePowerMode(config: unknown): Promise<{ ok: true }> {
  return bridge().request('host.power_mode.update', { config })
}
```

Create `plugins-src/power-mode/src/lib/types.ts` —— **逐字复制** `src/lib/power-mode/types.ts`，并在文件头加一行：

```ts
// ⚠️ 本文件是 src/lib/power-mode/types.ts 的副本。插件跑在隔离 webview 里,
// 不能 import 主程序的 src/(见 docs/plugin-v2-development.md §2)。
// 改动请两边同步 —— config.test.ts 的 parity 用例会在漂移时报警。
```

Create `plugins-src/power-mode/src/lib/config.ts` —— **逐字复制** `src/lib/power-mode/config.ts` 里的 `DEFAULT_CONFIG`、`normalizeConfig`、`isSurfaceEnabled` 及它们依赖的 `obj`/`bool`/`num` 三个私有辅助函数，**去掉** `resolveExplosion` 与对 `presets.ts` 的 import（插件不渲染特效，用不到预设参数），并把 presetId 的校验改成本地常量：

```ts
// ⚠️ 本文件是 src/lib/power-mode/config.ts 的裁剪副本(去掉 resolveExplosion 与
// presets 依赖:插件不渲染特效)。改动请与主程序同步 —— config.test.ts 的
// parity 用例会在 DEFAULT_CONFIG 漂移时报警。
import type { ExplosionConfig, PowerModeConfig, PresetId } from './types'

export const PRESET_IDS: readonly PresetId[] = ['particle', 'lightning', 'coin', 'confetti']
```

`normalizeConfig` 里把 `presetId in PRESET_PARAMS` 换成 `(PRESET_IDS as readonly string[]).includes(presetId)`，其余逐字保留。

- [ ] **Step 4: 写 parity 与 i18n 的失败测试**

Create `plugins-src/power-mode/src/lib/config.test.ts`:

```ts
import { describe, it, expect } from 'vitest'
import { DEFAULT_CONFIG, normalizeConfig, isSurfaceEnabled, PRESET_IDS } from './config'
// 主程序的原件。运行时不可 import(隔离 webview),但测试跑在 node 里,可以拿它
// 当漂移哨兵。
import { DEFAULT_CONFIG as HOST_DEFAULT } from '../../../../src/lib/power-mode/config'

describe('parity with the host copy', () => {
  it('keeps DEFAULT_CONFIG identical', () => {
    expect(DEFAULT_CONFIG).toEqual(HOST_DEFAULT)
  })
})

describe('config', () => {
  it('lists exactly the four shipped presets', () => {
    expect([...PRESET_IDS].sort()).toEqual(['coin', 'confetti', 'lightning', 'particle'])
  })

  it('normalizes junk to the defaults', () => {
    expect(normalizeConfig(null)).toEqual(DEFAULT_CONFIG)
  })

  it('defaults main off and plugin surfaces on', () => {
    const cfg = normalizeConfig({})
    expect(isSurfaceEnabled(cfg, 'main')).toBe(false)
    expect(isSurfaceEnabled(cfg, 'notemd.whatever')).toBe(true)
  })
})
```

Create `plugins-src/power-mode/src/lib/strings.test.ts`:

```ts
import { describe, it, expect } from 'vitest'
import { CATALOGS, t, setLocale, type MessageKey } from './strings'

describe('strings', () => {
  it('has every key in all four locales', () => {
    const keys = Object.keys(CATALOGS.en) as MessageKey[]
    expect(keys.length).toBeGreaterThan(10)
    for (const locale of ['zh', 'ja', 'de'] as const) {
      for (const k of keys) {
        expect(CATALOGS[locale][k], `${locale} missing ${k}`).toBeTruthy()
      }
    }
  })

  it('falls back to en for an unknown locale', () => {
    setLocale('fr')
    expect(t('title')).toBe(CATALOGS.en.title)
    setLocale('zh')
    expect(t('title')).toBe(CATALOGS.zh.title)
  })
})
```

- [ ] **Step 5: 跑测试确认失败**

Run: `pnpm --filter power-mode test`
Expected: FAIL —— 找不到 `./strings`（`config.test.ts` 也可能因 `./config` 未完成而失败）

- [ ] **Step 6: 写 strings.ts**

Create `plugins-src/power-mode/src/lib/strings.ts`:

```ts
// 插件自带 i18n。隔离 webview 用不了主程序的 t();结构照抄
// plugins-src/openclaw/src/lib/strings.ts。
export type MessageKey =
  | 'title' | 'surfaces.section' | 'surfaces.main' | 'surfaces.hint'
  | 'effects.section'
  | 'explosion.enable' | 'explosion.preset'
  | 'preset.particle' | 'preset.lightning' | 'preset.coin' | 'preset.confetti'
  | 'shake.enable' | 'shake.intensity' | 'shake.recoverTime'
  | 'combo.enable' | 'combo.timeout' | 'combo.showExclamation' | 'combo.precisionInput'
  | 'combo.precisionInput.hint'
  | 'demo.section' | 'demo.hint' | 'demo.sample' | 'demo.unavailable'
  | 'saved' | 'saveFailed'

type Catalog = Record<MessageKey, string>

const en: Catalog = {
  title: 'Power Mode',
  'surfaces.section': 'Where it applies',
  'surfaces.main': 'Main editor window',
  'surfaces.hint': 'Plugin windows that embed the host editor appear here automatically.',
  'effects.section': 'Effects',
  'explosion.enable': 'Cursor explosions',
  'explosion.preset': 'Preset',
  'preset.particle': 'Particle',
  'preset.lightning': 'Lightning',
  'preset.coin': 'Coin',
  'preset.confetti': 'Confetti',
  'shake.enable': 'Screen shake',
  'shake.intensity': 'Intensity',
  'shake.recoverTime': 'Recovery',
  'combo.enable': 'Combo meter',
  'combo.timeout': 'Timeout',
  'combo.showExclamation': 'Exclamations',
  'combo.precisionInput': 'Precision input',
  'combo.precisionInput.hint': 'Only count edits that do not shorten the document.',
  'demo.section': 'Try it',
  'demo.hint': 'Type here — settings above apply live, regardless of the switches.',
  'demo.sample': 'Type something and watch the sparks fly.',
  'demo.unavailable': 'The live preview needs a newer version of note.md.',
  saved: 'Saved',
  saveFailed: 'Could not save settings',
}

const zh: Catalog = {
  title: '狂暴模式',
  'surfaces.section': '生效范围',
  'surfaces.main': '主编辑窗口',
  'surfaces.hint': '内嵌宿主编辑器的插件窗口会自动出现在这里。',
  'effects.section': '特效',
  'explosion.enable': '光标爆炸',
  'explosion.preset': '预设',
  'preset.particle': '粒子',
  'preset.lightning': '闪电',
  'preset.coin': '金币',
  'preset.confetti': '彩纸',
  'shake.enable': '屏幕震动',
  'shake.intensity': '强度',
  'shake.recoverTime': '恢复时间',
  'combo.enable': '连击计数',
  'combo.timeout': '超时',
  'combo.showExclamation': '感叹词',
  'combo.precisionInput': '精确输入',
  'combo.precisionInput.hint': '只统计没让文档变短的编辑。',
  'demo.section': '试试看',
  'demo.hint': '在这里敲字 —— 上面的设置立刻生效,不受开关影响。',
  'demo.sample': '敲点什么,看看火花。',
  'demo.unavailable': '实操区需要更新版本的 note.md。',
  saved: '已保存',
  saveFailed: '设置保存失败',
}

const ja: Catalog = {
  title: 'パワーモード',
  'surfaces.section': '適用範囲',
  'surfaces.main': 'メインエディタウィンドウ',
  'surfaces.hint': 'ホストエディタを埋め込むプラグインウィンドウは自動的にここに表示されます。',
  'effects.section': 'エフェクト',
  'explosion.enable': 'カーソル爆発',
  'explosion.preset': 'プリセット',
  'preset.particle': 'パーティクル',
  'preset.lightning': 'ライトニング',
  'preset.coin': 'コイン',
  'preset.confetti': '紙吹雪',
  'shake.enable': '画面シェイク',
  'shake.intensity': '強さ',
  'shake.recoverTime': '復帰時間',
  'combo.enable': 'コンボカウンター',
  'combo.timeout': 'タイムアウト',
  'combo.showExclamation': '感嘆詞',
  'combo.precisionInput': '精密入力',
  'combo.precisionInput.hint': '文書が短くならない編集だけを数えます。',
  'demo.section': '試す',
  'demo.hint': 'ここで入力してください。上の設定がスイッチに関係なく即座に反映されます。',
  'demo.sample': '何か入力して火花を見てみましょう。',
  'demo.unavailable': 'ライブプレビューには新しいバージョンの note.md が必要です。',
  saved: '保存しました',
  saveFailed: '設定を保存できませんでした',
}

const de: Catalog = {
  title: 'Power-Modus',
  'surfaces.section': 'Geltungsbereich',
  'surfaces.main': 'Hauptfenster des Editors',
  'surfaces.hint': 'Plugin-Fenster mit eingebettetem Host-Editor erscheinen hier automatisch.',
  'effects.section': 'Effekte',
  'explosion.enable': 'Cursor-Explosionen',
  'explosion.preset': 'Voreinstellung',
  'preset.particle': 'Partikel',
  'preset.lightning': 'Blitz',
  'preset.coin': 'Münze',
  'preset.confetti': 'Konfetti',
  'shake.enable': 'Bildschirmwackeln',
  'shake.intensity': 'Stärke',
  'shake.recoverTime': 'Erholzeit',
  'combo.enable': 'Combo-Zähler',
  'combo.timeout': 'Zeitlimit',
  'combo.showExclamation': 'Ausrufe',
  'combo.precisionInput': 'Präzise Eingabe',
  'combo.precisionInput.hint': 'Nur Änderungen zählen, die das Dokument nicht kürzen.',
  'demo.section': 'Ausprobieren',
  'demo.hint': 'Hier tippen — die Einstellungen oben wirken sofort, unabhängig von den Schaltern.',
  'demo.sample': 'Tippe etwas und sieh die Funken fliegen.',
  'demo.unavailable': 'Die Live-Vorschau benötigt eine neuere Version von note.md.',
  saved: 'Gespeichert',
  saveFailed: 'Einstellungen konnten nicht gespeichert werden',
}

export const CATALOGS = { en, zh, ja, de } as const

let current: keyof typeof CATALOGS = 'en'

export function setLocale(locale: string): void {
  const base = locale.split('-')[0] as keyof typeof CATALOGS
  current = base in CATALOGS ? base : 'en'
}

export function t(key: MessageKey): string {
  return CATALOGS[current][key] ?? CATALOGS.en[key]
}
```

- [ ] **Step 7: 复制 editor-kit 装载器**

Create `plugins-src/power-mode/src/lib/editor-kit.ts` —— 复制 `plugins-src/idea-spark/src/lib/editor-kit.ts`，并做两处增补：

在 `KitEditor` 接口里加：

```ts
  /** 换特效配置,不重挂编辑器。宿主 v1 的增量成员。 */
  setPowerMode(cfg: unknown): void
```

在 `KitOptions` 接口里加：

```ts
  /**
   * 显式给值 = 本窗口自管特效,不看生效面开关。实操区正是要这个语义:
   * 用户还没勾「主编辑窗口」也该能在这里看到效果。
   */
  powerMode?: unknown
```

- [ ] **Step 8: 跑插件测试确认通过**

Run: `pnpm --filter power-mode test`
Expected: PASS（parity + 3 条 config + 2 条 strings）

- [ ] **Step 9: 写 App.svelte**

Create `plugins-src/power-mode/src/App.svelte`:

```svelte
<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import { bridge, loadPowerMode, savePowerMode, type SurfaceEntry } from './lib/bridge'
  import { normalizeConfig, DEFAULT_CONFIG, PRESET_IDS } from './lib/config'
  import type { PowerModeConfig, PresetId } from './lib/types'
  import { loadKit, type KitEditor } from './lib/editor-kit'
  import { setLocale, t } from './lib/strings'

  setLocale(bridge().locale)

  let cfg = $state<PowerModeConfig>(structuredClone(DEFAULT_CONFIG))
  let surfaces = $state<SurfaceEntry[]>([])
  let demoHost = $state<HTMLDivElement | null>(null)
  let kit: KitEditor | null = null
  let kitFailed = $state(false)
  let saveTimer: ReturnType<typeof setTimeout> | undefined
  let error = $state<string | null>(null)

  function surfaceLabel(s: SurfaceEntry): string {
    return s.names[bridge().locale] ?? s.names[bridge().locale.split('-')[0]] ?? s.name
  }

  function surfaceOn(id: string): boolean {
    const v = cfg.surfaces[id]
    return typeof v === 'boolean' ? v : id !== 'main'
  }

  /** 每次改动:实操区立刻跟上,落盘 debounce 300 ms。 */
  function touched(): void {
    kit?.setPowerMode($state.snapshot(cfg))
    if (saveTimer !== undefined) clearTimeout(saveTimer)
    saveTimer = setTimeout(async () => {
      try {
        await savePowerMode($state.snapshot(cfg))
        error = null
      } catch (e) {
        error = `${t('saveFailed')}: ${String(e)}`
      }
    }, 300)
  }

  function setSurface(id: string, on: boolean): void {
    cfg.surfaces = { ...cfg.surfaces, [id]: on }
    touched()
  }

  onMount(async () => {
    try {
      const payload = await loadPowerMode()
      cfg = normalizeConfig(payload.config ?? {})
      surfaces = payload.surfaces
    } catch (e) {
      error = String(e)
    }
    if (!demoHost) return
    try {
      const mount = await loadKit()
      kit = await mount(demoHost, {
        initialMarkdown: t('demo.sample'),
        mode: 'rich',
        // 显式给值 = 实操区自管,不受上面的生效面开关影响。
        powerMode: $state.snapshot(cfg),
      })
    } catch {
      kitFailed = true
    }
  })

  onDestroy(() => {
    if (saveTimer !== undefined) clearTimeout(saveTimer)
    kit?.destroy()
  })
</script>

<main>
  <h1>{t('title')}</h1>

  {#if error}<p class="error">{error}</p>{/if}

  <section>
    <h2>{t('surfaces.section')}</h2>
    <label class="row">
      <input type="checkbox" checked={surfaceOn('main')}
             onchange={(e) => setSurface('main', e.currentTarget.checked)} />
      <span>{t('surfaces.main')}</span>
    </label>
    {#each surfaces as s (s.id)}
      <label class="row">
        <input type="checkbox" checked={surfaceOn(s.id)}
               onchange={(e) => setSurface(s.id, e.currentTarget.checked)} />
        <span>{surfaceLabel(s)}</span>
      </label>
    {/each}
    <p class="hint">{t('surfaces.hint')}</p>
  </section>

  <section>
    <h2>{t('effects.section')}</h2>

    <label class="row">
      <input type="checkbox" bind:checked={cfg.explosion.enable} onchange={touched} />
      <span>{t('explosion.enable')}</span>
    </label>
    <label class="row indent">
      <span>{t('explosion.preset')}</span>
      <select bind:value={cfg.explosion.presetId} onchange={touched} disabled={!cfg.explosion.enable}>
        {#each PRESET_IDS as id (id)}
          <option value={id}>{t(`preset.${id}` as 'preset.particle')}</option>
        {/each}
      </select>
    </label>

    <label class="row">
      <input type="checkbox" bind:checked={cfg.shake.enable} onchange={touched} />
      <span>{t('shake.enable')}</span>
    </label>
    <label class="row indent">
      <span>{t('shake.intensity')}</span>
      <input type="range" min="1" max="20" step="1" bind:value={cfg.shake.intensity}
             oninput={touched} disabled={!cfg.shake.enable} />
      <output>{cfg.shake.intensity} px</output>
    </label>
    <label class="row indent">
      <span>{t('shake.recoverTime')}</span>
      <input type="range" min="100" max="2000" step="50" bind:value={cfg.shake.recoverTime}
             oninput={touched} disabled={!cfg.shake.enable} />
      <output>{cfg.shake.recoverTime} ms</output>
    </label>

    <label class="row">
      <input type="checkbox" bind:checked={cfg.combo.enable} onchange={touched} />
      <span>{t('combo.enable')}</span>
    </label>
    <label class="row indent">
      <span>{t('combo.timeout')}</span>
      <input type="range" min="2" max="30" step="1" bind:value={cfg.combo.timeout}
             oninput={touched} disabled={!cfg.combo.enable} />
      <output>{cfg.combo.timeout} s</output>
    </label>
    <label class="row indent">
      <input type="checkbox" bind:checked={cfg.combo.showExclamation}
             onchange={touched} disabled={!cfg.combo.enable} />
      <span>{t('combo.showExclamation')}</span>
    </label>
    <label class="row indent">
      <input type="checkbox" bind:checked={cfg.combo.precisionInput}
             onchange={touched} disabled={!cfg.combo.enable} />
      <span>{t('combo.precisionInput')}</span>
    </label>
    <p class="hint indent">{t('combo.precisionInput.hint')}</p>
  </section>

  <section class="demo">
    <h2>{t('demo.section')}</h2>
    <p class="hint">{t('demo.hint')}</p>
    {#if kitFailed}
      <p class="hint">{t('demo.unavailable')}</p>
    {:else}
      <div class="demo-host" bind:this={demoHost}></div>
    {/if}
  </section>
</main>

<style>
  /* 独立 Tauri 窗口须自声明 color-scheme,否则系统深色下 Canvas 系统色卡浅。 */
  :global(html) { color-scheme: light dark; }
  :global(body) { margin: 0; font: 13px/1.5 -apple-system, system-ui, sans-serif; }

  main {
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 18px 20px;
    height: 100vh;
    box-sizing: border-box;
  }
  h1 { font-size: 17px; margin: 0; }
  h2 { font-size: 12px; text-transform: uppercase; letter-spacing: .06em; opacity: .6; margin: 0 0 8px; }
  section { display: flex; flex-direction: column; }
  .row { display: flex; align-items: center; gap: 8px; padding: 3px 0; }
  .row.indent { padding-left: 22px; }
  .row span { flex: 0 0 auto; }
  .row input[type='range'] { flex: 1; }
  output { min-width: 56px; text-align: right; opacity: .7; font-variant-numeric: tabular-nums; }
  .hint { margin: 4px 0 0; opacity: .55; font-size: 12px; }
  .hint.indent { padding-left: 22px; }
  .error { color: #d33; margin: 0; }

  /* Kit 要求容器有确定高度:content-sized 容器下 source 模式会塌成 0。 */
  .demo { flex: 1; min-height: 0; }
  .demo-host {
    flex: 1;
    min-height: 180px;
    border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
    border-radius: 6px;
    overflow: hidden;
  }
</style>
```

- [ ] **Step 10: 加 dev 安装分支**

Modify `scripts/dev-install-plugin.sh`：

（a）第 4 行 usage、第 41 行的 `case` 分支、第 42 行的报错文案里，三处 `|idea-spark)` 之后各加 `|power-mode`。

（b）在文件里 `elif [[ "$PLUGIN" == "idea-spark" ]]; then … fi` 的 `fi` 之前插入：

```bash
elif [[ "$PLUGIN" == "power-mode" ]]; then
  SRC="plugins-src/power-mode"
  # Build the standalone UI bundle (dist/). Pure UI plugin; no native backend.
  pnpm --filter power-mode build
  VERSION=$(node -e "console.log(require('./$SRC/manifest.v2.json').version)")
  DEST="$ROOT/notemd.power-mode/$VERSION"
  rm -rf "$DEST"
  mkdir -p "$DEST/ui"
  cp -R "$SRC/dist/." "$DEST/ui/"
  cp "$SRC/manifest.v2.json" "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.power-mode/current"
  mark_installed "notemd.power-mode" "$VERSION"
  echo "✓ installed notemd.power-mode@$VERSION (ui-only) → $DEST"
  echo "  open it:                Plugins menu ▸ \"狂暴模式\""
  echo "  NOTE: 用了 Editor Kit 的插件联调前必须先 pnpm build —— 插件窗口读的是"
  echo "        磁盘上的 dist/,不是 Vite dev server。"
fi
```

（注意：原来 idea-spark 分支后面的 `fi` 要保留在新分支之后。）

- [ ] **Step 11: 安装依赖 + 构建 + 类型检查**

Run: `pnpm install && pnpm --filter power-mode build && pnpm --filter power-mode check && pnpm --filter power-mode test`
Expected: 全通过

- [ ] **Step 12: manifest 校验**

Run: `cd src-tauri && cargo test -p plugin-protocol`
Expected: PASS。另外手动确认 manifest 能被解析：

```bash
node -e "const m=require('./plugins-src/power-mode/manifest.v2.json');
console.log(m.id, m.version, m.capabilities.join(','), m.contributes.windows[0].open_command)"
```
Expected: `notemd.power-mode 1.0.0 editor.kit,power-mode open`

- [ ] **Step 13: Commit**

```bash
git add plugins-src/power-mode scripts/dev-install-plugin.sh pnpm-lock.yaml
git commit -m "$(cat <<'EOF'
feat(power-mode): 插件工程(设置窗口 + Editor Kit 实操区 + 四语种)

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: 端到端验证与发布

**Files:**
- Modify: `plugins-src/power-mode/manifest.v2.json`（`engines.notemd` 对齐实际发布版本）
- Modify: `src/lib/power-mode/presets.ts`（若目视比对后需要调 `size`）
- Modify: `docs/superpowers/specs/2026-08-05-power-mode-plugin-design.md`（状态改为已实现）

**Interfaces:**
- Consumes: Task 1–8 全部。
- Produces: 已发布的宿主版本 + 市场上的 `notemd.power-mode@1.0.0`。

- [ ] **Step 1: 全量自动化门禁**

Run: `pnpm check && pnpm test && pnpm build && (cd src-tauri && cargo test)`
Expected: 全绿。**任何一项没过就不要往下走。**

- [ ] **Step 2: dev 安装并起应用**

Run:
```bash
pnpm build                       # Kit 走磁盘 dist/,联调前必须先 build
scripts/dev-install-plugin.sh power-mode
pnpm tauri dev
```

- [ ] **Step 3: 手动验证清单（由用户实机操作）**

把下面这份清单交给用户逐条核对——**本项目不做 UI 自动化，不要用 osascript 驱动窗口**：

1. Plugins 菜单出现「狂暴模式」，点开出设置窗口（620×760）。
2. 「生效范围」里有「主编辑窗口」（未勾）和「奇思妙想」（已勾）；没装 idea-spark 时只有主编辑窗口一行。
3. 底部实操区能敲字，且**开箱就有特效**（不受上面开关影响）。
4. 拖「强度」滑块 → 实操区抖动幅度立刻变化，无需保存。
5. 换预设下拉 → 四个预设逐个试：
   - `particle`：粒子跟随主题文字色（切深浅色主题各看一次）
   - `lightning`：闪电，`mix-blend-mode: color-dodge` 在深浅色下都不发灰/不过曝
   - `coin` / `confetti`：图片模式正常播放
   - **每个预设的尺寸是否合适**（`size` 是 `ch`/`rem`，与 Obsidian 字体不同）。不合适就记下来，Step 4 调。
6. 勾上「主编辑窗口」→ 切到主窗口敲字 → 抖动 + 连击 + 爆炸都在。source 模式下**什么都不发生**（预期）。
7. 打开 Idea Spark 窗口敲字 → 有特效。回设置窗口取消勾选「奇思妙想」→ 切回 Idea 窗口（重新获得焦点）→ 特效消失。
8. 关掉设置窗口重开 → 所有选项保持上次的值。
9. Settings ▸ 插件里停用 power-mode → 主窗口和 Idea 窗口都不再有特效。
10. 连续快速输入 30 秒 → 不掉帧；`document.querySelectorAll('.power-mode-explosion').length` 不持续增长。

- [ ] **Step 4: 按目视结果调预设尺寸（如有需要）**

Modify `src/lib/power-mode/presets.ts` 的 `PRESET_PARAMS.<preset>.size`，并同步更新 `src/lib/power-mode/config.test.ts` 里那条 `toMatchObject` 的期望值（它是逐字钉上游值的，改了就得一起改，并在测试里加一行注释说明「本项目字体下重调」）。

Run: `pnpm test src/lib/power-mode/config.test.ts`
Expected: PASS

- [ ] **Step 5: 记录并提交调整**

```bash
git add src/lib/power-mode/presets.ts src/lib/power-mode/config.test.ts
git commit -m "$(cat <<'EOF'
fix(power-mode): 按本项目编辑器字体重调预设尺寸

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

（若 Step 3 全部通过、无需调整，跳过 Step 4/5。）

- [ ] **Step 6: 发布宿主**

> **发布前先确认**：`gh auth status` 的活跃账号是 `wizlijun`；发布必须在**独立 worktree** 里做（并行会话污染过发布产物）；产物是 aarch64 + x86_64 两个独立 dmg。按项目现有 `scripts/release.sh` 流程走，版本号按日期规则自动推导。

Run: 项目既有的发布流程（`scripts/release.sh`）。
Expected: 两个架构的 dmg + tarball 都产出，`lipo` 架构自检通过。

- [ ] **Step 7: 对齐 manifest 的 engines 并发布插件**

Modify `plugins-src/power-mode/manifest.v2.json` 的 `engines.notemd`，改成 Step 6 实际发布的宿主版本。

Run:
```bash
pnpm --filter power-mode build
pnpm release:plugins
pnpm gen:plugin-index
```

> ⚠️ **`gen-plugin-index.mjs` 会把本地 `dist-plugins/` 里已经下架的旧版本扫回索引。** 生成后核对 `index.json` 里 `notemd.power-mode` 只有 1.0.0，且别的插件条目没被意外改动（脚本默认 merge 线上索引，但本地残留产物仍会被扫进来）。

发布 KV/R2 索引按项目现有流程（本地 wrangler，不走 GitHub Actions）。

- [ ] **Step 8: 装市场版做一次冒烟**

Run: 从插件市场窗口安装 `notemd.power-mode@1.0.0`，重启应用，重跑 Step 3 的第 1、3、6、7 条。

> ⚠️ 若市场显示「已是最新」但功能没变：dev-install 占了同一个版本号。用 `install notemd.power-mode@1.0.0` 强制下发，或先删掉 `~/Library/Application Support/net.notemd.app/plugins/notemd.power-mode/`。

- [ ] **Step 9: 收尾**

Modify `docs/superpowers/specs/2026-08-05-power-mode-plugin-design.md` 的文件头，把
`> 状态：设计已确认，尚未实现`
改成
`> 状态：已实现并发布（宿主 <版本>，插件 notemd.power-mode@1.0.0，2026-08-05）`

```bash
git add docs/superpowers/specs/2026-08-05-power-mode-plugin-design.md plugins-src/power-mode/manifest.v2.json
git commit -m "$(cat <<'EOF'
docs(power-mode): spec 标记已实现并发布

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```
