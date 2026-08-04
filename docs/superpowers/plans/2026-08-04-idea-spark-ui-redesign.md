# 奇思妙想主界面重做 + 委托链路 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `notemd.idea-spark` 的窗口改成「打开就写字」(无标题栏、空白文档、自动保存、inbox 可隐藏+右键),并接上委托 agent 的完整链路。

**Architecture:** 纯前端插件的 UI 重做 + 两个通用宿主桥方法(`host.vault.remove` / `host.vault.rename`)+ Editor Kit 补 rich 模式占位符。委托走并行会话已合入的 `host.agent.run`/`host.agent.status`,完成提醒由常驻的 claude-agent 经 `notify` 规格代发托盘提醒,**不做宿主守望器**。

**Tech Stack:** Svelte 5(runes)、TypeScript、vitest、Tauri 2 + Rust、@moraya/core。

**Spec:** `docs/superpowers/specs/2026-08-04-idea-spark-ui-redesign-design.md`(需求以此为准)。前一轮设计见 `2026-08-04-idea-spark-plugin-design.md`,本计划取代其 §2/§3.2。

## Global Constraints

- 插件 id `notemd.idea-spark`;i18n 四语必须齐全:en / zh / ja / de。`strings.ts` 用 `Catalog = Record<MessageKey, string>`(映射类型,漏键在 `svelte-check` 编译期即报错),新增/删除 key 两处都要动。
- 插件 UI 跑隔离 webview:**绝不 import 主程序 `src/`**,一切能力走 `window.notemd` 桥。要复用主程序代码只能**复制**并在文件头注明来源与同步义务。
- Editor Kit 依赖图**不得触碰任何 Tauri IPC 模块**(`@tauri-apps/*`、`editor-bridge.ts`、`tabs.svelte`、`insights/*`、`adapters/*`)。允许:`@moraya/core`、`src/styles/editor-base.css`、`src/lib/source-highlight.ts`、`src/lib/autopair.ts`、`src/lib/placeholder-plugin.ts`(本计划新增)。
- Svelte 5 硬教训:`$effect` 内同步调用会读写 `$state` 的函数会自失效死循环、冻结整个 UI。启动序列一律放 `onMount` 的 async 函数;必要时 `untrack`。
- 主 worktree 常被并行会话共享:每次 commit **只精确 `git add` 本任务列出的文件**,绝不 `git add -A`;**不要碰 `docs/superpowers/plans/` 下的其它文件**。
- 占位符金句**一律不带句末句号**(占位符惯例)。
- 测试命令:Rust `cargo test --manifest-path src-tauri/Cargo.toml`;主前端 `pnpm check && pnpm test`;插件 `pnpm --filter idea-spark test` / `check` / `build`。
- 基线(2026-08-04 main `a963950` 之后):cargo 561 passed、`pnpm test` 1751 passed、`pnpm --filter idea-spark test` 131 passed。数字只应增不应减。

---

### Task 1: 宿主桥 `host.vault.remove` / `host.vault.rename`

**Files:**
- Modify: `src-tauri/src/plugin_runtime/host_api.rs`(`method_capability` 表 + 进程通道 vault 分派 + 表测试)
- Modify: `src-tauri/src/plugin_runtime/ui_rpc.rs`(两个方法体 + `dispatch_with` 分派 + 测试)

**Interfaces:**
- Produces: `host.vault.remove { path } → { ok: true }`;`host.vault.rename { from, to } → { ok: true }`。均挂既有 capability `vault.write`,复用既有 `resolve_in_vault` 路径校验。

- [ ] **Step 1: 写失败测试**

在 `ui_rpc.rs` 的 tests 模块内,沿用该模块既有的 services 桩与 `run_as` 风格辅助(先读一遍现有测试再写,签名以实际为准):

```rust
#[tokio::test]
async fn vault_remove_deletes_files_and_refuses_directories() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("a.md"), "x").unwrap();
    std::fs::create_dir(vault.path().join("sub")).unwrap();
    let s = services_for(vault.path());
    // 有 capability → 删除成功
    let r = run_as(&s, "p.id", &["vault.write"], "host.vault.remove",
        serde_json::json!({"path": "a.md"})).await;
    assert_eq!(r.result.unwrap()["ok"], true);
    assert!(!vault.path().join("a.md").exists());
    // 幂等:再删一次仍然 ok
    let r = run_as(&s, "p.id", &["vault.write"], "host.vault.remove",
        serde_json::json!({"path": "a.md"})).await;
    assert_eq!(r.result.unwrap()["ok"], true);
    // 目录 → 拒绝
    let r = run_as(&s, "p.id", &["vault.write"], "host.vault.remove",
        serde_json::json!({"path": "sub"})).await;
    assert!(r.error.unwrap().message.contains("directory"));
    // 无 capability → -32001
    let r = run_as(&s, "p.id", &[], "host.vault.remove",
        serde_json::json!({"path": "b.md"})).await;
    assert_eq!(r.error.unwrap().code, proto::ERR_CAPABILITY_DENIED);
    // 越界 → 错误
    let r = run_as(&s, "p.id", &["vault.write"], "host.vault.remove",
        serde_json::json!({"path": "../x"})).await;
    assert!(r.error.is_some());
}

#[tokio::test]
async fn vault_rename_moves_within_vault_and_never_clobbers() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("a.md"), "x").unwrap();
    std::fs::write(vault.path().join("taken.md"), "y").unwrap();
    let s = services_for(vault.path());
    let r = run_as(&s, "p.id", &["vault.write"], "host.vault.rename",
        serde_json::json!({"from": "a.md", "to": "sub/b.md"})).await;
    assert_eq!(r.result.unwrap()["ok"], true);
    assert!(vault.path().join("sub/b.md").exists());
    assert!(!vault.path().join("a.md").exists());
    // 目标已存在 → 不覆盖
    let r = run_as(&s, "p.id", &["vault.write"], "host.vault.rename",
        serde_json::json!({"from": "sub/b.md", "to": "taken.md"})).await;
    assert!(r.error.unwrap().message.contains("exists"));
    assert_eq!(std::fs::read_to_string(vault.path().join("taken.md")).unwrap(), "y");
    // 两端都过校验
    let r = run_as(&s, "p.id", &["vault.write"], "host.vault.rename",
        serde_json::json!({"from": "sub/b.md", "to": "../out.md"})).await;
    assert!(r.error.is_some());
}
```

