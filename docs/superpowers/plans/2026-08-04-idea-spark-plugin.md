# 奇思妙想(Idea Spark)插件实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `notemd.idea-spark` 插件(托盘/插件菜单入口 + rich/source 编辑窗 + 委托 claude-agent 论证 idea),以及它依赖的宿主基建:`host.vault.read_bytes`、Editor Kit(`__host__` 运行时下发)、宿主守望器(补纯前端插件无后端进程的空缺)。

**Architecture:** 纯前端插件(照 decision-log 形态)+ 宿主桥扩展。编辑器由宿主以 Editor Kit 组件包运行时下发(主前端同一次 vite 构建的第二个 entry,共享 moraya chunk,安装包净增≈0)。调 agent 走**并行会话「AI 先读」定义的 `host.agent.run` / `host.agent.status`**(capability `agent`);完成提醒走**它的托盘提醒注册表**(`OpenPath` 打开 `.proof.md`)。任务模板 `idea-proof` 由插件幂等种入 vault,claude-agent 自动发现。

**Tech Stack:** Svelte 5 + Tauri 2(Rust)、@moraya/core、vitest、cargo test。

**Spec:** `docs/superpowers/specs/2026-08-04-idea-spark-plugin-design.md`(全部需求以此为准)。

## Global Constraints

- 插件 id `notemd.idea-spark`,英文名 `Idea Spark`,i18n zh「奇思妙想」/ ja「アイデアスパーク」/ de「Ideenfunke」。
- idea 目录默认 `inbox/ideas`(vault 相对),配置存 vault 内 `.notemd/idea-spark.json`。
- 插件 manifest `engines.notemd: ">=6.804.3"`(桥扩展随宿主下一版发布;6.804.2 已发布**不含**这些扩展。发版当天按日期规则实际号可能更大,Task 15 打包前对齐真实号)。
- 新桥方法与 capability:`host.vault.read_bytes`→`vault.read`;`host.theme.css`→`editor.kit`;`__host__/` 资产→`editor.kit`。
- **与并行会话「AI 先读」的 API 对齐(2026-08-04 用户裁决)**:调 agent 一律用它定义的 `host.agent.run`/`host.agent.status`(capability `agent`),**不做 `host.plugin.execute`**(其 spec 明确列为非目标:权限面过大);完成提醒一律进它的托盘提醒注册表,**不自造通知机制**。其 spec 见 `docs/superpowers/specs/2026-08-04-ai-first-read-design.md`。
- 写 `.md` 必须经 `src/lib/okf/concept.ts` 模式(插件用**复制**的 concept.ts);新 `type` 先在 CONCEPT_TYPE 登记:`idea: 'Idea'`、`ideaProof: 'Idea Proof'`。文件名避开 `index.md`/`log.md`。
- 插件 UI 是隔离 webview:**绝不 import 主程序 `src/`**,一切能力走 `window.notemd` 桥。Editor Kit 是宿主代码,可以 import `src/`,但其依赖图**不得触碰任何 Tauri IPC 模块**(`@tauri-apps/api`、adapters、tabs、insights)。
- 主 worktree 常被共享:每次 commit 只精确 add 本任务列出的文件,绝不 `git add -A`。
- Rust 测试:`cargo test --manifest-path src-tauri/Cargo.toml plugin_runtime`;主前端:`pnpm check && pnpm test`;插件:`pnpm --filter idea-spark test`。
- GUI/窗口改动不做自动化验证:最后给手动验证清单,由用户实机验证。

## 本期范围与执行顺序(2026-08-04 定)

并行会话正在实现 `host.agent.*` 与托盘提醒;本轮**只做不依赖它们的任务**,委托与提醒链路等其 API 合入 main 后再做。

- **本轮执行(按序)**:Task 1 → 9 → 5 → 6 → 7 → 10 → 11 → 12 → 14。
- **押后(依赖 `host.agent.*` / 托盘提醒注册表)**:Task 3、8、13,以及 Task 14 的委托按钮链路(见 Task 14 的「本轮不做」小节)。
- **永久取消**:Task 2(`host.plugin.execute`)、Task 4(系统通知)。

基线(2026-08-04,worktree `feat/idea-spark`,base d165f6e):`cargo test --lib plugin_runtime` 142 passed;`pnpm test` 1709 passed / 156 files。

---

### Task 1: 宿主桥 `host.vault.read_bytes`

**Files:**
- Modify: `src-tauri/src/plugin_runtime/host_api.rs`(method_capability 表 + 进程通道 vault 分派 + 表测试)
- Modify: `src-tauri/src/plugin_runtime/ui_rpc.rs`(方法体 + dispatch_with 分派 + 测试)

**Interfaces:**
- Produces: 桥方法 `host.vault.read_bytes` `{path} → {base64}`(vault 相对路径,复用 `resolve_in_vault` 校验与 `base64_encode`,上限 `MAX_TEXT_BYTES`,即 vault 文本读写同一上限)。UI 桥与进程通道均可用,capability `vault.read`。

- [ ] **Step 1: 写失败测试**(`ui_rpc.rs` tests 模块,仿既有 `vault_round_trip_on_process_channel_with_services` 的姿势;`host_api.rs` 的 `method_capability_table` 测试加一行)

```rust
// ui_rpc.rs tests 内(用该模块既有的 services 桩/run_as 辅助):
#[tokio::test]
async fn vault_read_bytes_returns_base64_and_respects_gate() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("img.png"), b"\x89PNG").unwrap();
    // 有 vault.read → base64("\x89PNG")
    let r = run_as(&services_for(vault.path()), "p.id", &["vault.read"],
        "host.vault.read_bytes", serde_json::json!({"path": "img.png"})).await;
    assert_eq!(r.result.unwrap()["base64"], "iVBORw==".trim_end_matches("").to_string().replace("iVBORw==","iVBORw=="));
    // ↑ 实际断言写成:base64_encode(b"\x89PNG") 的值 "iVBORw=="
    // 无 capability → -32001
    let r = run_as(&services_for(vault.path()), "p.id", &[],
        "host.vault.read_bytes", serde_json::json!({"path": "img.png"})).await;
    assert_eq!(r.error.unwrap().code, proto::ERR_CAPABILITY_DENIED);
    // 越界路径 → ERR_INTERNAL(resolve_in_vault 拒绝)
    let r = run_as(&services_for(vault.path()), "p.id", &["vault.read"],
        "host.vault.read_bytes", serde_json::json!({"path": "../x"})).await;
    assert!(r.error.is_some());
}
```

注:`run_as`/services 桩沿用 `ui_rpc.rs` tests 既有辅助(约 :1100 附近);`host_api.rs::method_capability_table` 加 `assert_eq!(method_capability("host.vault.read_bytes"), Some("vault.read"));`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml plugin_runtime::ui_rpc`
Expected: FAIL(-32601 unknown method)

- [ ] **Step 3: 最小实现**

```rust
// ui_rpc.rs — method_capability 由 host_api.rs 提供,先在 host_api.rs:39 的
// vault.read 分支加上 "host.vault.read_bytes":
"host.vault.info" | "host.vault.read" | "host.vault.read_bytes"
| "host.vault.exists" | "host.vault.list" => Some("vault.read"),

// ui_rpc.rs vault_read 旁新增:
/// `{ path } → { base64 }` — vault 内文件原始字节(base64),上限 MAX_TEXT_BYTES。
pub(crate) fn vault_read_bytes(services: &dyn HostServices, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let p = resolve_in_vault(services, params)?;
    let meta = std::fs::metadata(&p).map_err(|e| format!("io: {e}"))?;
    if meta.len() > MAX_TEXT_BYTES {
        return Err(format!("too_large: file exceeds {MAX_TEXT_BYTES} bytes"));
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("io: {e}"))?;
    Ok(serde_json::json!({ "base64": base64_encode(&bytes) }))
}

