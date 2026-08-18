# 溯源(Trace to Source)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 选中文本 → 右键「溯源」/输入面 `/溯源` → 异步 agent 检索原始出处并下载字幕/正文 → 生成带反向链接的摘要 md 落 `traces/`,统一通知打开。

**Architecture:** 「指令 = task 模板」抽象:`task.json` 可选 `directive` 字段把任意 agent 任务变成 idea-spark 输入面的 `/xxx` 指令;右键路径经扩展后的 `plugin_v2_open_window`(query/push 双通道)把预填文本送进 idea-spark;抓取全交 harness(WebSearch/WebFetch/yt-dlp),宿主零抓取代码。

**Tech Stack:** Svelte 5(宿主前端 + idea-spark)、Rust/Tauri(宿主 + agent-run-core)、vitest、cargo test。

**Spec:** `docs/superpowers/specs/2026-08-18-trace-source-design.md`

## Global Constraints

- 不改任何 manifest 结构(`ManifestV2`/`Contributes` 是 `deny_unknown_fields`,加键=老宿主加载不出插件)。
- agent 永不写源 md、永不写 `.note.md`、永不用 `human:` 署名。
- 新 CONCEPT_TYPE 三件套:`src/lib/okf/concept.ts` 登记 + `searchidx/src/origin.rs` 定档 + `pnpm gen:origin-types` 重生成 fixture。
- 插件模板文本一律**数组拼行**,禁模板字面量(``` 与 `${VAULT}` 两个坑,见 `plugins-src/idea-spark/src/lib/task-template.ts:8-19`)。
- 插件 i18n:新增键必须进 `MESSAGE_KEYS` + 全部 4 个 catalog,`strings.test.ts` 自动把关。
- 主 worktree 常被共享:提交只精确 add 本任务文件,绝不 `git add -A`。
- 在 `.claude/worktrees/` 下开发时,`pnpm install` 前先 `ln -s ../../../moraya-core moraya-core`(worktree 相对位置的 `file:../moraya-core` 依赖)。
- GUI 改动(右键项、预填、chip)最终由用户手测,不做 UI 自动化;发版前须真机跑一次端到端溯源。

---

### Task 1: agent-run-core `TaskDef.directive` 字段

**Files:**
- Modify: `plugins-src/agent-run-core/src/task.rs:15-40`(struct)与同文件 `mod tests`

**Interfaces:**
- Produces: `TaskDef.directive: Vec<String>`(serde default 空)。无 Rust 消费者;它是 task.json 的**登记在案的 schema 扩展**,真正的读取方是 Task 5 的 idea-spark 前端。

- [ ] **Step 1: 写失败测试**(加进 `task.rs` 的 `mod tests`)

```rust
#[test]
fn directive_field_parses_and_defaults_empty() {
    let v = tempfile::tempdir().unwrap();
    write_task(v.path(), "with", r#"{"name":"T","directive":["溯源","trace"]}"#);
    write_task(v.path(), "without", r#"{"name":"U"}"#);
    let got = discover(v.path());
    assert_eq!(got[0].directive, vec!["溯源", "trace"]);
    assert!(got[1].directive.is_empty());
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/agent-run-core/Cargo.toml directive_field`
Expected: 编译失败 `no field directive`

- [ ] **Step 3: 加字段**(`task.rs` 的 `TaskDef`,`okf_type` 之后)

```rust
    /// 输入面指令名列表(如 ["溯源","trace"]),首项为展示名。非空 ⇒ 这个模板
    /// 可在 idea-spark 输入面以 `/名字` 调用。不参与运行语义,纯发现/展示。
    #[serde(default)]
    pub directive: Vec<String>,
```

- [ ] **Step 4: 跑测试确认通过 + 全量**

Run: `cargo test --manifest-path plugins-src/agent-run-core/Cargo.toml`
Expected: 全绿

- [ ] **Step 5: Commit**

```bash
git add plugins-src/agent-run-core/src/task.rs
git commit -m "feat(agent-run-core): TaskDef 增加 directive 字段——指令=task模板抽象"
```

---

### Task 2: OKF 新类型 `Trace Report` / `Trace Material` + origin 定档

**Files:**
- Modify: `src/lib/okf/concept.ts:21-47`(`CONCEPT_TYPE`)
- Modify: `searchidx/src/origin.rs:89-100`(`mapped_type_origin`)+ 同文件测试
- Regenerate: `searchidx/tests/fixtures/origin/concept-types.json`(经 `pnpm gen:origin-types`)

**Interfaces:**
- Produces: `CONCEPT_TYPE.traceReport === 'Trace Report'`(origin=Derived)、`CONCEPT_TYPE.traceMaterial === 'Trace Material'`(origin=Source)。Task 6 的模板 `okf_type` 引用 `"Trace Report"`。

- [ ] **Step 1: 先在 origin.rs 写失败测试**(加进 `rule4_maps_registered_types` 旁)

```rust
#[test]
fn trace_types_map_report_derived_material_source() {
    assert_eq!(derive("traces/a.md", Some(&fm("type: Trace Report")), &globs(&[])), Origin::Derived);
    assert_eq!(derive("traces/a/01-b.md", Some(&fm("type: Trace Material")), &globs(&[])), Origin::Source);
}
```

Run: `cargo test --manifest-path searchidx/Cargo.toml trace_types`
Expected: FAIL(两个都落到 rule 7 的 Derived,第二个断言不等)

- [ ] **Step 2: origin.rs 补档位**(`mapped_type_origin`)

```rust
        "Book Summary" | "Answer" | "Idea Proof" | "Reading Report" | "Decision Board"
        | "Decision Archive" | "Trace Report" => Some(Origin::Derived),
        "Book" | "Trace Material" => Some(Origin::Source),
```

- [ ] **Step 3: concept.ts 登记**(`vaultConventions` 之前插入)

```ts
  /** 溯源:agent 产出的溯源摘要 `traces/<date>-<time>.md`(idea-spark trace-source 任务) */
  traceReport: 'Trace Report',
  /** 溯源:下载的原始材料全文(字幕转写/博客正文/论文节选),`traces/<同名>/` 子目录 */
  traceMaterial: 'Trace Material',
```

- [ ] **Step 4: 重生成 fixture + 三处测试**

Run: `pnpm gen:origin-types && cargo test --manifest-path searchidx/Cargo.toml origin && pnpm vitest run src/lib/okf/concept-origin-sync.test.ts`
Expected: 全绿(fixture diff 出现两个新类型)

- [ ] **Step 5: Commit**

```bash
git add src/lib/okf/concept.ts searchidx/src/origin.rs searchidx/tests/fixtures/origin/concept-types.json
git commit -m "feat(okf): 登记 Trace Report(derived)/Trace Material(source) 概念类型"
```

---

### Task 3: 宿主 seed 通道——`plugin_v2_open_window` 带预填 payload

**Files:**
- Modify: `src-tauri/src/plugin_runtime/windows.rs:150-262`(`open_plugin_window` 加 query 形参;抽纯函数)
- Modify: `src-tauri/src/plugin_runtime/commands.rs:33-40`(`plugin_v2_open_window` 加 `seed`)
- Modify: `src-tauri/src/lib.rs:1841-1858` 与通知处理里的 `open_plugin_window` 调用点(补 `None` 实参)

**Interfaces:**
- Consumes: `windows::window_label` / `windows::push_to_window`(现成)。
- Produces: 前端可 `invoke('plugin_v2_open_window', { pluginId, windowId, seed })`,`seed` 为任意 JSON。语义:窗口**已开** → `push_to_window` 推 `{"type":"seed","payload":<seed>}`;窗口**新建** → URL 追加 `?seed=<urlencoded JSON>`(`protocol.rs` 用 `url.path()` 解析资产,query 无害)。Task 8 的 idea-spark 两个入口各接一路。

- [ ] **Step 1: 抽纯函数 + 失败测试**(windows.rs;URL 构造现在内联在 `open_plugin_window` 的 `format!("plugin://{plugin_id}/{}", win.entry)` 处)

```rust
/// 插件窗口 URL。`seed_json` 非空时挂成 `?seed=<urlencoded>`——protocol.rs 按
/// url.path() 解析资产,query 只被页面 JS 读取。
pub(crate) fn plugin_window_url(plugin_id: &str, entry: &str, seed_json: Option<&str>) -> String {
    let base = format!("plugin://{plugin_id}/{entry}");
    match seed_json {
        None => base,
        Some(s) => {
            let enc: String = url::form_urlencoded::byte_serialize(s.as_bytes()).collect();
            format!("{base}?seed={enc}")
        }
    }
}
```

测试(windows.rs `#[cfg(test)]`,若无则新建 mod):

```rust
#[test]
fn window_url_with_and_without_seed() {
    assert_eq!(plugin_window_url("a.b", "index.html", None), "plugin://a.b/index.html");
    let u = plugin_window_url("a.b", "index.html", Some(r#"{"t":"溯源 x"}"#));
    assert!(u.starts_with("plugin://a.b/index.html?seed=%7B%22t%22"));
    assert!(!u.contains('"'), "JSON 必须整体转义进 query");
}
```

Run: `cargo test --manifest-path src-tauri/Cargo.toml plugin_window_url` → 先 FAIL(函数不存在),实现后 PASS。
(若 `url` crate 不在 src-tauri 直接依赖里,用 `tauri::Url` 同源 re-export 或在 Cargo.toml 显式加 `url`——它已在依赖树中。)

- [ ] **Step 2: `open_plugin_window` 加形参并接线**

签名改为 `pub fn open_plugin_window(app: &AppHandle, plugin_id: &str, window_id: &str, seed_json: Option<&str>) -> Result<(), String>`,内部 URL 构造改用 `plugin_window_url(plugin_id, &win.entry, seed_json)`。**singleton 已开分支(windows.rs:174-181)行为不变**(focus 后 return;推送由调用方负责)。
所有既有调用点(`lib.rs` 托盘激活 + 通知 `OpenPluginWindow` 分支、`commands.rs`)补第 4 个实参 `None`。

Run: `cargo check --manifest-path src-tauri/Cargo.toml` → 无遗漏调用点。

- [ ] **Step 3: `plugin_v2_open_window` 命令扩 seed**(commands.rs,照 `lib.rs:1841-1858` 的 existed 判定抄)

```rust
#[tauri::command]
pub fn plugin_v2_open_window(
    app: tauri::AppHandle,
    plugin_id: String,
    window_id: String,
    seed: Option<serde_json::Value>,
) -> Result<(), String> {
    let existed = app
        .get_webview_window(&windows::window_label(&plugin_id, &window_id))
        .is_some();
    let seed_json = seed.as_ref().map(|s| s.to_string());
    // 新建窗口经 URL query 携带(eval 推送在 webview 加载前会落空);
    // 已开窗口 query 到不了(singleton 分支直接 focus+return),改走 push。
    windows::open_plugin_window(
        &app,
        &plugin_id,
        &window_id,
        if existed { None } else { seed_json.as_deref() },
    )?;
    if existed {
        if let Some(s) = seed {
            windows::push_to_window(
                &app, &plugin_id, &window_id,
                &serde_json::json!({ "type": "seed", "payload": s }),
            );
        }
    }
    Ok(())
}
```

(保持原函数既有的注册与错误路径;`push_to_window` 目前若为私有,改 `pub(crate)`。)

- [ ] **Step 4: 编译 + 既有测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 全绿(前端既有 `invoke('plugin_v2_open_window', {...})` 调用不传 seed,`Option` 自然兼容)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin_runtime/windows.rs src-tauri/src/plugin_runtime/commands.rs src-tauri/src/lib.rs
git commit -m "feat(host): plugin_v2_open_window 支持 seed 预填——新窗口走 URL query,已开窗口走 push"
```

---

### Task 4: 宿主右键「溯源」

**Files:**
- Create: `src/lib/context-menu/trace-action.ts`
- Create: `src/lib/context-menu/trace-action.test.ts`
- Modify: `src/lib/context-menu/menu-model.ts:58-63`、`src/lib/context-menu/icons.ts:18-36`
- Modify: `src/lib/context-menu/rich-actions.ts:112-117` 附近、`src/lib/context-menu/source-actions.ts:96-107` 附近
- Modify: `src/lib/i18n/en.ts:828`、`zh.ts:259`、`ja.ts:779`、`de.ts:779` 各自旁边
- Modify: `src/lib/context-menu/menu-model.test.ts`

**Interfaces:**
- Consumes: Task 3 的 `invoke('plugin_v2_open_window', { pluginId, windowId, seed })`;`activeTab()`(`src/lib/tabs.svelte.ts:48`,`Tab.filePath` 绝对路径);`sotvaultStore.vaultRoot`(`src/lib/sotvault.svelte.ts:21`)。
- Produces: `buildTraceSeed(selection, docPath, vaultRoot): string`(纯函数,预填 markdown)与 `openTraceDelegation(selection: string): Promise<void>`。

- [ ] **Step 1: 写 `buildTraceSeed` 失败测试**(trace-action.test.ts)

```ts
import { describe, expect, it } from 'vitest'
import { buildTraceSeed } from './trace-action'

describe('buildTraceSeed', () => {
  it('引用块逐行加 >、vault 内路径转相对、首行是 /溯源', () => {
    const s = buildTraceSeed('两行\n选区', '/V/notes/a.md', '/V')
    expect(s.startsWith('/溯源 \n\n')).toBe(true)
    expect(s).toContain('> 两行\n> 选区')
    expect(s).toContain('\n\n源文档: notes/a.md\n')
  })
  it('vault 外文档保留绝对路径,vaultRoot 为 null 同理', () => {
    expect(buildTraceSeed('x', '/elsewhere/b.md', '/V')).toContain('源文档: /elsewhere/b.md')
    expect(buildTraceSeed('x', '/elsewhere/b.md', null)).toContain('源文档: /elsewhere/b.md')
  })
  it('超长选区截断到 8000 字符', () => {
    const s = buildTraceSeed('好'.repeat(9000), '/V/a.md', '/V')
    expect(s.length).toBeLessThan(8600)
  })
  it('无路径时省略源文档行', () => {
    expect(buildTraceSeed('x', '', '/V')).not.toContain('源文档:')
  })
})
```

Run: `pnpm vitest run src/lib/context-menu/trace-action.test.ts` → FAIL(模块不存在)

- [ ] **Step 2: 实现 trace-action.ts**

```ts
// 右键「溯源」→ 打开 idea-spark 并预填 /溯源 委托文本。
// 选区文本即锚(answer-sites 同款约定);agent 侧协议见 trace-source 模板。
import { invoke } from '@tauri-apps/api/core'
import { activeTab } from '../tabs.svelte'
import { sotvaultStore } from '../sotvault.svelte'

/** 预填不是产物,只是委托底稿——超长选区截断即可,截断处如实标注。 */
const MAX_SELECTION = 8000

export function buildTraceSeed(selection: string, docPath: string, vaultRoot: string | null): string {
  let sel = selection.trim()
  if (sel.length > MAX_SELECTION) sel = `${sel.slice(0, MAX_SELECTION)}\n> …(选区过长已截断)`
  const quoted = sel.split('\n').map((l) => `> ${l}`).join('\n')
  const root = vaultRoot?.replace(/\/+$/, '')
  const rel = root && docPath.startsWith(`${root}/`) ? docPath.slice(root.length + 1) : docPath
  const source = docPath ? `\n\n源文档: ${rel}\n` : '\n'
  return `/溯源 \n\n${quoted}${source}`
}

export async function openTraceDelegation(selection: string): Promise<void> {
  const text = buildTraceSeed(selection, activeTab()?.filePath ?? '', sotvaultStore.vaultRoot)
  try {
    await invoke('plugin_v2_open_window', {
      pluginId: 'notemd.idea-spark',
      windowId: 'main',
      seed: { text },
    })
  } catch (e) {
    console.error('[trace] 打开 idea-spark 失败(插件未安装?):', e)
  }
}
```

Run: `pnpm vitest run src/lib/context-menu/trace-action.test.ts` → PASS

- [ ] **Step 3: 菜单项 + 图标 + i18n**

menu-model.ts `emphasis` 组 `question` 之后:

```ts
      item('trace', 'ctxmenu.trace', { emphasis: true, needsSelection: true, icon: 'trace' }),
```

icons.ts `PATHS` 加(放大镜):

```ts
  trace: '<circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.65" y2="16.65"/>',
```

i18n 四处(各自 `ctxmenu.question` 行后):

```ts
  'ctxmenu.trace': 'Trace source',   // en.ts
  'ctxmenu.trace': '溯源',            // zh.ts
  'ctxmenu.trace': '出典を探す',       // ja.ts
  'ctxmenu.trace': 'Quelle finden',  // de.ts
```

menu-model.test.ts(照 :22-32 既有姿势)加:

```ts
it('trace requires a selection', () => {
  const emphasis = getMenuModel({ hasSelection: false }).find((g) => g.id === 'emphasis')!
  const trace = emphasis.items.find((i) => i.id === 'trace')!
  expect(trace.needsSelection).toBe(true)
  expect(trace.icon).toBe('trace')
})
```

Run: `pnpm vitest run src/lib/context-menu/` → PASS;`pnpm check` → 无 i18n 键遗漏编译错误

- [ ] **Step 4: 两个适配器接动作**

rich-actions.ts(`question` case 之后):

```ts
        case 'trace': {
          const { from, to } = view.state.selection
          const text = view.state.doc.textBetween(from, to, '\n')
          if (!text.trim()) return
          const { openTraceDelegation } = await import('./trace-action')
          return openTraceDelegation(text)
        }
```

source-actions.ts(`question` case 之后):

```ts
        case 'trace': {
          const text = h.value().slice(h.el.selectionStart ?? 0, h.el.selectionEnd ?? 0)
          if (!text.trim()) return
          const { openTraceDelegation } = await import('./trace-action')
          return openTraceDelegation(text)
        }
```

Run: `pnpm check && pnpm vitest run src/lib/context-menu/` → PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/context-menu/trace-action.ts src/lib/context-menu/trace-action.test.ts \
  src/lib/context-menu/menu-model.ts src/lib/context-menu/menu-model.test.ts \
  src/lib/context-menu/icons.ts src/lib/context-menu/rich-actions.ts \
  src/lib/context-menu/source-actions.ts \
  src/lib/i18n/en.ts src/lib/i18n/zh.ts src/lib/i18n/ja.ts src/lib/i18n/de.ts
git commit -m "feat(host): 编辑器右键「溯源」——预填选区+源文档路径,派发到 idea-spark"
```

---

### Task 5: idea-spark 指令系统 `directives.ts`

**Files:**
- Create: `plugins-src/idea-spark/src/lib/directives.ts`
- Create: `plugins-src/idea-spark/src/lib/directives.test.ts`

**Interfaces:**
- Consumes: `vaultList(path) → {entries:[{name,is_dir}]}`、`vaultRead(path) → {content}`(`plugins-src/idea-spark/src/lib/bridge.ts:63-65,:48-50`),经注入的 io 端口保持可测。
- Produces:
  - `parseDirectiveInput(text): { name: string; rest: string } | null`
  - `interface DirectiveEntry { taskId: string; names: string[]; display: string; description: string }`
  - `discoverDirectives(io: DirectiveIo): Promise<DirectiveEntry[]>`
  - `matchDirective(entries: DirectiveEntry[], name: string): DirectiveEntry | null`

- [ ] **Step 1: 失败测试**(directives.test.ts)

```ts
import { describe, expect, it } from 'vitest'
import { parseDirectiveInput, discoverDirectives, matchDirective, type DirectiveIo } from './directives'

describe('parseDirectiveInput', () => {
  it('取首 token 为指令名,rest 保留其余全部(含换行引用块)', () => {
    const r = parseDirectiveInput('/溯源 只查论文\n\n> 引文\n\n源文档: a.md\n')!
    expect(r.name).toBe('溯源')
    expect(r.rest).toBe('只查论文\n\n> 引文\n\n源文档: a.md')
  })
  it('非 / 开头、纯 "/"、空文本都返回 null', () => {
    expect(parseDirectiveInput('普通 idea')).toBeNull()
    expect(parseDirectiveInput('/')).toBeNull()
    expect(parseDirectiveInput('  ')).toBeNull()
  })
  it('容忍名后直接换行', () => {
    expect(parseDirectiveInput('/trace\n> q')!.name).toBe('trace')
  })
})

function io(tasks: Record<string, string | null>): DirectiveIo {
  return {
    list: async (p) => {
      if (p !== '.notemd/agent-tasks') throw new Error('io: unexpected ' + p)
      return { entries: Object.keys(tasks).map((name) => ({ name, is_dir: true })) }
    },
    read: async (p) => {
      const id = p.split('/')[2]
      const body = tasks[id]
      if (body == null) throw new Error('io: missing')
      return { content: body }
    },
  }
}

describe('discoverDirectives', () => {
  it('只收 directive 非空的模板,坏 json/缺文件跳过', async () => {
    const got = await discoverDirectives(io({
      'trace-source': '{"name":"溯源","description":"找出处","directive":["溯源","trace"]}',
      'idea-proof': '{"name":"Idea proof","prompt":"p"}',
      broken: '{not json',
      missing: null,
    }))
    expect(got).toEqual([{ taskId: 'trace-source', names: ['溯源', 'trace'], display: '溯源', description: '找出处' }])
  })
  it('agent-tasks 目录不存在 → 空表', async () => {
    const bad: DirectiveIo = { list: async () => { throw new Error('io: no dir') }, read: async () => ({ content: '' }) }
    expect(await discoverDirectives(bad)).toEqual([])
  })
})

describe('matchDirective', () => {
  const entries = [{ taskId: 't', names: ['溯源', 'trace'], display: '溯源', description: '' }]
  it('任一名字精确命中,未知返回 null', () => {
    expect(matchDirective(entries, 'trace')?.taskId).toBe('t')
    expect(matchDirective(entries, '溯源')?.taskId).toBe('t')
    expect(matchDirective(entries, 'xx')).toBeNull()
  })
})
```

Run: `pnpm vitest run plugins-src/idea-spark/src/lib/directives.test.ts` → FAIL

- [ ] **Step 2: 实现 directives.ts**

```ts
// 指令 = task 模板:.notemd/agent-tasks/<id>/task.json 里 directive 非空的模板,
// 在输入面以 `/名字` 调用。发现走 vault RPC,全容错(参考 state-io.ts 的 parseState
// 与 task.rs「task.json 坏了就跳过」的先例)——一个坏模板不拉黑整张指令表。
const TASKS_DIR = '.notemd/agent-tasks'

export interface DirectiveEntry {
  taskId: string
  names: string[]
  display: string
  description: string
}

export interface DirectiveIo {
  list(path: string): Promise<{ entries: Array<{ name: string; is_dir: boolean }> }>
  read(path: string): Promise<{ content: string }>
}

/** `/溯源 只查论文\n> 引文` → { name:'溯源', rest:'只查论文\n> 引文' };非指令输入 → null */
export function parseDirectiveInput(text: string): { name: string; rest: string } | null {
  const t = text.trimStart()
  if (!t.startsWith('/')) return null
  const m = /^\/(\S+)([\s\S]*)$/.exec(t)
  if (!m) return null
  return { name: m[1], rest: m[2].trim() }
}

export async function discoverDirectives(io: DirectiveIo): Promise<DirectiveEntry[]> {
  let names: string[]
  try {
    const { entries } = await io.list(TASKS_DIR)
    names = entries.filter((e) => e.is_dir).map((e) => e.name)
  } catch {
    return []
  }
  const out: DirectiveEntry[] = []
  for (const id of names) {
    try {
      const { content } = await io.read(`${TASKS_DIR}/${id}/task.json`)
      const t = JSON.parse(content) as { directive?: unknown; description?: unknown }
      const directive = Array.isArray(t.directive)
        ? t.directive.filter((n): n is string => typeof n === 'string' && n !== '')
        : []
      if (directive.length === 0) continue
      out.push({
        taskId: id,
        names: directive,
        display: directive[0],
        description: typeof t.description === 'string' ? t.description : '',
      })
    } catch {
      /* 坏模板跳过 */
    }
  }
  return out
}

export function matchDirective(entries: DirectiveEntry[], name: string): DirectiveEntry | null {
  return entries.find((e) => e.names.includes(name)) ?? null
}
```

Run: `pnpm vitest run plugins-src/idea-spark/src/lib/directives.test.ts` → PASS

- [ ] **Step 3: Commit**

```bash
git add plugins-src/idea-spark/src/lib/directives.ts plugins-src/idea-spark/src/lib/directives.test.ts
git commit -m "feat(idea-spark): 指令解析与发现——directive 非空的 task 模板即 /指令"
```

---

### Task 6: `trace-source` task 模板(播种文件)

**Files:**
- Create: `plugins-src/idea-spark/src/lib/trace-template.ts`
- Create: `plugins-src/idea-spark/src/lib/trace-template.test.ts`
- Modify: `plugins-src/idea-spark/src/lib/task-template.ts:165-170`(`seedTaskTemplate` 泛化为接受文件表)

**Interfaces:**
- Consumes: `SeedIo`(`task-template.ts:152-155`)。
- Produces: `TRACE_TASK_ID = 'trace-source'`、`TRACE_TASK_FILES: Record<string, string>`;`seedTaskTemplate(io, files?)` 第二参缺省仍是 idea-proof 的 `TASK_FILES`(既有调用不变)。

- [ ] **Step 1: 失败测试**(trace-template.test.ts,照 task-template.test.ts 姿势)

```ts
import { describe, expect, it } from 'vitest'
import { TRACE_TASK_FILES, TRACE_TASK_ID } from './trace-template'

const at = (suffix: string) => {
  const hit = Object.entries(TRACE_TASK_FILES).find(([p]) => p.endsWith(suffix))
  expect(hit, suffix).toBeTruthy()
  return hit![1]
}

describe('trace-source 模板', () => {
  it('五个文件齐全且都在模板目录下', () => {
    const paths = Object.keys(TRACE_TASK_FILES)
    expect(paths).toHaveLength(5)
    for (const p of paths) expect(p.startsWith(`.notemd/agent-tasks/${TRACE_TASK_ID}/`)).toBe(true)
  })
  it('task.json 可解析,directive/okf_type/超时符合 spec', () => {
    const t = JSON.parse(at('/task.json'))
    expect(t.directive).toEqual(['溯源', 'trace'])
    expect(t.okf_type).toBe('Trace Report')
    expect(t.timeout_seconds).toBe(2700)
    expect(t.precheck).toBe('precheck.sh')
  })
  it('settings 放开 WebSearch/WebFetch/yt-dlp,写权限只有 traces/', () => {
    for (const f of ['/.claude/settings.json', '/.claude/settings.scoped.json']) {
      const s = JSON.parse(at(f))
      expect(s.permissions.allow).toContain('WebSearch')
      expect(s.permissions.allow).toContain('Bash(yt-dlp:*)')
      const writes = s.permissions.allow.filter((a: string) => a.startsWith('Write(') || a.startsWith('Edit('))
      for (const w of writes) expect(w).toContain('/traces/')
      expect(s.permissions.deny).toContain('Task')
    }
  })
  it('CLAUDE.md 含协议要件:输出行、材料目录、降级、缘起、红线', () => {
    const md = at('/CLAUDE.md')
    for (const must of ['输出:', 'traces/', 'yt-dlp', '未取到字幕', '缘起', 'Trace Material', '绝不修改'])
      expect(md, must).toContain(must)
    expect(md).toContain('${VAULT}') // 数组拼行没被 JS 求值
  })
})
```

Run: `pnpm vitest run plugins-src/idea-spark/src/lib/trace-template.test.ts` → FAIL

- [ ] **Step 2: 实现 trace-template.ts**(全部数组拼行;文件主体如下,逐字进模板)

```ts
// trace-source 任务模板:溯源。播种约定与坑同 task-template.ts(见其文件头注释:
// 数组拼行防 ``` 与 ${VAULT};host.vault.write 无 chmod)。
export const TRACE_TASK_ID = 'trace-source'

const BASE = `.notemd/agent-tasks/${TRACE_TASK_ID}`

const TASK_JSON = [
  '{',
  '  "name": "溯源",',
  '  "description": "为一段话找到原始出处(YouTube/论文/博客),下载字幕或正文,生成带反向链接的摘要。",',
  '  "prompt": "按 CLAUDE.md 的协议为委托文本中的引文溯源。产物只写进 vault 的 traces/ 目录。",',
  '  "max_turns": 100,',
  '  "timeout_seconds": 2700,',
  '  "precheck": "precheck.sh",',
  '  "okf_type": "Trace Report",',
  '  "directive": ["溯源", "trace"]',
  '}',
  '',
].join('\n')

const PRECHECK_SH = [
  '#!/bin/sh',
  '# traces/ 必须可写,否则这次运行注定白跑。yt-dlp 缺失不拦——协议内降级。',
  '[ -n "$NOTEMD_VAULT" ] || { echo "缺少 vault 参数"; exit 1; }',
  'mkdir -p "$NOTEMD_VAULT/traces" 2>/dev/null || { echo "traces/ 不可写"; exit 1; }',
  'exit 0',
  '',
].join('\n')

const SETTINGS_ALLOW = [
  '      "Read(${VAULT}/**)",',
  '      "Write(${VAULT}/traces/**)",',
  '      "Edit(${VAULT}/traces/**)",',
  '      "WebSearch",',
  '      "WebFetch",',
  '      "Bash(yt-dlp:*)"',
]

const SETTINGS_JSON = [
  '{',
  '  "permissions": {',
  '    "allow": [',
  ...SETTINGS_ALLOW,
  '    ],',
  '    "deny": [ "Task" ]',
  '  }',
  '}',
  '',
].join('\n')

const SETTINGS_SCOPED_JSON = [
  '{',
  '  "permissions": {',
  '    "allow": [',
  '      "Read(${NOTE})",',
  ...SETTINGS_ALLOW,
  '    ],',
  '    "deny": [ "Task" ]',
  '  }',
  '}',
  '',
].join('\n')

const CLAUDE_MD = [
  '# 任务:溯源——为一段话找到原始出处',
  '',
  '你在 note.md 的 agent 插件里以 headless 模式运行,vault 根在 `${VAULT}`。',
  '委托文本的结构:',
  '',
  '- `> ` 引用块 = 待溯源的原文(用户从某篇文档里选出来的一段话)。',
  '- `源文档: <路径>` 行 = 这段话所在的文档,摘要要反向链接回它。可能缺失。',
  '- `输出: <路径>` 行 = 摘要文件的落点(vault 相对路径),**不得改名**。',
  '- 其余文字 = 用户的范围与关注点说明(如「只查 YouTube 和 arxiv」「关注工程实现」)。',
  '  未指定范围时,YouTube、论文库(arxiv/Semantic Scholar 等)、欧美技术博客三类都试。',
  '',
  '## 流程',
  '',
  '1. 从引文中提取可检索的断言、术语、人名、数字,构造多组检索词。',
  '2. 用 WebSearch 按用户指定范围检索;候选出处逐个核验,确认引文与来源内容',
  '   真实对应,不要只凭标题相似就认定。',
  '3. 取全文材料:',
  '   - 博客/新闻/论文页:WebFetch 拉正文;论文优先 arxiv abs 页。',
  '   - YouTube:先探测 `yt-dlp --version`;可用则用',
  '     `yt-dlp --skip-download --write-subs --write-auto-subs --sub-langs "en.*,zh.*" -o "<临时名>" <url>`',
  '     取字幕并转成通顺的纯文本转写;**yt-dlp 不可用就降级**:给出视频链接与基于',
  '     搜索结果的内容描述,并在摘要里如实声明「未取到字幕」。',
  '4. 每份取到的全文材料写一个文件:`traces/<摘要同名去 .md>/<nn>-<来源短名>.md`',
  '   (例:输出是 `traces/2026-08-18-143012.md`,材料就在 `traces/2026-08-18-143012/01-karpathy-blog.md`)。',
  '   材料 frontmatter 必写:',
  '',
  '   ```',
  '   ---',
  '   type: Trace Material',
  '   title: <来源标题>',
  '   sources:',
  '     - resource: <原始 URL>',
  '       title: <来源标题>',
  '       author: <作者/频道,可省>',
  '   ---',
  '   ```',
  '',
  '5. 摘要写到 `输出:` 指定的路径,结构:',
  '   - frontmatter:`type: Trace Report`、`title`(一行主题)、`generated:',
  '     { by: process:trace-source, at: <ISO 8601> }`、`sources:` 列出全部核验过的出处 URL。',
  '   - `## 缘起`:原样引用待溯源引文(引用块),下一行给出源文档链接——',
  '     `源文档:` 是 vault 内相对路径时写 `[<文件名>](</相对路径>)` 形式的 markdown 链接,',
  '     vault 外或缺失时原样写路径纯文本。',
  '   - `## 结论`:最可能的原始出处,标可信度(确认/高度疑似/未找到);每条断言旁',
  '     直接标 URL。',
  '   - `## 摘要`:按用户关注点组织的内容提炼——不是来源的复述,而是回答「这段话',
  '     的原始语境是什么、作者真正的主张是什么、与引文的出入在哪」。',
  '   - `## 继续阅读`:逐条列出材料全文的**相对链接**,如',
  '     `[Karpathy 博客正文](2026-08-18-143012/01-karpathy-blog.md)`。',
  '6. 找不到出处也要产出摘要:声明未找到,列出已排查的候选与排除理由。',
  '',
  '## 红线',
  '',
  '- 只写 `traces/` 下的文件,绝不修改 vault 里的其他任何文件,绝不动源文档。',
  '- frontmatter 署名用 `process:trace-source`,绝不用 `human:` 前缀。',
  '- 输出语言跟随委托文本语言;来源引用保留原文。',
  '- 用了外部来源就把 URL 标在它支撑的那句话旁边。',
  '',
].join('\n')

/** vault 相对路径 → 文件内容。 */
export const TRACE_TASK_FILES: Record<string, string> = {
  [`${BASE}/task.json`]: TASK_JSON,
  [`${BASE}/CLAUDE.md`]: CLAUDE_MD,
  [`${BASE}/precheck.sh`]: PRECHECK_SH,
  [`${BASE}/.claude/settings.json`]: SETTINGS_JSON,
  [`${BASE}/.claude/settings.scoped.json`]: SETTINGS_SCOPED_JSON,
}
```

- [ ] **Step 3: 泛化 `seedTaskTemplate`**(task-template.ts)

```ts
export async function seedTaskTemplate(
  io: SeedIo,
  files: Record<string, string> = TASK_FILES,
): Promise<void> {
  for (const [path, content] of Object.entries(files)) {
    if (await io.exists(path)) continue
    await io.write(path, content)
  }
}
```

Run: `pnpm vitest run plugins-src/idea-spark/src/lib/trace-template.test.ts plugins-src/idea-spark/src/lib/task-template.test.ts` → PASS

- [ ] **Step 4: Commit**

```bash
git add plugins-src/idea-spark/src/lib/trace-template.ts plugins-src/idea-spark/src/lib/trace-template.test.ts \
  plugins-src/idea-spark/src/lib/task-template.ts
git commit -m "feat(idea-spark): trace-source 任务模板——溯源协议、traces/ 权限、yt-dlp 降级"
```

---

### Task 7: idea-spark `delegateDirective`

**Files:**
- Modify: `plugins-src/idea-spark/src/lib/agent-client.ts`
- Modify: `plugins-src/idea-spark/src/lib/agent-client.test.ts`
- Modify: `plugins-src/idea-spark/src/lib/strings.ts`(`MESSAGE_KEYS` + 4 catalog)

**Interfaces:**
- Consumes: Task 5 的 `DirectiveEntry`;Task 6 的 `TRACE_TASK_FILES`;`agentRun`(bridge)。
- Produces:
  - `directiveOutputRel(now: Date): string` → `traces/YYYY-MM-DD-HHmmss.md`
  - `delegateDirective(entry: DirectiveEntry, rest: string, vaultRoot: string): Promise<DelegateResult & { outRel?: string }>`
- 新 strings 键:`notifyDirectiveOk`(zh:『溯源完成』风格的通用文案『委托完成』)、`notifyDirectiveFail`(『委托失败』),4 语言。

- [ ] **Step 1: 失败测试**(agent-client.test.ts 里追加,照既有 stub `window.notemd` 的姿势)

```ts
describe('delegateDirective', () => {
  it('task 取 entry.taskId,prompt=rest+输出行,notify 指向摘要绝对路径', async () => {
    const calls: any[] = []
    stubBridge({ 'host.agent.run': (p: any) => { calls.push(p); return { run_id: 'r1' } } })
    const entry = { taskId: 'trace-source', names: ['溯源'], display: '溯源', description: '' }
    const r = await delegateDirective(entry, '只查论文\n\n> 引文', '/V')
    expect(r.ok).toBe(true)
    const p = calls[0]
    expect(p.task).toBe('trace-source')
    expect(p.prompt).toMatch(/^只查论文\n\n> 引文\n\n输出: traces\/\d{4}-\d{2}-\d{2}-\d{6}\.md\n$/)
    expect(p.note_path).toBeUndefined()
    expect(p.notify.open_path).toBe(`/V/${r.outRel}`)
    expect(p.notify.expect_file).toBe(`/V/${r.outRel}`)
    expect(p.notify.title_ok).toContain('/溯源')
  })
  it('agent_unavailable 前缀映射 agent-missing', async () => {
    stubBridge({ 'host.agent.run': () => { throw new Error('agent_unavailable: not installed') } })
    const entry = { taskId: 't', names: ['x'], display: 'x', description: '' }
    const r = await delegateDirective(entry, 'y', '/V')
    expect(r).toMatchObject({ ok: false, reason: 'agent-missing' })
  })
})
```

(`stubBridge` 若测试文件里没有现成辅助,按该文件既有 mock 方式改写——断言内容保持不变。)

Run: `pnpm vitest run plugins-src/idea-spark/src/lib/agent-client.test.ts` → FAIL

- [ ] **Step 2: strings 加键**(`MESSAGE_KEYS` 加 `'notifyDirectiveOk', 'notifyDirectiveFail'`;catalogs:en `Delegation done`/`Delegation failed`,zh `委托完成`/`委托失败`,ja `委任完了`/`委任失敗`,de `Auftrag erledigt`/`Auftrag fehlgeschlagen`)

Run: `pnpm vitest run plugins-src/idea-spark/src/lib/strings.test.ts` → PASS

- [ ] **Step 3: 实现**(agent-client.ts 追加;seed 同时播 trace 模板)

```ts
import { TRACE_TASK_FILES } from './trace-template'

/** 指令产物统一落 traces/,时间戳定名——调用方因此能预知 expect_file(spec §3)。 */
export function directiveOutputRel(now: Date): string {
  const p = (n: number, w = 2) => String(n).padStart(w, '0')
  const d = `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}`
  const t = `${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`
  return `traces/${d}-${t}.md`
}

/**
 * 委托一次 /指令 运行。与 delegateIdea 的差别:task 来自指令表而非写死,
 * 没有 note_path(指令文本自足),输出路径由这里定死并追加成 `输出:` 行。
 */
export async function delegateDirective(
  entry: { taskId: string; display: string },
  rest: string,
  vaultRoot: string,
): Promise<DelegateResult & { outRel?: string }> {
  try {
    await seedTaskTemplate(seedIo, TRACE_TASK_FILES)
  } catch (e) {
    console.warn('[idea-spark] seeding trace-source failed:', e)
  }
  const outRel = directiveOutputRel(new Date())
  const outAbs = absolute(vaultRoot, outRel)
  try {
    const { run_id } = await agentRun({
      task: entry.taskId,
      prompt: `${rest}\n\n输出: ${outRel}\n`,
      notify: {
        title_ok: `${t('notifyDirectiveOk')} · /${entry.display}`,
        title_fail: `${t('notifyDirectiveFail')} · /${entry.display}`,
        open_path: outAbs,
        expect_file: outAbs,
      },
    })
    if (typeof run_id !== 'string' || run_id === '') {
      return { ok: false, reason: 'error', message: 'the agent started a run without a run id' }
    }
    return { ok: true, runId: run_id, outRel }
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    return {
      ok: false,
      reason: message.includes('agent_unavailable') ? 'agent-missing' : 'error',
      message,
    }
  }
}
```

(`AgentRunParams.note_path` 若为必填类型则改成可选——bridge.ts:103-112 的类型按 `plugin.rs:636-644` 的实际 schema 本来就是选填。)

Run: `pnpm vitest run plugins-src/idea-spark/src/lib/agent-client.test.ts` → PASS

- [ ] **Step 4: Commit**

```bash
git add plugins-src/idea-spark/src/lib/agent-client.ts plugins-src/idea-spark/src/lib/agent-client.test.ts \
  plugins-src/idea-spark/src/lib/strings.ts plugins-src/idea-spark/src/lib/bridge.ts
git commit -m "feat(idea-spark): delegateDirective——指令委托,输出定名 traces/,通知直开摘要"
```

---

### Task 8: idea-spark App 接线(seed 预填 + 指令 chip + 委托路由)

**Files:**
- Modify: `plugins-src/idea-spark/src/App.svelte`(:483-534 onMount、:492-501 onMessage、:397-456 delegate、:611 editor 区上方)
- Modify: `plugins-src/idea-spark/src/lib/strings.ts`(chip 无新键;若 delegate 错误提示需要区分指令,复用既有错误文案)
- Modify: `plugins-src/idea-spark/manifest.v2.json`(版本号 patch +1)

**Interfaces:**
- Consumes: Task 3 双通道(`onMessage` 的 `{type:'seed',payload:{text}}` + `location.search` 的 `?seed=<json>`)、Task 5 全部导出、Task 7 `delegateDirective`、既有 `showMarkdown`(App.svelte:131-134)与 `startNew`(tray-activate 分支所调)。

- [ ] **Step 1: seed 双入口**

onMessage(:492-501)加分支(在 `tray-activate` 之前):

```ts
    if (type === 'seed') {
      const text = (payload as { payload?: { text?: string } }).payload?.text
      if (typeof text === 'string' && text) void applySeed(text)
      return
    }
```

onMount 尾部(kit 挂载之后)加:

```ts
  // 右键「溯源」新开窗口时,预填经 URL query 进来(已开窗口走 host push)。
  try {
    const raw = new URLSearchParams(location.search).get('seed')
    if (raw) {
      const seed = JSON.parse(raw) as { text?: string }
      if (typeof seed.text === 'string' && seed.text) await applySeed(seed.text)
    }
  } catch (e) {
    console.warn('[idea-spark] bad seed query:', e)
  }
```

`applySeed` 新函数(放 delegate 附近):新建一条 idea 再填入文本——复用 `startNew()` 的现有路径,完成后 `showMarkdown(text)` 并同步 `fallbackText = text`(kit 挂载失败的降级编辑器也要吃到预填)。

- [ ] **Step 2: 指令表加载 + chip**

- `let directives: DirectiveEntry[] = $state([])`;onMount 里(boot 之后):先 `seedTaskTemplate(seedIo, TRACE_TASK_FILES)` 尽力播种(fresh vault 也能发现 `/溯源`),再 `directives = await discoverDirectives({ list: vaultList, read: vaultRead })`,整体 try/catch → `[]`。
- 当前文本的指令匹配派生值:编辑器 `onChange` 已把文本喂给现有状态(查 App.svelte 的 onChange 回调,拿同一份文本);`matched = parseDirectiveInput(text)` 且 `matchDirective(directives, name)` 非空时,在 action bar(:665-675)上方渲染一行 chip:

```svelte
{#if matchedDirective}
  <div class="directive-chip">/{matchedDirective.display} · {matchedDirective.description}</div>
{/if}
```

样式与现有面板一致(小号、muted 前景色);无匹配不渲染、不报错。

- [ ] **Step 3: delegate 路由**

`delegate()`(:397-456)开头:取当前编辑器全文 → `parseDirectiveInput`;命中 `matchDirective` 时改走:

```ts
    const parsed = parseDirectiveInput(currentText)
    const entry = parsed && matchDirective(directives, parsed.name)
    if (parsed && entry) {
      const r = await delegateDirective(entry, parsed.rest, vaultRoot)
      // 结果提示复用既有 delegateIdea 的成功/agent-missing/error 三分支 UI 文案
      return
    }
```

未命中(含 `/xxx` 打错):不报错,落回既有 idea-proof 流程。指令运行不进 `pendingRuns`(v1:完成靠统一通知,不做收件箱徽标)。

- [ ] **Step 4: 检查 + 手测清单**

Run: `pnpm check && pnpm vitest run plugins-src/idea-spark/` → 全绿
GUI 手测(用户执行,dev 构建):
1. 编辑器选中一段话 → 右键「溯源」→ idea-spark 弹出且预填 `/溯源 + 引用块 + 源文档:`(chip 显示)。
2. idea-spark 已开时再右键溯源 → 同窗口刷新预填(push 通道)。
3. 手打 `/溯源 测试` → chip 出现;`/不存在 x` → 无 chip,委托走 idea-proof。
4. 点委托 → Agent 区出现 trace-source 运行;完成后通知点开 `traces/` 摘要。

- [ ] **Step 5: Commit**

```bash
git add plugins-src/idea-spark/src/App.svelte plugins-src/idea-spark/src/lib/strings.ts plugins-src/idea-spark/manifest.v2.json
git commit -m "feat(idea-spark): seed 预填双通道 + 指令 chip + /指令 委托路由"
```

---

### Task 9: CHANGELOG(双语)+ 全量验证

**Files:**
- Modify: `CHANGELOG.md`、`CHANGELOG.zh-CN.md`(各自「未发布」区;注意主 worktree 里可能有并行会话的未提交条目,只追加不动别人的)

**Interfaces:**
- Consumes: 全部前置任务。

- [ ] **Step 1: 条目(面向用户)**

zh(en 对应翻译,序列一致):

```markdown
- 新增「溯源」:选中一段话,右键「溯源」或在奇思妙想里输入 `/溯源`,委托 agent 到 YouTube/论文库/博客找到原始出处,下载字幕与正文,生成带反向链接的摘要到 `traces/`,完成后通知直达。
- 奇思妙想支持 `/指令`:任何声明了 `directive` 的 agent 任务模板都会成为输入面指令——自己写模板就能造新指令。
```

- [ ] **Step 2: 全量验证**

Run(依次,全部必须绿):

```bash
pnpm check
pnpm test
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path plugins-src/agent-run-core/Cargo.toml
cargo test --manifest-path searchidx/Cargo.toml
node scripts/changelog.mjs check
```

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md CHANGELOG.zh-CN.md
git commit -m "docs(changelog): 溯源功能与 /指令 抽象"
```

---

### Task 10: 真机端到端探针(发版门槛,不在 CI)

**步骤(实现完、用户 GUI 验证通过后、发版前执行):**

- [ ] dev 构建起一个真 vault,右键溯源一段**已知出处**的引文(如 "Attention is all you need" 摘要句),范围写「只查 arxiv」。
- [ ] 验证:`traces/<ts>.md` 生成、frontmatter `type: Trace Report` + `generated.by: process:trace-source`、「缘起」链接指回源文档、「继续阅读」相对链接可点开材料文件、材料 frontmatter `type: Trace Material` + 原始 URL。
- [ ] `pnpm okf:lint <vault>/traces` 通过。
- [ ] 装有 yt-dlp 的机器上再跑一条 YouTube 引文验证字幕路径;卸载/无 yt-dlp 时验证降级声明出现。
- [ ] 通过后:宿主随下次 release 发版;idea-spark 走插件市场发布(`release-plugins.sh`,注意 gen-plugin-index 的 merge 语义与 wrangler `--remote`)。