`host_api.rs` 的 `method_capability_table` 测试加两行:

```rust
assert_eq!(method_capability("host.vault.remove"), Some("vault.write"));
assert_eq!(method_capability("host.vault.rename"), Some("vault.write"));
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib plugin_runtime`
Expected: FAIL(-32601 unknown method)

- [ ] **Step 3: 最小实现**

```rust
// host_api.rs:41 的 vault.write 分支扩成:
"host.vault.write" | "host.vault.mkdir" | "host.vault.remove" | "host.vault.rename" => {
    Some("vault.write")
}

// host_api.rs 进程通道的 vault_out match 加两臂(与其余 vault 一致):
"host.vault.remove" => Some(rpc::vault_remove(s, &req.params)),
"host.vault.rename" => Some(rpc::vault_rename(s, &req.params)),

// ui_rpc.rs,vault_mkdir 旁新增:
/// `{ path } → { ok: true }`。只删文件:目录要靠别的方式清理,插件误传一个
/// 目录名不该把整棵子树带走。目标不存在按成功处理(幂等,调用方重试安全)。
pub(crate) fn vault_remove(
    services: &dyn HostServices, params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p = resolve_in_vault(services, params)?;
    match std::fs::symlink_metadata(&p) {
        Err(_) => return Ok(serde_json::json!({ "ok": true })), // 已不存在
        Ok(m) if m.is_dir() => return Err("io: refusing to remove a directory".into()),
        Ok(_) => {}
    }
    std::fs::remove_file(&p).map_err(|e| format!("io: {e}"))?;
    Ok(serde_json::json!({ "ok": true }))
}

/// `{ from, to } → { ok: true }`。两端都过 vault 围栏;目标已存在一律报错而不
/// 覆盖 —— 重命名撞名时静默吃掉用户的另一个文件是不可接受的。
pub(crate) fn vault_rename(
    services: &dyn HostServices, params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let from = resolve_in_vault(services, &serde_json::json!({ "path": req_str(params, "from")? }))?;
    let to = resolve_in_vault(services, &serde_json::json!({ "path": req_str(params, "to")? }))?;
    if to.exists() {
        return Err("io: destination already exists".into());
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("io: {e}"))?;
    }
    std::fs::rename(&from, &to).map_err(|e| format!("io: {e}"))?;
    Ok(serde_json::json!({ "ok": true }))
}

// ui_rpc.rs 的 dispatch_with match 加两臂:
"host.vault.remove" => vault_remove(services, &req.params),
"host.vault.rename" => vault_rename(services, &req.params),
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS(全量,含集成测试;基线 561,应变成 563+)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin_runtime/host_api.rs src-tauri/src/plugin_runtime/ui_rpc.rs
git commit -m "feat(plugin-bridge): host.vault.remove / host.vault.rename(vault.write)"
```

---

### Task 2: Editor Kit —— rich 模式的占位符

**Files:**
- Modify: `src/editor-kit/rich.ts`(挂 placeholder 插件)
- Modify: `src/editor-kit/main.ts`(把 `placeholder` 透传给 rich)
- Modify: `src/editor-kit/kit.css`(自带 `::before` 规则)
- Test: `src/editor-kit/rich.test.ts`(新建)

**Interfaces:**
- Consumes: `src/lib/placeholder-plugin.ts` 的 `placeholderPlugin(text: string): Plugin`(既有,纯 ProseMirror,零 IPC)。
- Produces: `mountRich(host, initial, baseDir, onChange, placeholder?)` —— 新增第 5 个可选参数;`KitOptions.placeholder` 从此在 rich 与 source 两种模式下都生效(v1 API 形状不变)。

- [ ] **Step 1: 写失败测试**

```ts
// src/editor-kit/rich.test.ts
import { describe, it, expect } from 'vitest'
import { placeholderPlugin } from '../lib/placeholder-plugin'

describe('kit rich placeholder', () => {
  it('is part of the plugin set only when a placeholder was given', async () => {
    // mountRich 需要真实 DOM + moraya,jsdom 下挂不起来;这里验证的是接线
    // 契约:传了 placeholder 才追加插件、且插件带的正是那段文字。
    const { richPlugins } = await import('./rich')
    expect(richPlugins(undefined)).toHaveLength(0)
    const withText = richPlugins('写点什么')
    expect(withText).toHaveLength(1)
    // 插件的 decorations 对空文档产出带 data-placeholder 的装饰
    expect(withText[0]).toBeInstanceOf(placeholderPlugin('x').constructor)
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test src/editor-kit/rich`
Expected: FAIL(`richPlugins` 未导出)

- [ ] **Step 3: 实现**

```ts
// rich.ts 顶部新增 import(placeholder-plugin 只依赖 prosemirror-state/view,
// 零 Tauri IPC,符合 kit 的依赖白名单):
import { placeholderPlugin } from '../lib/placeholder-plugin'
import type { Plugin } from 'prosemirror-state'

/** 传了提示文字才追加插件。抽成纯函数以便在没有 DOM 的测试里验证接线。 */
export function richPlugins(placeholder: string | undefined): Plugin[] {
  return placeholder ? [placeholderPlugin(placeholder)] : []
}

// mountRich 签名加第 5 参 `placeholder?: string`;createEditor 返回后:
const extra = richPlugins(placeholder)
if (extra.length) {
  instance.view.updateState(
    instance.view.state.reconfigure({
      plugins: instance.view.state.plugins.concat(extra),
    }),
  )
}
return instance
```

`main.ts` 里调用 `mountRich(host, markdown, '', emit, opts.placeholder)`。

`kit.css` 追加(规则抄自主程序 `src/components/RichEditor.svelte` 的局部样式,那里是 scoped 的所以 kit 必须自带一份;两处各留一行注释互指):

```css
/* 空文档的提示文字。规则与主程序 RichEditor.svelte 的同名样式保持一致 ——
   placeholder-plugin 只挂 data-placeholder,渲染全靠这条。 */
.kit-host .moraya-editor .is-empty::before {
  content: attr(data-placeholder);
  color: color-mix(in srgb, CanvasText 38%, Canvas);
  float: left;
  height: 0;
  pointer-events: none;
}
```