// dispatch_with 的 match 加一臂:
"host.vault.read_bytes" => vault_read_bytes(services, &req.params),

// host_api.rs make_sink 的进程通道 vault_out match 加一臂(与其余 vault 一致):
"host.vault.read_bytes" => Some(rpc::vault_read_bytes(s, &req.params)),
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml plugin_runtime`
Expected: PASS(含既有全部测试)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin_runtime/host_api.rs src-tauri/src/plugin_runtime/ui_rpc.rs
git commit -m "feat(plugin-bridge): host.vault.read_bytes——vault 内文件按 base64 读取(vault.read)"
```

---

### Task 2: (永久取消)宿主桥 `host.plugin.execute`

**不做。** 并行会话「AI 先读」的 spec 把通用 `host.plugin.execute` 明确列为非目标(「权限面过大」),用户 2026-08-04 裁决以它为准。

替代:调 claude-agent 一律走它定义的 `host.agent.run` / `host.agent.status`(capability `agent`),由主程序转发到 claude-agent 的 `command.execute`。本计划的 Task 13 按该 API 实现,并押后到其合入 main 之后。

---

### Task 3: (押后)宿主守望器桥面

**押后到「AI 先读」的 `host.agent.*` 与托盘提醒注册表合入 main 之后。**

原设计的 `host.agent.watch` + `plugin_v2_window_push` 保留其存在理由——奇思妙想是**纯前端插件、没有后端进程**,窗口一关就没人轮询,而「AI 先读」的模型假设插件有后端进程自行轮询 `host.agent.status`。宿主必须替这类插件守望。

落地时的形态(用户 2026-08-04 裁决):守望器终态时**直接写进托盘提醒注册表**(action = `OpenPath(<proof 路径>)`),不自造通知机制;桥面仍需一个登记入口(名称与参数在其 API 落地后对齐,可能并入 `host.agent.*` 家族)。届时按 Task 8 的守望器实现一并做。

---

### Task 4: (本期取消)系统通知接入

**不做。** 用户在另一处统一做托盘通知(tray icon 四态/子菜单那套),届时直接复用,本计划不引入 `tauri-plugin-notification`,也不加 `notification:default` 权限。

对下游的影响与替代:Task 8 的守望器**不直接发通知**,改为暴露一个可注册的通知挂钩 `registerWatchNotifier(fn)`(默认无注册者 = 不提醒)。统一托盘通知落地后,只需在启动时调用一次 `registerWatchNotifier` 即可接上,守望器本身不用改。

本期的完成提醒因此降级为:窗口开着 → 就地欢庆(插件窗口收到回推);窗口关着 → 下次开窗时历史列表显示「已完成」。这是已知的临时缺口,不是遗漏。

---

### Task 5: Editor Kit——vite 第二 entry + kit 源码

**Files:**
- Modify: `vite.config.ts`(多入口 + 稳定产物名)
- Create: `src/editor-kit/main.ts`(入口:mountMarkdownEditor + CSS/主题注入)
- Create: `src/editor-kit/rich.ts`(moraya 挂载,复刻 editor-bridge 选项)
- Create: `src/editor-kit/source.ts`(textarea+pre 高亮骨架)
- Create: `src/editor-kit/media.ts`(桥版 MediaResolver)
- Create: `src/editor-kit/theme.ts`(host.theme.css → style 槽)
- Create: `src/editor-kit/kit.css`(工具条/布局的少量自有样式)
- Test: `src/editor-kit/source.test.ts`、`src/editor-kit/media.test.ts`(vitest/jsdom)

**Interfaces:**
- Produces(v1 冻结,插件 Task 14 消费):

```ts
export interface KitEditor {
  getMarkdown(): string
  setMarkdown(md: string): void
  getMode(): 'rich' | 'source'
  setMode(m: 'rich' | 'source'): Promise<void>
  focus(): void
  destroy(): void
}
export interface KitOptions {
  initialMarkdown: string
  mode?: 'rich' | 'source'          // 默认 'rich'
  onChange?: (md: string) => void   // debounce 由内部处理
  placeholder?: string
}
export async function mountMarkdownEditor(container: HTMLElement, opts: KitOptions): Promise<KitEditor>
```

- 依赖约束:kit 依赖图只允许 `@moraya/core`、`src/styles/editor-base.css`、`src/lib/source-highlight.ts`、`src/lib/autopair.ts`、`window.notemd`(运行时全局)。**不 import `src/lib/editor-bridge.ts`**(它拖进 tabs/insights/Tauri adapters——IPC 模块,插件 webview 没有);createEditor 的选项值逐项复刻 editor-bridge.ts:45-68,并在两处各留一行注释互指,提醒同步。

- [ ] **Step 1: vite 多入口配置**

```ts
// vite.config.ts build.rollupOptions 改为:
rollupOptions: {
  input: {
    index: 'index.html',
    insights: 'insights.html',
    preview: 'preview.html',
    pluginMarket: 'plugin-market.html',
    logs: 'logs.html',
    dailyNotes: 'daily-notes.html',
    // Editor Kit:运行时下发给插件窗口的编辑器组件包(spec §3.4)。
    // JS 入口、稳定文件名;与主窗口共享 moraya 等公共 chunk,安装包净增≈0。
    'editor-kit': 'src/editor-kit/main.ts',
  },
  output: {
    entryFileNames: (c) => c.name === 'editor-kit' ? 'assets/editor-kit-v1.js' : 'assets/[name]-[hash].js',
    chunkFileNames: 'assets/[name]-[hash].js',
    assetFileNames: (a) =>
      (a.names ?? []).some((n) => n.startsWith('editor-kit'))
        ? 'assets/editor-kit-v1[extname]'
        : 'assets/[name]-[hash][extname]',
  },
},
```

- [ ] **Step 2: 写 source 模式与 media 的失败测试**

```ts
// src/editor-kit/source.test.ts
import { describe, it, expect } from 'vitest'
import { mountSource } from './source'

describe('kit source mode', () => {
  it('renders textarea + highlight pre and round-trips value', () => {
    const host = document.createElement('div')
    const s = mountSource(host, '# Title\n\nbody', () => {})
    const ta = host.querySelector('textarea')!
    expect(ta.value).toBe('# Title\n\nbody')
    expect(host.querySelector('pre')!.innerHTML).toContain('Title')
    s.setValue('changed')
    expect(s.getValue()).toBe('changed')
    s.destroy()
    expect(host.childElementCount).toBe(0)
  })
  it('fires onChange on input', () => {
    const host = document.createElement('div')
    let last = ''
    const s = mountSource(host, '', (v) => (last = v))
    const ta = host.querySelector('textarea')!
    ta.value = 'abc'
    ta.dispatchEvent(new Event('input'))
    expect(last).toBe('abc')
    s.destroy()
  })
})

// src/editor-kit/media.test.ts
import { describe, it, expect, vi } from 'vitest'
import { bridgeMediaResolver } from './media'

describe('bridgeMediaResolver', () => {
  it('resolves vault-relative path via host.vault.read_bytes to a blob url', async () => {
    const request = vi.fn().mockResolvedValue({ base64: 'aGVsbG8=' }) // "hello"
    ;(window as any).notemd = { request }
    const r = bridgeMediaResolver('inbox/ideas')
    const url = await r.resolve('img.png')
    expect(request).toHaveBeenCalledWith('host.vault.read_bytes', { path: 'inbox/ideas/img.png' })
    expect(url).toMatch(/^blob:/)
  })
  it('passes http(s) urls through and placeholders on failure', async () => {
    ;(window as any).notemd = { request: vi.fn().mockRejectedValue(new Error('x')) }
    const r = bridgeMediaResolver('inbox/ideas')
    expect(await r.resolve('https://a/b.png')).toBe('https://a/b.png')
    expect(await r.resolve('missing.png')).toMatch(/^data:image\/png/) // 1x1 占位
  })
})
```

- [ ] **Step 3: 跑测试确认失败**

Run: `pnpm test src/editor-kit`
Expected: FAIL(模块不存在)

- [ ] **Step 4: 实现 kit**

`source.ts`(骨架,高亮/自动配对复用主程序模块):

```ts
import { renderSourceHtml } from '../lib/source-highlight'
import { autopairKeydown } from '../lib/autopair' // 若导出名不同,以该文件实际导出为准接线
export interface SourcePane {
  getValue(): string
  setValue(v: string): void
  focus(): void
  destroy(): void
}
export function mountSource(host: HTMLElement, initial: string, onChange: (v: string) => void): SourcePane {
  const wrap = document.createElement('div')
  wrap.className = 'kit-source'
  const pre = document.createElement('pre')
  pre.className = 'kit-source-hl'
  const ta = document.createElement('textarea')
  ta.className = 'kit-source-ta'
  ta.spellcheck = false
  ta.value = initial
  const paint = () => { pre.innerHTML = renderSourceHtml(ta.value, [], -1) }
  ta.addEventListener('input', () => { paint(); onChange(ta.value) })
  ta.addEventListener('keydown', (e) => autopairKeydown(e, ta))
  ta.addEventListener('scroll', () => { pre.scrollTop = ta.scrollTop; pre.scrollLeft = ta.scrollLeft })
  wrap.append(pre, ta)
  host.appendChild(wrap)
  paint()
  return {
    getValue: () => ta.value,
    setValue: (v) => { ta.value = v; paint() },
    focus: () => ta.focus(),
    destroy: () => wrap.remove(),
  }
}
```

注:`renderSourceHtml`/autopair 的实际签名以 `src/lib/source-highlight.ts`、`src/lib/autopair.ts` 为准(两文件零依赖、可直接 import);签名不合就在 kit 内做最薄适配,不改原文件。

`media.ts`:

```ts
const PLACEHOLDER = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII='
function bridge() { return (window as any).notemd as { request(m: string, p?: unknown): Promise<any> } }
/** MediaResolver 形状对齐 @moraya/core 的 MediaResolver 接口(以 core 类型为准)。 */
export function bridgeMediaResolver(baseDir: string) {
  return {
    async resolve(src: string): Promise<string> {
      if (/^(https?:|data:|blob:)/.test(src)) return src
      try {
        const rel = src.startsWith('/') ? src.slice(1) : `${baseDir}/${src}`
        const { base64 } = await bridge().request('host.vault.read_bytes', { path: rel })
        const bin = atob(base64)
        const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0))
        return URL.createObjectURL(new Blob([bytes]))
      } catch { return PLACEHOLDER }
    },
  }
}
```

`rich.ts`(选项复刻 `src/lib/editor-bridge.ts:45-68`,去掉 Tauri 依赖项):

```ts
import { createEditor, type MorayaEditorInstance } from '@moraya/core'
import { bridgeMediaResolver } from './media'
// 选项值与 src/lib/editor-bridge.ts mountRichEditor 保持一致(那边改这边同步)。
export async function mountRich(
  host: HTMLElement, initial: string, baseDir: string, onChange: (md: string) => void,
): Promise<MorayaEditorInstance> {
  return createEditor({
    container: host,
    initialContent: initial,
    mediaResolver: bridgeMediaResolver(baseDir),
    platform: { getCurrentFilePath: () => null, isMacOS: true },
    enableMath: false,
    enableMermaid: false,
    enableTableResize: true,
    enableImageSelection: false,
    enableHistory: true,
    enableInlineMarkInputRules: false,
    inlineSyntaxScope: 'line',
    onChange,
    changeDebounceMs: 200,
  })
}
```

`theme.ts`(消费 Task 7 的 `host.theme.css`):

```ts
export async function applyKitTheme(): Promise<void> {
  const notemd = (window as any).notemd
  let slot = document.querySelector('style[data-kit-theme]') as HTMLStyleElement | null
  if (!slot) { slot = document.createElement('style'); slot.setAttribute('data-kit-theme', ''); document.head.appendChild(slot) }
  try {
    const t = await notemd.request('host.theme.css', {})
    const dark = t.follow_system && window.matchMedia('(prefers-color-scheme: dark)').matches
    slot.textContent = dark ? t.dark_css : t.light_css
  } catch { slot.textContent = '' }
}
export function watchKitTheme(): void {
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => void applyKitTheme())
  ;(window as any).notemd?.onMessage?.((p: any) => { if (p?.type === 'theme-changed') void applyKitTheme() })
}
```

`main.ts`(拼装 + CSS 注入 + 模式切换,单一字符串真源):

```ts
import '../styles/editor-base.css'
import './kit.css'
import { mountRich } from './rich'
import { mountSource, type SourcePane } from './source'
import { applyKitTheme, watchKitTheme } from './theme'
// KitOptions / KitEditor 接口定义见上方 Interfaces 块,原样放这里。
export async function mountMarkdownEditor(container: HTMLElement, opts: KitOptions): Promise<KitEditor> {
  // 自带 CSS:JS 入口的样式产物是同名 css 文件,相对自身 URL 注入一次。
  const cssHref = new URL('./editor-kit-v1.css', import.meta.url).href
  if (!document.querySelector(`link[href="${cssHref}"]`)) {
    const link = document.createElement('link'); link.rel = 'stylesheet'; link.href = cssHref
    document.head.appendChild(link)
  }
  await applyKitTheme(); watchKitTheme()
  let markdown = opts.initialMarkdown
  let mode: 'rich' | 'source' = opts.mode ?? 'rich'
  const host = document.createElement('div'); host.className = 'kit-host'; container.appendChild(host)
  let rich: Awaited<ReturnType<typeof mountRich>> | null = null
  let source: SourcePane | null = null
  const emit = (md: string) => { markdown = md; opts.onChange?.(md) }
  async function mountCurrent() {
    host.innerHTML = ''
    if (mode === 'rich') { source = null; rich = await mountRich(host, markdown, '', emit) }
    else { rich?.destroy?.(); rich = null; source = mountSource(host, markdown, emit) }
  }
  await mountCurrent()
  return {
    getMarkdown: () => (mode === 'rich' && rich ? rich.getMarkdown() : source?.getValue() ?? markdown),
    setMarkdown: (md) => { markdown = md; if (mode === 'rich') rich?.setContent(md); else source?.setValue(md) },
    getMode: () => mode,
    setMode: async (m) => { if (m === mode) return; markdown = (mode === 'rich' ? rich?.getMarkdown() : source?.getValue()) ?? markdown; mode = m; await mountCurrent() },
    focus: () => (mode === 'rich' ? rich?.view?.focus?.() : source?.focus()),
    destroy: () => { rich?.destroy?.(); source?.destroy(); host.remove() },
  }
}
```

注:`rich.getMarkdown()/setContent()/destroy()/view.focus()` 以 `MorayaEditorInstance` 实际 API 为准(`moraya-core/src/setup.ts` 的返回类型),不符则就地适配。`kit.css` 写 `.kit-host/.kit-source/.kit-source-hl/.kit-source-ta` 的定位样式(透明 textarea 叠 pre,参考 SourceView.svelte 的布局思路,约 60 行)。

- [ ] **Step 5: 跑测试 + 构建验证产物**