- [ ] **Step 4: 跑测试与构建**

Run: `pnpm test src/editor-kit && pnpm check && pnpm build`
Expected: PASS;`dist/assets/editor-kit-v1.js` 与 `.css` 仍产出(构建脚本自带断言)

- [ ] **Step 5: Commit**

```bash
git add src/editor-kit/rich.ts src/editor-kit/main.ts src/editor-kit/kit.css src/editor-kit/rich.test.ts
git commit -m "feat(editor-kit): rich 模式支持 placeholder(纳入 placeholder-plugin)"
```

---

### Task 3: 占位符金句与轮换

**Files:**
- Create: `plugins-src/idea-spark/src/lib/placeholder.ts`
- Test: `plugins-src/idea-spark/src/lib/placeholder.test.ts`
- Modify: `plugins-src/idea-spark/src/lib/strings.ts`(加 5 个 key × 4 语;**删掉** `templateH1`/`templateHint`/`sectionDomain`/`sectionTransfer`/`sectionResources`/`sectionOutcome` 六个不再使用的 key)

**Interfaces:**
- Produces:
```ts
export const PLACEHOLDER_KEYS = ['ph1', 'ph2', 'ph3', 'ph4', 'ph5'] as const
export function placeholderLines(): string[]          // 按当前 locale 取五句
export function pickPlaceholder(lines: string[], seq: number): string  // lines[seq % len],lines 为空时返回 ''
```

五句文案(**全部无句末句号**),四语按母语重写而非直译:

| key | zh | en |
|---|---|---|
| ph1 | 写小说有三条规矩,可惜没人知道是哪三条 —— 毛姆 | Three rules for writing a novel — unfortunately, nobody knows what they are — Maugham |
| ph2 | 想法像兔子,养两只很快就一打 —— 斯坦贝克 | Ideas are like rabbits — get a couple and soon you have a dozen — Steinbeck |
| ph3 | 写作很简单,盯着白纸直到额头渗出血珠 —— 吉恩·福勒 | Writing is easy, you just stare at a blank page until your forehead bleeds — Gene Fowler |
| ph4 | 灵感是业余选手的事 —— 查克·克洛斯 | Inspiration is for amateurs — Chuck Close |
| ph5 | 这封信写长了,因为我没时间把它写短 —— 帕斯卡 | I made this longer because I had no time to make it shorter — Pascal |

| key | ja | de |
|---|---|---|
| ph1 | 小説の書き方には三つの規則がある、残念ながら誰も知らない —— モーム | Es gibt drei Regeln für einen Roman, leider kennt sie niemand — Maugham |
| ph2 | アイデアはウサギに似ている、二匹いればすぐ一ダース —— スタインベック | Ideen sind wie Kaninchen, zwei werden schnell ein Dutzend — Steinbeck |
| ph3 | 書くのは簡単、額から血がにじむまで白紙を見つめるだけ —— ジーン・ファウラー | Schreiben ist leicht, starr auf das leere Blatt, bis dir Blut auf der Stirn steht — Gene Fowler |
| ph4 | ひらめきは素人のもの —— チャック・クローズ | Inspiration ist etwas für Amateure — Chuck Close |
| ph5 | 短くする時間がなかったので長くなった —— パスカル | Dieser Brief wurde lang, weil ich keine Zeit hatte, ihn kurz zu machen — Pascal |

- [ ] **Step 1: 写失败测试**

```ts
import { describe, it, expect } from 'vitest'
import { pickPlaceholder, placeholderLines, PLACEHOLDER_KEYS } from './placeholder'
import { setLocale } from './strings'

describe('pickPlaceholder', () => {
  const lines = ['a', 'b', 'c', 'd', 'e']
  it('cycles through every line before repeating', () => {
    expect([0, 1, 2, 3, 4].map((n) => pickPlaceholder(lines, n))).toEqual(lines)
    expect(pickPlaceholder(lines, 5)).toBe('a')
    expect(pickPlaceholder(lines, 12)).toBe('c')
  })
  it('survives a negative or fractional counter without throwing', () => {
    expect(typeof pickPlaceholder(lines, -1)).toBe('string')
    expect(typeof pickPlaceholder(lines, 2.7)).toBe('string')
  })
  it('returns an empty string for an empty pool', () => {
    expect(pickPlaceholder([], 3)).toBe('')
  })
})

describe('placeholderLines', () => {
  it('gives five non-empty lines in every locale, none ending in a full stop', () => {
    for (const locale of ['en', 'zh', 'ja', 'de'] as const) {
      setLocale(locale)
      const lines = placeholderLines()
      expect(lines, locale).toHaveLength(PLACEHOLDER_KEYS.length)
      for (const line of lines) {
        expect(line.trim().length, `${locale}: ${line}`).toBeGreaterThan(0)
        expect(/[。.!!??]$/.test(line.trim()), `${locale} 不该以句号结尾: ${line}`).toBe(false)
      }
    }
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm --filter idea-spark test placeholder`
Expected: FAIL(模块不存在)

- [ ] **Step 3: 实现**

```ts
// placeholder.ts
import { t } from './strings'

/** 空白文档的灰字提示。五句轮换,各拆一个不肯动笔的借口;出处均可考。 */
export const PLACEHOLDER_KEYS = ['ph1', 'ph2', 'ph3', 'ph4', 'ph5'] as const

export function placeholderLines(): string[] {
  return PLACEHOLDER_KEYS.map((k) => t(k))
}

/** `lines[seq % len]`。计数器而非随机:五句都会轮到、行为可预测、测试不必注入种子。 */
export function pickPlaceholder(lines: string[], seq: number): string {
  if (lines.length === 0) return ''
  const n = Number.isFinite(seq) ? Math.floor(seq) : 0
  return lines[((n % lines.length) + lines.length) % lines.length]
}
```

`strings.ts`:`MESSAGE_KEYS` 删六个模板 key、加 `ph1`–`ph5`,四个 catalog 同步(映射类型会强制你改全,漏了 `pnpm --filter idea-spark check` 直接报 TS2739)。

- [ ] **Step 4: 跑测试与类型检查**