Run: `pnpm test src/editor-kit && pnpm build && ls dist/assets/editor-kit-v1.js dist/assets/editor-kit-v1.css`
Expected: 测试 PASS;两个稳定名产物存在;`dist/assets` 里 moraya chunk 只有一份(可 `grep -l prosemirror dist/assets/*.js | wc -l` 抽查非 kit 专有)

- [ ] **Step 6: Commit**

```bash
git add vite.config.ts src/editor-kit/
git commit -m "feat(editor-kit): rich/source 编辑器组件包——主前端第二 entry,共享 chunk,稳定产物名 v1"
```

---

### Task 6: `plugin://` 协议 `__host__` 资产服务(capability 门 + 内嵌资产)

**Files:**
- Modify: `src-tauri/src/plugin_runtime/protocol.rs`(Routed 新变体 + 路由 + shell 用 asset_resolver + 纯路由测试)

**Interfaces:**
- Produces: `GET plugin://<id>/__host__/assets/editor-kit-v1.js`(及其相对 chunk/css)→ 宿主内嵌前端资产 `/assets/...`;调用方 manifest 无 `editor.kit` capability 时 404。`Routed` 枚举新增 `HostAsset(String)`(值为映射后的资产路径,如 `/assets/editor-kit-v1.js`)。

- [ ] **Step 1: 写失败测试**(纯路由层,protocol.rs tests 已有 MapView 桩)

```rust
#[test]
fn host_asset_route_requires_editor_kit_capability() {
    let dir = ui_fixture();
    // 无 editor.kit → 404
    let view = view_with_caps(dir.path(), vec!["vault.read".into()]);
    match handle_parsed(&view, "GET", "p.id", "/__host__/assets/editor-kit-v1.js", None, "en", "default") {
        Routed::Response(r) => assert_eq!(r.status(), http::StatusCode::NOT_FOUND),
        _ => panic!("expected 404 response"),
    }
    // 有 editor.kit → HostAsset,且 __host__ 前缀被剥掉
    let view = view_with_caps(dir.path(), vec!["editor.kit".into()]);
    match handle_parsed(&view, "GET", "p.id", "/__host__/assets/chunk-abc.js", None, "en", "default") {
        Routed::HostAsset(p) => assert_eq!(p, "/assets/chunk-abc.js"),
        _ => panic!("expected HostAsset"),
    }
    // 路径穿越照旧拒绝
    let view = view_with_caps(dir.path(), vec!["editor.kit".into()]);
    match handle_parsed(&view, "GET", "p.id", "/__host__/../secret", None, "en", "default") {
        Routed::Response(r) => assert_eq!(r.status(), http::StatusCode::FORBIDDEN),
        _ => panic!("expected 403"),
    }
}
```

(tests 需要一个带自定义 capabilities 的 view 辅助 `view_with_caps`,仿既有 `view_for`。)

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml plugin_runtime::protocol`
Expected: FAIL(变体不存在)

- [ ] **Step 3: 实现**

```rust
// Routed 加变体:
/// 宿主内嵌前端资产(Editor Kit):shell 用 app.asset_resolver() 取字节。
HostAsset(String),

// handle_parsed 的 "GET" 臂改为:
"GET" => {
    if let Some(rest) = path.strip_prefix("/__host__/") {
        // capability 门:未声明 editor.kit 的插件对宿主资产一律 404(spec §3.4)。
        if !capabilities.iter().any(|c| c == "editor.kit") {
            return Routed::Response(plain(http::StatusCode::NOT_FOUND, "not found"));
        }
        // 只允许 assets/ 下的普通文件名(拒绝任何 .. / 绝对段)。
        if rest.split('/').any(|seg| seg.is_empty() || seg == ".." || seg == ".") {
            return Routed::Response(plain(http::StatusCode::FORBIDDEN, "forbidden"));
        }
        return Routed::HostAsset(format!("/{rest}"));
    }
    Routed::Response(serve_asset(&ui_root, plugin_id, path, locale, theme))
}

// shell(handle,:277 附近)HostAsset 分支:
Routed::HostAsset(asset_path) => {
    let mime = mime_for(std::path::Path::new(&asset_path));
    match app.asset_resolver().get(asset_path.clone()) {
        Some(asset) => http::Response::builder()
            .status(http::StatusCode::OK)
            .header("content-type", mime)
            .header("cache-control", "no-cache")
            .body(asset.bytes().to_vec())
            .unwrap(),
        None => plain(http::StatusCode::NOT_FOUND, "not found"),
    }
}
```

注:`asset_resolver().get` 的确切签名/返回体以 Tauri 2 当前版本为准(`AssetResolver::get(path: String) -> Option<Asset>`);dev 模式下 asset_resolver 走 dev server 同样可用。若 `/__host__/../` 在 URL 解析层已被规整,穿越测试断言按实际到达 handle_parsed 的 path 调整,但拒绝逻辑保留。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml plugin_runtime`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin_runtime/protocol.rs
git commit -m "feat(plugin-protocol): __host__ 保留路径下发宿主内嵌资产(editor.kit 门禁)"
```

---

### Task 7: `host.theme.css` + 主题变更推送

**Files:**
- Modify: `src-tauri/src/themes/commands.rs`(内部函数 `theme_css_bundle`)
- Modify: `src-tauri/src/plugin_runtime/ui_rpc.rs`(dispatch() 拦截)
- Modify: `src-tauri/src/plugin_runtime/host_api.rs`(capability 表 + 表测试)
- Modify: `src-tauri/src/plugin_runtime/commands.rs`(`plugin_v2_theme_changed` 命令)
- Modify: `src-tauri/src/lib.rs`(generate_handler 登记)
- Modify: `src/App.svelte`(applyThemeContent 两处调用后 fire-and-forget invoke,约 :280/:285)

**Interfaces:**
- Produces: 桥方法 `host.theme.css` `{} → {light_css, dark_css, follow_system}`(capability `editor.kit`,仅 UI 桥)。读 settings.json 的 `theme: {light, dark, followSystem}` 对象(缺省 id 用 `"default"`),分别读编译产物(`compiled_path` + read,同 `theme_load_compiled`),读不到的槽给空串。
- Produces: `#[tauri::command] plugin_v2_theme_changed(app)` — 向所有**已打开**且 manifest capabilities 含 `editor.kit` 的插件窗口 push `{type:"theme-changed"}`(遍历 STATE,仿 `windows::refresh_plugin_windows_locale` 的 label 收集)。
- Consumes: Task 5 的 `theme.ts` 消费本方法与推送。

- [ ] **Step 1: 实现 `theme_css_bundle`**(themes/commands.rs;纯读盘,单测覆盖 settings 解析)

```rust
/// settings.json 的 theme 键(对象 {light, dark, followSystem})→ 两个槽的编译 CSS。
/// 键缺失/形状异常一律回退 "default";编译产物缺失时该槽为空串(不报错)。
pub fn theme_css_bundle(app: &tauri::AppHandle) -> serde_json::Value {
    let (light_id, dark_id, follow) = read_theme_settings(app);
    let load = |id: &str| -> String {
        compiled_path(app, id).ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default()
    };
    serde_json::json!({
        "light_css": load(&light_id), "dark_css": load(&dark_id), "follow_system": follow,
    })
}
fn read_theme_settings(app: &tauri::AppHandle) -> (String, String, bool) { /* 读 app_config_dir()/settings.json 的 theme 对象;parse 细节 + 缺省 ("default","default",true) */ }
```

`read_theme_settings` 的 JSON 解析拆成纯函数 `parse_theme_settings(&serde_json::Value) -> (String, String, bool)` 并单测:对象形态取 light/dark/followSystem;历史字符串形态(旧版 theme 是字符串)三值取同一 id + false;缺失取缺省。

- [ ] **Step 2: dispatch() 拦截 + capability 表 + 命令**

```rust
// host_api.rs method_capability 加:
"host.theme.css" => Some("editor.kit"),
// (method_capability_table 测试同步加断言)

// ui_rpc.rs dispatch() 拦截(与 agent.watch 同一段,手动门):
if req.method == "host.theme.css" {
    let id = req.id;
    if !capabilities.iter().any(|c| c == "editor.kit") {
        return err(id, proto::ERR_CAPABILITY_DENIED,
            "method host.theme.css requires capability 'editor.kit'".into());
    }
    return ok(id, crate::themes::commands::theme_css_bundle(app));
}

// plugin_runtime/commands.rs:
/// 主题切换后通知所有持 editor.kit 的已开插件窗口刷新主题 CSS。
#[tauri::command]
pub fn plugin_v2_theme_changed(app: tauri::AppHandle) {
    let targets: Vec<(String, String)> = match super::STATE.read() {
        Ok(st) => st.plugins.iter()
            .filter(|(_, (m, _))| m.capabilities.iter().any(|c| c == "editor.kit"))
            .flat_map(|(pid, (m, _))| m.contributes.windows.iter().map(move |w| (pid.clone(), w.id.clone())))
            .collect(),
        Err(_) => return,
    };
    for (pid, wid) in targets {
        super::windows::push_to_window(&app, &pid, &wid, &serde_json::json!({"type": "theme-changed"}));
    }
}
```

App.svelte 两处 `applyThemeContent(...)` 调用后加:`void invoke('plugin_v2_theme_changed')`。lib.rs generate_handler 登记 `plugin_v2_theme_changed`。

- [ ] **Step 3: 跑测试 + 编译**

Run: `cargo test --manifest-path src-tauri/Cargo.toml && pnpm check`
Expected: PASS(含 parse_theme_settings 单测、capability 表测试)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/themes/commands.rs src-tauri/src/plugin_runtime/ui_rpc.rs src-tauri/src/plugin_runtime/host_api.rs src-tauri/src/plugin_runtime/commands.rs src-tauri/src/lib.rs src/App.svelte
git commit -m "feat(editor-kit): host.theme.css 主题包 + 主题变更推送到 editor.kit 插件窗口"
```

---

### Task 8: (押后)宿主守望器 `src/lib/agent-watch/`

**押后,与 Task 3 同批。** 依赖 `host.agent.status`(轮询)与托盘提醒注册表(终态提醒)。

保留的设计要点,落地时照此实现:轮询 setTimeout 链不进 `$effect`(`$effect` 内调读写 `$state` 的函数须 `untrack`,v4.2.4 教训);守望列表持久化以便主程序重启后恢复;终态推回插件窗口(`window.__notemd_dispatch`)供开着的窗口就地欢庆,同时写托盘提醒供关窗场景;`run-status` 的 `lost` 视为失败。测试用假定时器覆盖 done/lost/重启恢复/去重四条路径。

---

### Task 9: CONCEPT_TYPE 登记 `Idea` / `Idea Proof`

**Files:**
- Modify: `src/lib/okf/concept.ts:12-32`

**Interfaces:**
- Produces: `CONCEPT_TYPE.idea === 'Idea'`、`CONCEPT_TYPE.ideaProof === 'Idea Proof'`(Task 11 复制进插件后消费)。

- [ ] **Step 1: 登记**

```ts
  /** 奇思妙想:用户写下的 idea 原文(plugins-src/idea-spark) */
  idea: 'Idea',
  /** 奇思妙想:agent 产出的论证文档 `<name>.proof.md` */
  ideaProof: 'Idea Proof',
```

- [ ] **Step 2: 验证**

Run: `pnpm check && pnpm test src/lib/okf`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/lib/okf/concept.ts
git commit -m "feat(okf): 登记 Idea / Idea Proof 概念类型(奇思妙想插件)"
```

---

### Task 10: idea-spark 插件脚手架

**Files:**
- Create: `plugins-src/idea-spark/manifest.v2.json`、`package.json`、`vite.config.ts`、`tsconfig.json`、`index.html`、`src/main.ts`、`src/vite-env.d.ts`、`src/App.svelte`(占位壳)、`src/lib/bridge.ts`、`src/lib/strings.ts`
- Test: `plugins-src/idea-spark/src/lib/strings.test.ts`
- Modify: `scripts/dev-install-plugin.sh`(case 列表,仿 decision-log 分支)、`scripts/release-plugins.sh`(case 列表,:41-51 一带,纯 UI 插件出 universal 包)

**Interfaces:**
- Produces: 可构建、可 dev 安装的空窗口插件;`bridge()`/`t()` 供后续任务用。
- `package.json`/`vite.config.ts`/`tsconfig.json` 逐字仿 `plugins-src/decision-log/` 同名文件(name 改 `idea-spark`,dependencies 仅 `yaml`);`bridge.ts` 逐字复制 `plugins-src/decision-log/src/lib/bridge.ts`。

- [ ] **Step 1: manifest**

```json
{
  "manifest_version": 2,
  "id": "notemd.idea-spark",
  "name": "Idea Spark",
  "version": "1.0.0",
  "kind": "native",
  "engines": { "notemd": ">=6.804.2" },
  "description": "Capture a spark, then let an agent argue it into a next-step document.",
  "ui": "ui/",
  "activation": { "events": ["onCommand:open"] },
  "contributes": {
    "menus": [ { "location": "plugins", "label": "Idea Spark", "command": "open" } ],
    "windows": [ {
      "id": "main", "entry": "index.html", "title": "Idea Spark",
      "width": 760, "height": 640, "min_width": 600, "min_height": 480,
      "open_command": "open"
    } ],
    "tray": [ { "window": "main" } ]
  },
  "capabilities": [
    "vault.read", "vault.write", "toast", "editor.open",
    "plugin.execute:notemd.claude-agent", "agent.watch", "editor.kit"
  ],
  "i18n": {
    "zh": { "name": "奇思妙想", "menus": { "open": "奇思妙想" } },
    "ja": { "name": "アイデアスパーク", "menus": { "open": "アイデアスパーク" } },
    "de": { "name": "Ideenfunke", "menus": { "open": "Ideenfunke" } }
  }
}
```

- [ ] **Step 2: strings.ts + 失败测试**

`strings.ts` 照 `plugins-src/decision-log/src/lib/strings.ts` 的结构:`MessageKey` 联合类型 + en 全量 catalog + zh/ja/de 全量 catalog + `t(key)`(locale 取 `bridge().locale`)。首批 key(后续任务增补,增补必须四语齐全):
`title, editorPlaceholder, save, saved, delegate, delegateDeferred, delegating, waitHint, settings, ideaDir, history, statusDraft, statusRunning, statusDone, statusFailed, openResult, retry, needVault, agentMissing, agentMissingHint, celebrate, templateH1, templateHint, sectionDomain, sectionTransfer, sectionResources, sectionOutcome, deleteConfirm, close`

(`delegateDeferred` = 本轮委托按钮禁用时的提示,意为「委托功能待 agent 接口就绪」;`agentMissing*`/`waitHint`/`retry` 等本轮虽无调用方,仍需四语齐全,Task 13 落地即用。)

`strings.test.ts`(插件 i18n 审计通病的固定防线):

```ts
import { describe, it, expect } from 'vitest'
import { CATALOGS, MESSAGE_KEYS } from './strings'
describe('strings', () => {
  it('every locale covers every key (no silent fallback)', () => {
    for (const locale of ['en', 'zh', 'ja', 'de'] as const) {
      for (const key of MESSAGE_KEYS) {
        expect(CATALOGS[locale][key], `${locale}.${key}`).toBeTruthy()
      }
    }
  })
})
```