Run: `pnpm --filter idea-spark test && pnpm --filter idea-spark check`
Expected: PASS / 0 errors

- [ ] **Step 5: Commit**

```bash
git add plugins-src/idea-spark/src/lib/placeholder.ts plugins-src/idea-spark/src/lib/placeholder.test.ts plugins-src/idea-spark/src/lib/strings.ts
git commit -m "feat(idea-spark): 空白文档的五句轮换金句(四语,无句末句号)"
```

---

### Task 4: 时间戳命名、空白文档、新增 store 字段

**Files:**
- Modify: `plugins-src/idea-spark/src/lib/naming.ts`(新增 `timestampFileName`)
- Modify: `plugins-src/idea-spark/src/lib/state-io.ts`(`SparkState` 加 `inboxOpen` / `placeholderSeq`)
- Modify: `plugins-src/idea-spark/src/lib/store.svelte.ts`(`ideaTemplate` → 空串、`nextFileName` 改用时间戳、`SparkStore` 加字段)
- Test: 同目录既有 `naming.test.ts` / `state-io.test.ts` / `store.test.ts`

**Interfaces:**
- Produces:
```ts
// naming.ts
export function timestampFileName(now: Date, taken: Set<string>): string
//   `YYYY-MM-DD-HHmm-idea.md`(本地时间,非 UTC);撞名追加 `-2`/`-3`…
// state-io.ts
export interface SparkState {
  ideaDir: string
  pendingRuns: Record<string, string>
  inboxOpen: boolean        // 新增,默认 false
  placeholderSeq: number    // 新增,默认 0
}
// store.svelte.ts 的 SparkStore 新增:
//   inboxOpen: boolean
//   placeholderSeq: number
//   saveState: { kind: 'idle' } | { kind: 'saving' } | { kind: 'saved'; at: string } | { kind: 'failed'; message: string }
```
- `nextFileName(s, markdown, nowIso)` 第三参语义从「YYYY-MM-DD 字符串」改为**完整时刻**;调用方同步更新。`slugFromMarkdown` 保留但降级为 **inbox 行标题来源**,注释与测试要改。

- [ ] **Step 1: 写失败测试**

```ts
// naming.test.ts 追加
describe('timestampFileName', () => {
  const at = new Date(2026, 7, 4, 19, 42) // 本地时间 2026-08-04 19:42
  it('names by creation minute, not by title', () => {
    expect(timestampFileName(at, new Set())).toBe('2026-08-04-1942-idea.md')
  })
  it('pads single-digit month/day/hour/minute', () => {
    expect(timestampFileName(new Date(2026, 0, 2, 3, 4), new Set())).toBe('2026-01-02-0304-idea.md')
  })
  it('suffixes on collision inside the same minute', () => {
    const taken = new Set(['2026-08-04-1942-idea.md', '2026-08-04-1942-idea-2.md'])
    expect(timestampFileName(at, taken)).toBe('2026-08-04-1942-idea-3.md')
  })
})

// state-io.test.ts 追加
it('defaults the new fields and tolerates wrong types', () => {
  expect(parseState(null).inboxOpen).toBe(false)
  expect(parseState(null).placeholderSeq).toBe(0)
  expect(parseState('{"inboxOpen":"yes","placeholderSeq":"x"}').inboxOpen).toBe(false)
  expect(parseState('{"inboxOpen":"yes","placeholderSeq":"x"}').placeholderSeq).toBe(0)
  expect(parseState('{"inboxOpen":true,"placeholderSeq":7}').placeholderSeq).toBe(7)
})
it('round-trips the new fields', () => {
  const s = { ...DEFAULT_STATE, inboxOpen: true, placeholderSeq: 3 }
  expect(parseState(serializeState(s))).toEqual(s)
})

// store.test.ts 追加
it('starts a new idea blank — no template', () => {
  expect(ideaTemplate()).toBe('')
})
it('names a never-saved idea by timestamp and keeps the name afterwards', () => {
  const s = createStore()
  s.ideaDir = 'inbox/ideas'
  const at = new Date(2026, 7, 4, 19, 42).toISOString()
  expect(nextFileName(s, '# 随便什么标题', at)).toBe('2026-08-04-1942-idea.md')
  s.current = '2026-08-04-1942-idea.md'
  expect(nextFileName(s, '# 改了标题', new Date(2026, 7, 5, 8, 0).toISOString())).toBe(
    '2026-08-04-1942-idea.md',
  )
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm --filter idea-spark test`
Expected: FAIL

- [ ] **Step 3: 实现**

```ts
// naming.ts
/**
 * `YYYY-MM-DD-HHmm-idea.md`,取**创建时刻的本地时间**(`toISOString()` 会把傍晚
 * 的想法记成第二天)。刻意不按标题命名:自动保存会在用户写出标题之前就落盘,
 * 而事后改名会把一条 idea 散到多个文件里。撞名(同一分钟内连开两条)追加序号。
 */
export function timestampFileName(now: Date, taken: Set<string>): string {
  const p = (n: number) => String(n).padStart(2, '0')
  const base = `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}-idea`
  let name = `${base}.md`
  let n = 2
  while (taken.has(name)) {
    name = `${base}-${n}.md`
    n += 1
  }
  return name
}
```

```ts
// store.svelte.ts
/** 新文档是空白的:进窗口就写字,提示只由灰字占位符承担。 */
export function ideaTemplate(): string {
  return ''
}

export function nextFileName(s: SparkStore, _markdown: string, nowIso: string): string {
  return s.current ?? timestampFileName(new Date(nowIso), new Set(fileNames(s)))
}
```