- [ ] **Step 3: 跑测试确认失败 → 补全四语 catalog → 通过**

Run: `pnpm install && pnpm --filter idea-spark test`
Expected: 先 FAIL(缺文件/缺 key)后 PASS

- [ ] **Step 4: 占位 App + 构建 + 脚本 case**

`App.svelte` 暂时只渲染 `t('title')`;`main.ts` 照 decision-log(mount App)。`dev-install-plugin.sh`/`release-plugins.sh` 各加 `idea-spark` case,内容与 `decision-log` 分支逐字同构(纯 UI:`pnpm --filter idea-spark build` + dist→ui,universal 包)。

Run: `pnpm --filter idea-spark build && bash scripts/dev-install-plugin.sh idea-spark`
Expected: 构建出 `plugins-src/idea-spark/dist/`;本地安装目录出现 `notemd.idea-spark/…/ui/`

- [ ] **Step 5: Commit**

```bash
git add plugins-src/idea-spark/ scripts/dev-install-plugin.sh scripts/release-plugins.sh
git commit -m "feat(idea-spark): 插件脚手架——manifest/托盘入口/四语 strings/构建与安装接线"
```

---

### Task 11: 插件纯逻辑库(TDD)

**Files:**
- Create+Test: `plugins-src/idea-spark/src/lib/okf/concept.ts`(从 `src/lib/okf/concept.ts` **复制**,文件头注明来源与「上游改动需同步」)
- Create: `plugins-src/idea-spark/src/lib/naming.ts` / Test: `naming.test.ts`
- Create: `plugins-src/idea-spark/src/lib/idea-doc.ts` / Test: `idea-doc.test.ts`
- Create: `plugins-src/idea-spark/src/lib/state-io.ts` / Test: `state-io.test.ts`
- Create: `plugins-src/idea-spark/src/lib/status.ts` / Test: `status.test.ts`

**Interfaces (Produces):**

```ts
// naming.ts
export function slugFromMarkdown(md: string): string
//  首个非空标题/首行 → 去 [\\/:*?"<>|#%`] → 空白折叠为 '-' → 截 40 字符 → 空则 'idea'
export function ideaFileName(md: string, today: string, taken: Set<string>): string
//  `${today}-${slug}.md`;撞名追加 -2/-3…;保证不等于保留名 index.md/log.md
export function proofPathFor(ideaRelPath: string): string
//  'inbox/ideas/a.md' → 'inbox/ideas/a.proof.md'

// idea-doc.ts(消费复制的 concept.ts)
export function buildIdeaDoc(body: string, nowIso: string): string
//  conceptFileText({type: CONCEPT_TYPE.idea, created: nowIso}, body)

// state-io.ts —— .notemd/idea-spark.json 的纯序列化(读写走桥,由 App 层做)
export interface SparkState { ideaDir: string; pendingRuns: Record<string, string> } // ideaRelPath → run_id
export const DEFAULT_STATE: SparkState = { ideaDir: 'inbox/ideas', pendingRuns: {} }
export function parseState(raw: string | null): SparkState   // 坏 JSON/缺键 → 默认值合并
export function serializeState(s: SparkState): string
export const STATE_PATH = '.notemd/idea-spark.json'

// status.ts
export type IdeaStatus = 'draft' | 'running' | 'done' | 'failed'
export function deriveStatus(name: string, files: Set<string>, pending: Record<string, string>, failed: Set<string>): IdeaStatus
//  done: `${base}.proof.md` ∈ files;running: rel ∈ pending;failed: rel ∈ failed;否则 draft
export function listIdeas(entries: Array<{name: string; is_dir: boolean}>): string[]
//  过滤:*.md 且非 *.proof.md 且非保留名,按名倒序(新日期在前)
```

- [ ] **Step 1: 复制 concept.ts 并写全部失败测试**(每个模块 3-6 个用例;naming 覆盖:中文标题、空文档、撞名、40 字符截断、保留名规避;state-io 覆盖:null/坏 JSON/部分键;status 覆盖四态与 proof 文件不算 idea)
- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm --filter idea-spark test`
Expected: 新测试全 FAIL

- [ ] **Step 3: 实现四个模块使测试通过**
- [ ] **Step 4: okf-lint 交叉验证**——测试里把 `buildIdeaDoc('x','2026-08-04T00:00:00Z')` 写入临时文件后用 `scripts/okf-lint-core.mjs` 的校验函数断言合规(参照 decision-log 或既有插件测试引用 okf-lint-core 的姿势;若插件测试环境不便引用主仓脚本,则断言 frontmatter 含非空 `type: Idea` 且以 `---` 开头,并在主仓测试侧补 lint 用例)。

Run: `pnpm --filter idea-spark test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add plugins-src/idea-spark/src/lib/
git commit -m "feat(idea-spark): 命名/文档/状态/配置纯逻辑库(TDD)+ 复制 okf concept"
```

---

### Task 12: `idea-proof` 任务模板常量 + 幂等种入

**Files:**
- Create: `plugins-src/idea-spark/src/lib/task-template.ts` / Test: `task-template.test.ts`

**Interfaces:**
- Produces:

```ts
export const TASK_ID = 'idea-proof'
export const TASK_FILES: Record<string, string>
//  键(vault 相对路径):
//  .notemd/agent-tasks/idea-proof/task.json
//  .notemd/agent-tasks/idea-proof/CLAUDE.md
//  .notemd/agent-tasks/idea-proof/precheck.sh
//  .notemd/agent-tasks/idea-proof/.claude/settings.json
//  .notemd/agent-tasks/idea-proof/.claude/settings.scoped.json
export async function seedTaskTemplate(io: { exists(p: string): Promise<boolean>; write(p: string, c: string): Promise<void> }): Promise<void>
//  逐文件 exists→跳过,不存在才 write;绝不覆盖(用户可改模板)
```

- 文件内容(**全文如下,不留白**):

`task.json`:

```json
{
  "name": "Idea proof",
  "description": "把一个模糊的 idea 论证成可下一步的文档:先找落差,再证伪,再最小验证。",
  "prompt": "读取给定的 idea 文件,严格按 CLAUDE.md 的协议输出论证文档。产物只有一个:与 idea 同目录的 <同名>.proof.md。绝不修改 idea 原文。输出语言跟随 idea 原文语言。",
  "max_turns": 80,
  "timeout_seconds": 1800,
  "precheck": "precheck.sh"
}
```

`precheck.sh`:

```sh
#!/bin/sh
# idea 文件必须存在且非空,否则这次运行不值得花 token。
[ -n "$NOTEMD_NOTE" ] || { echo "缺少 idea 文件参数"; exit 1; }
[ -s "$NOTEMD_NOTE" ] || { echo "idea 文件不存在或为空:$NOTEMD_NOTE"; exit 1; }
exit 0
```

`.claude/settings.json`:

```json
{
  "permissions": {
    "allow": [
      "Read(${VAULT}/**)",
      "Write(${VAULT}/**/*.proof.md)",
      "Edit(${VAULT}/**/*.proof.md)",
      "WebSearch",
      "WebFetch"
    ],
    "deny": [ "Bash", "Task" ]
  }
}
```

`.claude/settings.scoped.json`:

```json
{
  "permissions": {
    "allow": [
      "Read(${NOTE})",
      "Read(${VAULT}/**)",
      "Write(${VAULT}/**/*.proof.md)",
      "Edit(${VAULT}/**/*.proof.md)",
      "WebSearch",
      "WebFetch"
    ],
    "deny": [ "Bash", "Task" ]
  }
}
```

(允许 WebSearch/WebFetch 是有意为之:协议的「是否撞题」一步需要查已有工作;与 answer-note-question 的全封闭策略不同。)

`CLAUDE.md`(模板字符串,注意用普通字符串拼接或转义反引号——落地页 ENTRY_MAP 的模板反引号坑):

````markdown
# 任务:把 idea 论证成可下一步的文档

你在 note.md 的 Claude Agent 插件里以 headless 模式运行,vault 根在 `${VAULT}`。
输入是一个 idea 文件(环境变量 NOTEMD_NOTE 指向它):用户刚写下的一个模糊念头,
可能含四个小节:领域/方向、可能迁移的场景、现有条件、期望成果——缺哪个就在文中
标注「未提供」,不要臆造。

你是一名严谨的研究顾问和论文审稿人。目标不是鼓励用户,而是帮 ta 用最低成本缩小
未知空间。按以下流程输出:

1. **先找落差,不要急着给方案**
   - 结果落差:理论有效但实验/现实不稳定的地方。
   - 迁移落差:原场景有效,换到新场景后前提失效的地方。
   - 假设落差:大家默认成立、但现实中未必成立的前提。
   每个落差写成一句可检验陈述:在【具体条件】下,【现有做法】因为【明确原因】,
   无法稳定实现【目标】。
2. **判断这个问题是否值得做**:能否证伪(什么结果会证明它不成立)、能否观测
   (关键证据现在拿得到吗)、能否小规模验证(最小实验是什么)、失败是否有信息
   (失败后能否区分:问题不存在/信号不足/基线够强/方法错了)、是否撞题
   (已有工作是否覆盖;搜不到文献时检查是否只是术语不同或负结果藏在附录里)。
3. **给出 3 个候选研究/产品验证点**,每个包括:可证伪陈述、关键证据和证据等级、
   最接近的已有工作/竞品/实践、与已有工作的真实差异、最小验证动作、
   如果结果为负还能学到什么。
4. **先做反方审稿**:对最值得做的候选,先尝试否定它——是否可能只是测量误差、
   样本偏差、指标选错、数据泄漏或基线过弱?有没有更简单的方法已经足够?
   哪些模糊词必须改成可测指标?如果严重撞题或前提不成立,直接说
   「不值得做」或「需要收窄」。
5. **设计逐级验证门**(按顺序,不跳步):G0 现象是否真实存在;G1 所需信号是否
   可观测;G2 机制是否优于简单基线和强基线;G3 接入完整流程后是否产生真实收益;
   G4 是否能在不同条件下复现。每一关说明:验证命题、最小实验、对照组、通过标准、
   否定标准、失败后的下一步。
6. **最后输出**(文档必须以这个结构收束):直接判断(值得做/需要收窄/暂不值得做)、
   最大未知、最先要做的一个验证动作、3 个候选点排序、逐级验证门、最低成立标准、
   结论边界(在什么条件下,最多能声称什么)。

要求:区分事实、已有结论、你的推断;不编造文献或证据,找不到证据就写
「尚未找到证据」;不为了显得创新而回避简单强基线。

## 产物(逐条遵守)

1. **只写一个文件**:与 idea 同目录、同名去掉 `.md` 加 `.proof.md`
   (例:`inbox/ideas/2026-08-04-foo.md` → `inbox/ideas/2026-08-04-foo.proof.md`)。
2. 文件开头是 YAML frontmatter,`type` 必填:

   ```
   ---
   type: Idea Proof
   title: <一行结论,如「值得做:…」>
   generated:
     by: process:claude-agent
     at: <ISO 8601 时间>
   sources:
     - resource: <idea 文件的 vault 相对路径>
   ---
   ```

3. **绝不修改 idea 原文**,也不写其他任何文件。重跑即整体覆盖旧的 `.proof.md`。
4. 输出语言跟随 idea 原文语言。
````

- [ ] **Step 1: 写失败测试**