`state-io.ts` 的 `parseState` 对两个新键逐键独立回落(布尔非 `true` 一律 false;数字非有限值一律 0),`DEFAULT_STATE` 补 `inboxOpen: false, placeholderSeq: 0`。`SparkStore` 与 `createStore()` 补三个字段(`inboxOpen`、`placeholderSeq`、`saveState: { kind: 'idle' }`)。

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm --filter idea-spark test && pnpm --filter idea-spark check`
Expected: PASS(既有 131 条不应变红——`slugFromMarkdown` 的测试保留,只改注释语义)

- [ ] **Step 5: Commit**

```bash
git add plugins-src/idea-spark/src/lib/naming.ts plugins-src/idea-spark/src/lib/naming.test.ts plugins-src/idea-spark/src/lib/state-io.ts plugins-src/idea-spark/src/lib/state-io.test.ts plugins-src/idea-spark/src/lib/store.svelte.ts plugins-src/idea-spark/src/lib/store.test.ts
git commit -m "feat(idea-spark): 空白文档 + 时间戳命名 + inboxOpen/placeholderSeq/saveState"
```

---

### Task 5: 自动保存(纯逻辑)

**Files:**
- Create: `plugins-src/idea-spark/src/lib/autosave.ts`
- Test: `plugins-src/idea-spark/src/lib/autosave.test.ts`

**Interfaces:**
- Produces:
```ts
export const AUTOSAVE_MS = 1500
export interface Autosave {
  /** 内容变了:重排定时器。 */
  schedule(): void
  /** 立刻写盘(切换/关窗/委托/Cmd+S),并取消待触发的定时器。 */
  flush(): Promise<void>
  /** 拆除:取消定时器,不写盘。 */
  dispose(): void
}
export function createAutosave(save: () => Promise<void>, delayMs?: number): Autosave
```
语义:`schedule()` 连续调用只保留最后一次;`flush()` 在有待写内容时调 `save()`、无则直接 resolve;`save()` 抛错不得让定时器链断掉(下一次 `schedule` 仍能工作)。

- [ ] **Step 1: 写失败测试**

```ts
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createAutosave } from './autosave'

beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())

describe('createAutosave', () => {
  it('saves once after the user stops typing', async () => {
    const save = vi.fn().mockResolvedValue(undefined)
    const a = createAutosave(save, 1500)
    a.schedule(); a.schedule(); a.schedule()
    expect(save).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(1500)
    expect(save).toHaveBeenCalledTimes(1)
  })
  it('flush writes immediately and cancels the pending timer', async () => {
    const save = vi.fn().mockResolvedValue(undefined)
    const a = createAutosave(save, 1500)
    a.schedule()
    await a.flush()
    expect(save).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(3000)
    expect(save).toHaveBeenCalledTimes(1) // 定时器没有再打一次
  })
  it('flush without pending work does not call save', async () => {
    const save = vi.fn().mockResolvedValue(undefined)
    await createAutosave(save, 1500).flush()
    expect(save).not.toHaveBeenCalled()
  })
  it('keeps working after save throws', async () => {
    const save = vi.fn().mockRejectedValueOnce(new Error('disk full')).mockResolvedValue(undefined)
    const a = createAutosave(save, 1500)
    a.schedule()
    await vi.advanceTimersByTimeAsync(1500)
    a.schedule()
    await vi.advanceTimersByTimeAsync(1500)
    expect(save).toHaveBeenCalledTimes(2)
  })
  it('dispose cancels without saving', async () => {
    const save = vi.fn().mockResolvedValue(undefined)
    const a = createAutosave(save, 1500)
    a.schedule()
    a.dispose()
    await vi.advanceTimersByTimeAsync(3000)
    expect(save).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm --filter idea-spark test autosave`
Expected: FAIL(模块不存在)

- [ ] **Step 3: 实现**(注入 `save` 与延时以便单测;定时器用 `setTimeout` 链,不进 `$effect`)

```ts
export const AUTOSAVE_MS = 1500

export interface Autosave {
  schedule(): void
  flush(): Promise<void>
  dispose(): void
}

/**
 * 停笔 `delayMs` 后写盘。`save` 抛错只被吞掉(调用方自己把失败反映到 UI),
 * 但不能让后续的 schedule 失效 —— 磁盘临时写不进去不该让自动保存从此罢工。
 */
export function createAutosave(save: () => Promise<void>, delayMs = AUTOSAVE_MS): Autosave {
  let timer: ReturnType<typeof setTimeout> | null = null
  let pending = false
  const run = async () => {
    timer = null
    if (!pending) return
    pending = false
    try { await save() } catch { /* 调用方负责显示失败 */ }
  }
  return {
    schedule() {
      pending = true
      if (timer != null) clearTimeout(timer)
      timer = setTimeout(() => void run(), delayMs)
    },
    async flush() {
      if (timer != null) { clearTimeout(timer); timer = null }
      await run()
    },
    dispose() {
      if (timer != null) { clearTimeout(timer); timer = null }
      pending = false
    },
  }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm --filter idea-spark test autosave`
Expected: PASS(5 条)

- [ ] **Step 5: Commit**

```bash
git add plugins-src/idea-spark/src/lib/autosave.ts plugins-src/idea-spark/src/lib/autosave.test.ts
git commit -m "feat(idea-spark): 自动保存的防抖/flush 纯逻辑"
```

---

### Task 6: 布局重做 —— 去标题栏、悬浮 ModeToggle、动作条、接上自动保存

**Files:**
- Create: `plugins-src/idea-spark/src/components/ModeToggle.svelte`
- Modify: `plugins-src/idea-spark/src/App.svelte`
- Modify: `plugins-src/idea-spark/src/lib/store.svelte.ts`(`saveState` 的写入点、`newIdea` 递增 `placeholderSeq` 并落盘)
- Modify: `plugins-src/idea-spark/src/lib/strings.ts`(加 `saving` / `saveFailed` / `inbox` 三个 key × 4 语;`history` 键改名为 `inbox` 或保留 `history` 作为面板标题——二选一,在报告里说明)

**Interfaces:**
- Consumes: Task 2 的 kit `placeholder` 选项、Task 3 的 `placeholderLines`/`pickPlaceholder`、Task 5 的 `createAutosave`。
- Produces: 动作条布局与 `saveState` 的 UI 呈现,供 Task 7/8 继续挂按钮。

- [ ] **Step 1: ModeToggle 组件**

视觉照抄主程序 `src/components/ModeToggle.svelte`(去工作树读该文件,逐样式复制:`.seg` 容器 `color-mix(in srgb, CanvasText 9%, Canvas)` 底、`border-radius:8px`、`padding:2px`;按钮 `32×26`、`opacity:.5`、hover `.85`、`.active` 白底 + `box-shadow: 0 1px 2px rgba(0,0,0,.12)`;两个 16×16 SVG:眼睛 = rich,`</>` = source)。文件头注明「复制自主程序,上游改动需同步」。

```svelte
<script lang="ts">
  import { t } from '../lib/strings'
  let { mode, onchange }: { mode: 'rich' | 'source'; onchange: (m: 'rich' | 'source') => void } = $props()
</script>
```

- [ ] **Step 2: App.svelte 结构改造**

- 删除 `<header class="topbar">` 整块(连同 `t('title')` 的使用);`SettingsPopover` 的触发按钮移进动作条。
- 编辑区容器加 `position: relative`,内部渲染 `{#if !store.kitFailed}<div class="float-toggle"><ModeToggle {mode} onchange={switchMode} /></div>{/if}`,样式 `position:absolute; top:0; right:12px; z-index:10`。
- 动作条:左侧 `saveState` 文案(`idle` 显示空、`saving` → `t('saving')`、`saved` → `已保存 HH:mm`、`failed` → `t('saveFailed')` + 告警色 + 点击重试);右侧 `新想法` / `委托 Agent`(Task 8 前保持 `disabled`)/ `📥 inbox`(Task 7 前保持 `disabled`)/ `⚙`。**删除保存按钮**。
- 挂 kit 时传 `placeholder: pickPlaceholder(placeholderLines(), store.placeholderSeq)`。
- 自动保存接线:`const autosave = createAutosave(() => saveIdea(markdown()).then(() => {}))`;`onEdited` 里 `markEdited(...)` 之后 `autosave.schedule()`;`Cmd/Ctrl+S`、`beforeunload`、切换 idea、新建、切模式前调 `await autosave.flush()`;`onMount` 的清理函数里 `autosave.dispose()`。
- **空文档不落盘**:`saveIdea` 开头判断 `markdown().trim() === ''` 时直接返回 null,不创建文件。

- [ ] **Step 3: 跑测试与构建**

Run: `pnpm --filter idea-spark test && pnpm --filter idea-spark check && pnpm --filter idea-spark build`
Expected: 全绿、0 error、构建成功

- [ ] **Step 4: Commit**

```bash
git add plugins-src/idea-spark/src/components/ModeToggle.svelte plugins-src/idea-spark/src/App.svelte plugins-src/idea-spark/src/lib/store.svelte.ts plugins-src/idea-spark/src/lib/store.test.ts plugins-src/idea-spark/src/lib/strings.ts
git commit -m "feat(idea-spark): 去标题栏、悬浮模式切换、动作条与自动保存接线"
```

---

### Task 7: inbox 面板 —— 隐藏/展开、右键菜单、删除与重命名

**Files:**
- Create: `plugins-src/idea-spark/src/components/ContextMenu.svelte`
- Create: `plugins-src/idea-spark/src/components/ConfirmDialog.svelte`
- Create: `plugins-src/idea-spark/src/components/InboxPanel.svelte`(替换 `HistoryList.svelte`,删掉后者)
- Modify: `plugins-src/idea-spark/src/lib/bridge.ts`(加 `vaultRemove` / `vaultRename` 包装)
- Modify: `plugins-src/idea-spark/src/lib/store.svelte.ts`(加 `deleteIdea` / `renameIdea` / `toggleInbox`、纯函数 `filesToDelete` / `validateRename`)
- Modify: `plugins-src/idea-spark/src/App.svelte`(inbox 按钮接线、面板挂载)
- Modify: `plugins-src/idea-spark/src/lib/strings.ts`(菜单项与确认框文案 × 4 语)
- Test: `plugins-src/idea-spark/src/lib/store.test.ts`

**Interfaces:**
- Consumes: Task 1 的 `host.vault.remove` / `host.vault.rename`。
- Produces:
```ts
// bridge.ts
export function vaultRemove(path: string): Promise<{ ok: true }>
export function vaultRename(from: string, to: string): Promise<{ ok: true }>
// store.svelte.ts(纯函数,先测)
export function filesToDelete(s: SparkStore, name: string): string[]
//   → [ideaRel] 或 [ideaRel, proofRel](proof 存在时);顺序:idea 在前
export function validateRename(s: SparkStore, from: string, raw: string): { ok: true; name: string } | { ok: false; reason: 'empty' | 'slash' | 'dot' | 'taken' }
//   自动补 `.md`;拒绝空/含 `/`/以 `.` 开头/与既有文件撞名(与自身同名视为 ok)
```

- [ ] **Step 1: 写失败测试**

```ts
describe('filesToDelete', () => {
  it('includes the proof sidecar when it exists', () => {
    const s = createStore()
    s.ideaDir = 'inbox/ideas'
    s.files = ['inbox/ideas/a.md', 'inbox/ideas/a.proof.md', 'inbox/ideas/b.md']
    expect(filesToDelete(s, 'a.md')).toEqual(['inbox/ideas/a.md', 'inbox/ideas/a.proof.md'])
    expect(filesToDelete(s, 'b.md')).toEqual(['inbox/ideas/b.md'])
  })
})

describe('validateRename', () => {
  const s = createStore()
  s.ideaDir = 'inbox/ideas'
  s.files = ['inbox/ideas/a.md', 'inbox/ideas/taken.md']
  it('appends .md and accepts a free name', () => {
    expect(validateRename(s, 'a.md', '新名字')).toEqual({ ok: true, name: '新名字.md' })
    expect(validateRename(s, 'a.md', '新名字.md')).toEqual({ ok: true, name: '新名字.md' })
  })
  it('renaming to its own name is fine', () => {
    expect(validateRename(s, 'a.md', 'a')).toEqual({ ok: true, name: 'a.md' })
  })
  it('rejects empty, slashes, leading dots and taken names', () => {
    expect(validateRename(s, 'a.md', '   ')).toEqual({ ok: false, reason: 'empty' })
    expect(validateRename(s, 'a.md', 'x/y')).toEqual({ ok: false, reason: 'slash' })
    expect(validateRename(s, 'a.md', '.hidden')).toEqual({ ok: false, reason: 'dot' })
    expect(validateRename(s, 'a.md', 'taken')).toEqual({ ok: false, reason: 'taken' })
  })
})
```

- [ ] **Step 2: 跑测试确认失败 → 实现纯函数 → 通过**

Run: `pnpm --filter idea-spark test store`

- [ ] **Step 3: 组件与动作**

- `ContextMenu.svelte`:`{ x, y, items, onclose }` props。绝对定位、边界翻转(靠近窗口右/下边时改向左/上展开)、`Esc` 关闭、上下方向键移动焦点、`Enter` 触发、点击外部关闭(`window` 上 capture 阶段 `mousedown`)、危险项 `class="danger"`。
- `ConfirmDialog.svelte`:`{ title, lines, confirmLabel, onconfirm, oncancel }`,`lines` 逐条列出将删的文件路径。
- `InboxPanel.svelte`:240px 右侧面板;行显示 `slugFromMarkdown(正文)` → 首行非空 → 退回 `displayName(name)`,加相对时间与状态标记(`✦`/`⏳`/`⚠`);`oncontextmenu` 阻止默认并打开 `ContextMenu`;保留既有 `listFailed` 告警条。
- `store` 动作:`deleteIdea(name)`(确认后逐个 `vaultRemove`,刷新列表;删的是当前文档时清空编辑器回到空白草稿)、`renameIdea(from, raw)`(`validateRename` → `vaultRename` → 若是当前文档则更新 `state.current`)、`toggleInbox()`(翻转并落盘)。

- [ ] **Step 4: 跑测试与构建**

Run: `pnpm --filter idea-spark test && pnpm --filter idea-spark check && pnpm --filter idea-spark build`
Expected: 全绿

- [ ] **Step 5: Commit**

```bash
git add plugins-src/idea-spark/src/components/ plugins-src/idea-spark/src/lib/bridge.ts plugins-src/idea-spark/src/lib/store.svelte.ts plugins-src/idea-spark/src/lib/store.test.ts plugins-src/idea-spark/src/lib/strings.ts plugins-src/idea-spark/src/App.svelte
git commit -m "feat(idea-spark): inbox 面板(可隐藏)+ 右键删除/重命名/打开"
```

---

### Task 8: 委托链路

**Files:**
- Create: `plugins-src/idea-spark/src/lib/agent-client.ts`
- Test: `plugins-src/idea-spark/src/lib/agent-client.test.ts`
- Modify: `plugins-src/idea-spark/src/lib/bridge.ts`(加 `agentRun` / `agentStatus`)
- Modify: `plugins-src/idea-spark/manifest.v2.json`(`capabilities` 加 `"agent"`)
- Modify: `plugins-src/idea-spark/src/App.svelte`(委托按钮启用、轮询、欢庆)
- Modify: `plugins-src/idea-spark/src/lib/store.svelte.ts`(boot 时校正 `pending`)
- Modify: `plugins-src/idea-spark/src/lib/strings.ts`(委托相关文案;删掉不再需要的 `delegateDeferred`)

**Interfaces:**
- Consumes(宿主既有,已在 main):`host.agent.run` / `host.agent.status`,capability `agent`。
- Produces:
```ts
export const TASK_ID = 'idea-proof'
export type DelegateResult =
  | { ok: true; runId: string }
  | { ok: false; reason: 'agent-missing' | 'error'; message: string }
export async function delegateIdea(ideaRel: string, title: string, vaultRoot: string): Promise<DelegateResult>
export type RunView =
  | { kind: 'running'; steps: number; last: string }
  | { kind: 'done'; success: boolean; message: string }
  | { kind: 'lost' }
export function interpretStatus(raw: unknown): RunView
export const POLL_MS = 2000
```

**关键接口事实(已核实,照此实现)**:
- `host.agent.run` 参数:`task`(必需)、`prompt`、`note_path`(**绝对路径且文件必须已存在**,claude-agent 用 `canonicalize`)、`notify`。`notify` 的四个字段 **全必需**:`title_ok` / `title_fail` / `open_path`(绝对) / `expect_file`(绝对),解析失败会直接报错。返回 `{ run_id }`。
- `host.agent.status` 参数:`run_id`(必需)、`task`(**必须显式传**,默认值是 `answer-note-question`)。返回 `{state:'done',record}` / `{state:'running',steps,last}` / `{state:'lost'}`。
- claude-agent 未装/未启用:错误消息前缀 `agent_unavailable:`(码是 -32000,**没有专用错误码**),据此判 `agent-missing`。
- **不要**给 manifest 加 `notify` capability:提醒由 claude-agent 代发,托盘注册表**没有去重**,两边都推会出现两条。

- [ ] **Step 1: 写失败测试**(mock `window.notemd.request`,按调用序断言)

```ts
it('sends the run with a complete notify spec and absolute paths', async () => {
  const request = vi.fn().mockResolvedValue({ run_id: 'r1' })
  ;(window as any).notemd = { request }
  const r = await delegateIdea('inbox/ideas/a.md', '我的想法', '/V')
  expect(r).toEqual({ ok: true, runId: 'r1' })
  const [method, params] = request.mock.calls.at(-1)!
  expect(method).toBe('host.agent.run')
  expect(params.task).toBe('idea-proof')
  expect(params.note_path).toBe('/V/inbox/ideas/a.md')
  expect(params.notify.open_path).toBe('/V/inbox/ideas/a.proof.md')
  expect(params.notify.expect_file).toBe('/V/inbox/ideas/a.proof.md')
  expect(params.notify.title_ok).toContain('我的想法')
  expect(params.notify.title_fail).toContain('我的想法')
})
it('maps the agent_unavailable prefix to agent-missing', async () => {
  ;(window as any).notemd = { request: vi.fn().mockRejectedValue(new Error('-32000: agent_unavailable: unknown v2 plugin')) }
  const r = await delegateIdea('inbox/ideas/a.md', 't', '/V')
  expect(r).toMatchObject({ ok: false, reason: 'agent-missing' })
})
it('interprets every status shape', () => {
  expect(interpretStatus({ state: 'running', steps: 3, last: 'Read a.md' })).toEqual({ kind: 'running', steps: 3, last: 'Read a.md' })
  expect(interpretStatus({ state: 'done', record: { status: 'success', result: 'ok' } })).toEqual({ kind: 'done', success: true, message: 'ok' })
  expect(interpretStatus({ state: 'done', record: { status: 'error', stderr_tail: 'boom' } })).toEqual({ kind: 'done', success: false, message: 'boom' })
  expect(interpretStatus({ state: 'lost' })).toEqual({ kind: 'lost' })
  expect(interpretStatus({ nonsense: 1 })).toEqual({ kind: 'lost' })
})
```

- [ ] **Step 2: 跑测试确认失败 → 实现 → 通过**

`delegateIdea` 顺序:`seedTaskTemplate`(既有,Task 12 已实现)→ 拼绝对路径 → `host.agent.run` → 返回 `run_id`。错误消息含 `agent_unavailable` 判 `agent-missing`,否则 `error`。

Run: `pnpm --filter idea-spark test agent-client`

- [ ] **Step 3: UI 接线**

- 委托按钮去掉 `disabled`;点击:`await autosave.flush()` → 空文档提示并终止 → `delegateIdea` → `markPending` + **落盘**(`persist()`)→ 行内状态转「论证中」。
- `agent-missing` → 弹层提示去插件市场装 claude-agent(复用既有 `agentMissing` / `agentMissingHint` 文案)。
- 轮询:`setTimeout` 链每 `POLL_MS` 调 `host.agent.status { task: TASK_ID, run_id }`;`done` → `applyRunDone` + 刷新列表 + 触发 `Celebration`;`lost` → 标失败。窗口卸载时停止(`onMount` 清理函数)。**不进 `$effect`**。
- boot 时对 `state.pending` 逐个 status 校正一次(done → 移出并落盘;lost → 标失败)。
- inbox 右键的「委托给 Agent」接同一条链路。

- [ ] **Step 4: 跑测试与构建**

Run: `pnpm --filter idea-spark test && pnpm --filter idea-spark check && pnpm --filter idea-spark build`
Expected: 全绿

- [ ] **Step 5: Commit**

```bash
git add plugins-src/idea-spark/src/lib/agent-client.ts plugins-src/idea-spark/src/lib/agent-client.test.ts plugins-src/idea-spark/src/lib/bridge.ts plugins-src/idea-spark/manifest.v2.json plugins-src/idea-spark/src/App.svelte plugins-src/idea-spark/src/lib/store.svelte.ts plugins-src/idea-spark/src/lib/strings.ts
git commit -m "feat(idea-spark): 委托链路——host.agent.run + notify 托盘提醒 + 轮询"
```

---

### Task 9: 全量回归、文档、手动验证清单

**Files:**
- Modify: `docs/plugin-v2-development.md`(§5 capability 表:`vault.write` 补 `host.vault.remove` / `host.vault.rename`)
- Modify: `docs/superpowers/2026-08-04-idea-spark-round1-followups.md`(勾掉本轮已解决的项:委托链路、以及第一轮遗留里被本轮顺带修掉的)

- [ ] **Step 1: 全量回归**

Run:
```bash
cargo test --manifest-path src-tauri/Cargo.toml
pnpm check && pnpm test
pnpm --filter idea-spark test && pnpm --filter idea-spark check && pnpm --filter idea-spark build
pnpm build
```
Expected: 全绿。基线 cargo 561 / pnpm 1751 / idea-spark 131,均应只增不减。**任何失败如实写进报告,不许掩盖**。

- [ ] **Step 2: 文档 + Commit**

```bash
git add docs/plugin-v2-development.md docs/superpowers/2026-08-04-idea-spark-round1-followups.md
git commit -m "docs(plugin-v2): 登记 host.vault.remove / host.vault.rename"
```

- [ ] **Step 3: dev 安装 + 交给用户的手动验证清单**(只起构建,不做桌面自动化)

```bash
pnpm build && bash scripts/dev-install-plugin.sh idea-spark
```
(`__host__` 在 dev 下读磁盘上的 `dist/`,所以必须先 `pnpm build`)

清单:
1. 窗口无标题栏,编辑区从顶端铺到底;模式切换在编辑区右上角,点击可切且内容不丢。
2. 新窗口是**空白**的,灰字是五句之一;反复「新想法」应轮到不同的句子;rich 与 source 两种模式下灰字都显示。
3. 打字停 1.5s 后动作条显示「已保存 HH:mm」;**没有保存按钮**;`Cmd+S` 立即保存。
4. vault 里出现 `inbox/ideas/YYYY-MM-DD-HHmm-idea.md`,带 OKF frontmatter。空白文档不产生文件。
5. `📥 inbox` 切换面板显隐,关窗重开保持上次状态;行显示的是**正文标题**不是文件名。
6. 右键一条 idea:委托 / 在主编辑器打开 / 重命名 / 删除四项齐全;删除弹确认并列出将删文件(含 `.proof.md`);重命名后名字固定不再自动改。
7. 委托 → 行内「论证中」→ 关窗 → 跑完托盘出提醒 → 点提醒直接打开 `.proof.md`;窗口开着时有欢庆动画。
8. 未装 claude-agent 时点委托,提示去市场安装。

---

## Self-Review 记录

- **Spec 覆盖**:§1 布局(Task 6)、§2 空白文档与轮换(Task 3/4/6)、§2.3 kit 占位符(Task 2)、§3 自动保存(Task 4/5/6)、§4 委托链路(Task 8)、§5 inbox 与右键(Task 7)、§6 两个桥方法(Task 1)、§7 文件结构(Task 6/7/8 各建各的)、§8 测试(各任务 + Task 9)、§9 不做(未引入守废纸篓/多选/搜索/随机)。
- **类型一致性**:`SparkState` 新字段(Task 4)↔ `toggleInbox`/`placeholderSeq` 消费(Task 6/7);`timestampFileName`(Task 4)↔ `nextFileName` 调用(Task 4 内改完);`Autosave`(Task 5)↔ App 接线(Task 6);`filesToDelete`/`validateRename`(Task 7)↔ 组件调用(同任务);`delegateIdea`/`interpretStatus`(Task 8)↔ UI(同任务)。
- **已知外部事实**均已核实并写进任务:`host.agent.*` 的参数/返回/错误前缀、`notify` 四字段全必需、`host.agent.status` 的 `task` 必须显式传、托盘提醒无去重(故不加 `notify` capability)、`placeholder-plugin.ts` 零 IPC 可进 kit、其 CSS 在主程序是 scoped 的所以 kit 要自带。
- **一处需实现者判断并在报告说明**:Task 6 的 `history` → `inbox` key 改名 vs 保留(二选一,不影响功能)。