```ts
import { describe, it, expect, vi } from 'vitest'
import { TASK_FILES, seedTaskTemplate, TASK_ID } from './task-template'

describe('idea-proof template', () => {
  it('contains the five files with parseable json and okf-frontmatter protocol', () => {
    const keys = Object.keys(TASK_FILES)
    expect(keys).toHaveLength(5)
    const task = JSON.parse(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/task.json`])
    expect(task.timeout_seconds).toBe(1800)
    expect(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/CLAUDE.md`]).toContain('type: Idea Proof')
    expect(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/CLAUDE.md`]).toContain('绝不修改 idea 原文')
    JSON.parse(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/.claude/settings.json`])
  })
  it('seed is idempotent: existing files are never overwritten', async () => {
    const write = vi.fn(); const exists = vi.fn().mockResolvedValue(true)
    await seedTaskTemplate({ exists, write })
    expect(write).not.toHaveBeenCalled()
    const write2 = vi.fn(); const exists2 = vi.fn().mockResolvedValue(false)
    await seedTaskTemplate({ exists: exists2, write: write2 })
    expect(write2).toHaveBeenCalledTimes(5)
  })
})
```

- [ ] **Step 2: 跑测试确认失败 → 实现 → 通过**

Run: `pnpm --filter idea-spark test`

- [ ] **Step 3: Commit**

```bash
git add plugins-src/idea-spark/src/lib/task-template.ts plugins-src/idea-spark/src/lib/task-template.test.ts
git commit -m "feat(idea-spark): idea-proof 任务模板全文常量 + 幂等种入"
```

---

### Task 13: (押后)agent-client——按 `host.agent.*` 实现委托

**押后到「AI 先读」的 `host.agent.run`/`host.agent.status` 合入 main 之后。**

**Files:** Create: `plugins-src/idea-spark/src/lib/agent-client.ts` / Test: `agent-client.test.ts`

落地时的形态(替换原 `host.plugin.execute` 方案):

1. `seedTaskTemplate`(Task 12,已完成)幂等种入 `idea-proof` 模板。
2. `host.vault.info` 取 root,拼绝对路径 `${root}/${ideaRelPath}`(claude-agent 的 `run-note` 期望绝对路径)。
3. `host.agent.run { task: 'idea-proof', note_path: <abs> }` → `run_id`;claude-agent 未装/未启用时该 API 返回 `agent_unavailable`,据此走引导安装分支(不再需要自己探活)。
4. 登记宿主守望(Task 3 落地后的入口),notify 参数 `{title, body, open_path: proofPathFor(ideaRelPath)}`。

manifest 的 `capabilities` 届时补 `agent`(以及托盘提醒若需要的 `notify`)。

---

### Task 14: App.svelte 主界面(编辑器/保存/历史/设置/委托/欢庆)

**Files:**
- Modify: `plugins-src/idea-spark/src/App.svelte`(替换 Task 10 占位壳)
- Create: `plugins-src/idea-spark/src/components/HistoryList.svelte`、`src/components/SettingsPopover.svelte`、`src/components/Celebration.svelte`
- Create: `plugins-src/idea-spark/src/lib/store.svelte.ts` / Test: `store.test.ts`(状态机部分)

**本轮不做(押后到 Task 13/3/8 落地)**:委托按钮的实际调用链路与完成推送。本轮 App 里「委托 Agent」按钮**照常渲染但禁用**,`title`/提示走 strings 的 `delegateDeferred`(四语,文案意为「等 agent 接口就绪」)。`store.svelte.ts` 的 `pending`/`failed` 状态与 `applyRunDone()` 纯函数**照常实现并单测**(Task 13 落地时直接接上),只是本轮没有调用方。启动序列里「对 pendingRuns 逐个 `run-status` 校正」本轮跳过(pendingRuns 恒为空)。`Celebration.svelte` 照常实现,由 store 状态驱动,本轮无触发路径。

**Interfaces:**
- Consumes: Editor Kit(动态 import `plugin://notemd.idea-spark/__host__/assets/editor-kit-v1.js` → `mountMarkdownEditor`,签名见 Task 5);Task 11/12;`bridge().onMessage`(本轮只需忽略未知 payload;`{type:'theme-changed'}` 由 kit 自己消费)。
- 结构与行为(全部落实,不留 TBD):
  - **布局**:上编辑区(kit 容器 + 模式切换由 kit 内置)、下操作条(保存/委托 Agent/设置齿轮)、右侧或底部历史列表。
  - **预填模板**(strings 四语,`templateH1` 等 key):

    ```markdown
    # {templateH1}

    {templateHint}

    ## {sectionDomain}

    ## {sectionTransfer}

    ## {sectionResources}

    ## {sectionOutcome}
    ```

  - **启动序列**:`host.vault.info` → root 为 null 则整窗提示 `needVault`;否则读 `.notemd/idea-spark.json`(`host.vault.read`,不存在按 DEFAULT_STATE)→ `host.vault.list(ideaDir)`(目录不存在视为空)→ 动态 import kit 挂编辑器(失败——宿主过旧/404——显示错误文案并降级为纯 textarea)。
  - **保存**:按钮 + `Cmd/Ctrl+S` 键监听;`ideaFileName(md, today, taken)` 定名(today 用 `new Date()` 本地日期 YYYY-MM-DD),`buildIdeaDoc` 包 frontmatter,`host.vault.write(ideaDir/name)`;成功 toast `saved`,当前文档标记为该文件(再次保存=覆盖同文件,不再改名);编辑器内容非模板原样且未保存时关窗前 `host.toast` 提醒(尽力而为)。
  - **委托(本轮禁用)**:按钮渲染但 `disabled`,悬停/说明文案用 `delegateDeferred`。调用链路见 Task 13。
  - **完成状态迁移(本轮只做纯函数)**:`applyRunDone(state, {run_id, status, open_path})` —— success → 该条 done + 置 `celebrate` 标志(`<Celebration/>` 消费,纯 CSS confetti,2s 自动收);其他 status → failed。本轮由单测驱动,无运行时调用方。
  - **历史列表**:`listIdeas` + `deriveStatus` 渲染;每条:名字(去日期前缀的 slug)、状态徽标(`statusDraft/Running/Done/Failed` 四语)、done 条目有「打开结果」(`host.editor.open`)、点击条目载入该 idea 到编辑器(`host.vault.read`,剥 frontmatter 显示 body——简单按首个 `---…---` 剥离)。
  - **设置弹层**:ideaDir 文本框 + 保存(写 state 文件,重新 list);校验非空、不以 `/` 开头、不含 `..`。
  - **store.svelte.ts** 收拢上述状态(`docs: []`、`pending`、`failed`、`current`、`busy`、`celebrate`),纯状态迁移写 vitest。

- [ ] **Step 1: store 状态机失败测试**(`applyRunDone`: success→done+celebrate、error/lost→failed;setIdeaDir 校验拒绝 `/abs`、`a/../b`、空串)
- [ ] **Step 2: 确认失败 → 实现 store → 通过**

Run: `pnpm --filter idea-spark test`

- [ ] **Step 3: 实现三个组件与 App.svelte 接线,构建通过**

Run: `pnpm --filter idea-spark build && pnpm --filter idea-spark check`
Expected: 构建/类型检查通过

- [ ] **Step 4: Commit**

```bash
git add plugins-src/idea-spark/src/
git commit -m "feat(idea-spark): 主界面——kit 编辑器/保存/历史/设置/委托/欢庆全流程"
```

---

### Task 15: 全量校验、手动验证清单、发布

**本轮范围**:只做 Step 1(全量回归)+ Step 2(文档)+ Step 3(dev 手动验证清单)。**Step 4 发布不做**——本轮产物依赖押后的 Task 3/8/13,插件功能不完整,不上市场。

**Files:**
- Modify: `docs/plugin-v2-development.md`(§5 capability 表补 `editor.kit` 与新方法 `host.vault.read_bytes`/`host.theme.css`、`__host__` 保留路径;§3 contributes 表顺手补上遗漏的 `tray` 字段)
- Modify: `README.md` / `README.zh.md`(若有插件列表段,加一行奇思妙想;没有则跳过)

- [ ] **Step 1: 全量回归**

Run:
```bash
cargo test --manifest-path src-tauri/Cargo.toml
pnpm check && pnpm test
pnpm --filter idea-spark test && pnpm --filter idea-spark build
pnpm okf:lint plugins-src/idea-spark 2>/dev/null || true  # 若 lint 目标不适用插件目录则跳过
```
Expected: 全部通过

- [ ] **Step 2: 文档更新 + Commit**

```bash
git add docs/plugin-v2-development.md README.md README.zh.md
git commit -m "docs(plugin-v2): 登记 editor.kit capability、__host__ 保留路径与两个新桥方法"
```

- [ ] **Step 3: dev 构建 + 交给用户的手动验证清单**(我方只起 dev,不做桌面自动化)

```bash
bash scripts/dev-install-plugin.sh idea-spark && pnpm tauri dev
```

清单(逐条人工过,本轮可验的部分):
1. 插件菜单与托盘出现「奇思妙想」;点击开窗,标题中文。
2. 编辑器 rich 模式 live-preview 正常、与主程序主题一致;切主题后插件窗跟随;rich/source 切换保内容。
3. 写 idea → 保存 → vault `inbox/ideas/` 出现带 frontmatter 的 `.md`(Obsidian/CLI 可读)。
4. 历史列表列出已存 idea;点击载入编辑器;设置里改 idea 目录生效;vault 未开时提示。
5. 「委托 Agent」按钮为禁用态并给出「等 agent 接口就绪」提示(本轮预期行为)。

押后到 Task 3/8/13 落地后再验:委托链路、论证中状态、欢庆、打开 `.proof.md`、关窗后托盘提醒。

**Step 4 发布**:本轮不做。功能完整(Task 3/8/13 落地)后再按惯例发布——宿主先发(独立 worktree + `.env.release`,gh 账号 wizlijun),插件后发(`release-plugins.sh --release idea-spark` → `gen-plugin-index.mjs`,注意本地 dist-plugins 旧包回扫坑),打包前把 `engines.notemd` 对齐实际宿主发版号。

---

## Self-Review 记录

- Spec 覆盖:§1 manifest/托盘(Task 10)、§2 UX 与状态推导(Task 11/14)、§2.1+§3.4 Editor Kit(Task 5/6/7)、§3.3 read_bytes(Task 1)、§4 任务模板(Task 12)、§5 OKF(Task 9/11/12)、§6 错误处理(无 vault Task 14,engines 门 Task 10)、§7 测试(各任务)+§8 YAGNI(未引入批注/wikilink/数学)。
- **押后**:§3.1 调 agent(改用 `host.agent.*`,Task 13)、§3.2 守望与提醒(Task 3/8,终态改推托盘提醒)。**永久取消**:`host.plugin.execute`(Task 2)、系统通知(Task 4)。依据:2026-08-04 用户裁决与并行会话「AI 先读」spec 的 API 对齐。
- 与 spec 的一处已知偏差:kit **不直接 import** `editor-bridge.ts`(它依赖 tabs/insights/Tauri adapters),改为复刻其 createEditor 选项并双向注释互指——spec §3.4 已按此修正。
- 类型一致性:`mountMarkdownEditor`/`KitEditor`(Task 5↔14)、`SparkState`(Task 11↔14)、`applyRunDone`(Task 14 内,Task 13 落地时接上)已对齐。
- 外部 API 有两处以实际代码为准的适配点,均已在任务内标注兜底:`MorayaEditorInstance` 成员名(Task 5)、`AssetResolver::get`(Task 6)。
