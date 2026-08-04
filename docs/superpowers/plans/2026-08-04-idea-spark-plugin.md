# 奇思妙想(Idea Spark)插件实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `notemd.idea-spark` 插件(托盘/插件菜单入口 + rich/source 编辑窗 + 委托 claude-agent 论证 idea),以及它依赖的三件宿主基建:`host.vault.read_bytes`、`host.plugin.execute`、`host.agent.watch`+守望器、Editor Kit(`__host__` 运行时下发)。

**Architecture:** 纯前端插件(照 decision-log 形态)+ 宿主桥扩展。编辑器由宿主以 Editor Kit 组件包运行时下发(主前端同一次 vite 构建的第二个 entry,共享 moraya chunk,安装包净增≈0)。委托后宿主前端守望器轮询 claude-agent `run-status`,终态推回插件窗口并调用通知挂钩(挂钩本期无注册者——系统通知由用户另做的统一托盘通知承接,见 Task 4)。任务模板 `idea-proof` 由插件幂等种入 vault,claude-agent 自动发现。

**Tech Stack:** Svelte 5 + Tauri 2(Rust)、@moraya/core、vitest、cargo test。

**Spec:** `docs/superpowers/specs/2026-08-04-idea-spark-plugin-design.md`(全部需求以此为准)。

## Global Constraints

- 插件 id `notemd.idea-spark`,英文名 `Idea Spark`,i18n zh「奇思妙想」/ ja「アイデアスパーク」/ de「Ideenfunke」。
- idea 目录默认 `inbox/ideas`(vault 相对),配置存 vault 内 `.notemd/idea-spark.json`。
- 插件 manifest `engines.notemd: ">=6.804.2"`(桥扩展随宿主下一版发布;版本号由 release.sh 按日期规则自动推导,只需 ≥ 该值)。
- 新桥方法与 capability:`host.vault.read_bytes`→`vault.read`;`host.plugin.execute`→`plugin.execute:<目标id>`(带参);`host.agent.watch`→`agent.watch`;`host.theme.css`→`editor.kit`;`__host__/` 资产→`editor.kit`。
- 写 `.md` 必须经 `src/lib/okf/concept.ts` 模式(插件用**复制**的 concept.ts);新 `type` 先在 CONCEPT_TYPE 登记:`idea: 'Idea'`、`ideaProof: 'Idea Proof'`。文件名避开 `index.md`/`log.md`。
- 插件 UI 是隔离 webview:**绝不 import 主程序 `src/`**,一切能力走 `window.notemd` 桥。Editor Kit 是宿主代码,可以 import `src/`,但其依赖图**不得触碰任何 Tauri IPC 模块**(`@tauri-apps/api`、adapters、tabs、insights)。
- 主 worktree 常被共享:每次 commit 只精确 add 本任务列出的文件,绝不 `git add -A`。
- Rust 测试:`cargo test --manifest-path src-tauri/Cargo.toml plugin_runtime`;主前端:`pnpm check && pnpm test`;插件:`pnpm --filter idea-spark test`。
- GUI/窗口改动不做自动化验证:最后给手动验证清单,由用户实机验证。

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

### Task 2: 宿主桥 `host.plugin.execute`(带参 capability 跨插件调用)

**Files:**
- Modify: `src-tauri/src/plugin_runtime/ui_rpc.rs`(dispatch() 拦截 + 纯函数门 + 测试)

**Interfaces:**
- Produces: 桥方法 `host.plugin.execute` `{plugin_id, command, context} → <目标插件返回值>`。仅 UI 桥。调用方 manifest 必须声明 capability 字符串 `plugin.execute:<plugin_id>`(精确匹配目标 id)。
- Produces(内部): `pub(crate) fn plugin_execute_gate(capabilities: &[String], params: &serde_json::Value) -> Result<(String, String, serde_json::Value), (i64, String)>` — 返回 `(target_id, command, context)` 或 `(错误码, 消息)`。

- [ ] **Step 1: 写失败测试**(纯函数门,不需要 AppHandle)

```rust
#[test]
fn plugin_execute_gate_enforces_parameterized_capability() {
    let p = serde_json::json!({"plugin_id": "notemd.claude-agent", "command": "run-note", "context": {"a": 1}});
    // 未持有 → -32001
    let e = plugin_execute_gate(&[], &p).unwrap_err();
    assert_eq!(e.0, proto::ERR_CAPABILITY_DENIED);
    assert!(e.1.contains("plugin.execute:notemd.claude-agent"));
    // 持有其他目标的授权也不行
    let caps = vec!["plugin.execute:notemd.other".to_string()];
    assert!(plugin_execute_gate(&caps, &p).is_err());
    // 精确持有 → 放行并解出三元组
    let caps = vec!["plugin.execute:notemd.claude-agent".to_string()];
    let (id, cmd, ctx) = plugin_execute_gate(&caps, &p).unwrap();
    assert_eq!(id, "notemd.claude-agent");
    assert_eq!(cmd, "run-note");
    assert_eq!(ctx["a"], 1);
    // 缺字段 → ERR_INVALID(用 proto::ERR_INTERNAL 若无 INVALID 常量,消息含字段名)
    let bad = serde_json::json!({"command": "x"});
    assert!(plugin_execute_gate(&caps, &bad).is_err());
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml plugin_runtime::ui_rpc`
Expected: FAIL(函数不存在)

- [ ] **Step 3: 实现门函数 + dispatch() 拦截**

```rust
// ui_rpc.rs(dispatch 上方):
/// `host.plugin.execute` 的参数与授权门(纯函数,单测友好)。
/// capability 形如 `plugin.execute:<目标插件id>`,精确匹配。
pub(crate) fn plugin_execute_gate(
    capabilities: &[String],
    params: &serde_json::Value,
) -> Result<(String, String, serde_json::Value), (i64, String)> {
    let target = params.get("plugin_id").and_then(|v| v.as_str())
        .ok_or((proto::ERR_INTERNAL, "host.plugin.execute needs 'plugin_id'".to_string()))?;
    let command = params.get("command").and_then(|v| v.as_str())
        .ok_or((proto::ERR_INTERNAL, "host.plugin.execute needs 'command'".to_string()))?;
    let needed = format!("plugin.execute:{target}");
    if !capabilities.iter().any(|c| c == &needed) {
        return Err((proto::ERR_CAPABILITY_DENIED,
            format!("method host.plugin.execute requires capability '{needed}'")));
    }
    let context = params.get("context").cloned().unwrap_or(serde_json::Value::Null);
    Ok((target.to_string(), command.to_string(), context))
}

// dispatch()(:209)在 `if !is_host_method` 之后、host 路径之前拦截
// (需要 AppHandle,所以不进 dispatch_with;进程通道自然维持 -32601):
if req.method == "host.plugin.execute" {
    let id = req.id;
    return match plugin_execute_gate(capabilities, &req.params) {
        Err((code, msg)) => err(id, code, msg),
        Ok((target, command, context)) => {
            let out: Result<serde_json::Value, String> = async {
                let lc = super::commands::get_or_register(app, &target)?;
                lc.ensure_active(&super::lifecycle::Trigger::Command(command.clone())).await?;
                lc.execute(plugin_protocol::ExecuteCommandParams { command, context }).await
            }.await;
            match out { Ok(v) => ok(id, v), Err(d) => err(id, proto::ERR_INTERNAL, d) }
        }
    };
}
```

注意:目标是 UI-only 插件(无 binary)时 `get_or_register` 会失败——错误原样透传即可(claude-agent 有 binary,不受影响)。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml plugin_runtime`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin_runtime/ui_rpc.rs
git commit -m "feat(plugin-bridge): host.plugin.execute——带参 capability 的跨插件命令调用"
```

---

### Task 3: 宿主桥 `host.agent.watch` + `plugin_v2_window_push` 命令

**Files:**
- Modify: `src-tauri/src/plugin_runtime/ui_rpc.rs`(watch 参数校验纯函数 + dispatch() 拦截 emit)
- Modify: `src-tauri/src/plugin_runtime/commands.rs`(新命令)
- Modify: `src-tauri/src/lib.rs`(generate_handler 登记,搜 `plugin_v2_execute` 所在列表)

**Interfaces:**
- Produces: 桥方法 `host.agent.watch`(capability `agent.watch`,仅 UI 桥):params `{executor, task, run_id, notify: {title, body, open_path?}}`;效果 = 向主窗口 emit Tauri 事件 `agent-watch:add`,payload 追加 `requester: <调用方插件id>`;返回 `{ok:true}`。
- Produces: `#[tauri::command] plugin_v2_window_push(app, plugin_id: String, window_id: String, payload: serde_json::Value)` → `windows::push_to_window`(主前端专用,给守望器回推插件窗口)。
- Produces(内部): `pub(crate) fn agent_watch_payload(requester: &str, params: &serde_json::Value) -> Result<serde_json::Value, String>`。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn agent_watch_payload_validates_and_stamps_requester() {
    let p = serde_json::json!({
        "executor": "notemd.claude-agent", "task": "idea-proof", "run_id": "r1",
        "notify": {"title": "t", "body": "b", "open_path": "inbox/ideas/x.proof.md"}
    });
    let v = agent_watch_payload("notemd.idea-spark", &p).unwrap();
    assert_eq!(v["requester"], "notemd.idea-spark");
    assert_eq!(v["executor"], "notemd.claude-agent");
    assert_eq!(v["run_id"], "r1");
    // 缺 run_id → Err
    assert!(agent_watch_payload("x.y", &serde_json::json!({"executor": "a.b", "task": "t"})).is_err());
}
```

同时在 `method_capability_table` 测试加:`assert_eq!(method_capability("host.agent.watch"), Some("agent.watch"));`

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml plugin_runtime::ui_rpc`
Expected: FAIL

- [ ] **Step 3: 实现**

```rust
// host_api.rs method_capability 加:
"host.agent.watch" => Some("agent.watch"),

// ui_rpc.rs:
pub(crate) fn agent_watch_payload(requester: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    for key in ["executor", "task", "run_id"] {
        if params.get(key).and_then(|v| v.as_str()).map(str::is_empty).unwrap_or(true) {
            return Err(format!("host.agent.watch needs '{key}'"));
        }
    }
    let mut v = params.clone();
    v["requester"] = serde_json::Value::String(requester.to_string());
    Ok(v)
}

// dispatch() 拦截(host.plugin.execute 拦截之后;capability 门手动做,因为
// 这里在 dispatch_with 的统一门之前):
if req.method == "host.agent.watch" {
    let id = req.id;
    if !capabilities.iter().any(|c| c == "agent.watch") {
        return err(id, proto::ERR_CAPABILITY_DENIED,
            "method host.agent.watch requires capability 'agent.watch'".into());
    }
    return match agent_watch_payload(plugin_id, &req.params) {
        Ok(payload) => {
            use tauri::Emitter;
            let _ = app.emit("agent-watch:add", payload);
            ok(id, serde_json::json!({"ok": true}))
        }
        Err(d) => err(id, proto::ERR_INTERNAL, d),
    };
}

// commands.rs(plugin_v2_open_window 旁):
/// 主前端守望器向插件窗口回推 payload(window.__notemd_dispatch)。
#[tauri::command]
pub fn plugin_v2_window_push(
    app: tauri::AppHandle,
    plugin_id: String,
    window_id: String,
    payload: serde_json::Value,
) {
    super::windows::push_to_window(&app, &plugin_id, &window_id, &payload);
}
```

lib.rs 的 `generate_handler![...]` 列表(含 `plugin_runtime::commands::plugin_v2_execute` 的那处)加 `plugin_runtime::commands::plugin_v2_window_push`。

- [ ] **Step 4: 跑测试 + 编译确认**

Run: `cargo test --manifest-path src-tauri/Cargo.toml plugin_runtime && cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin_runtime/ui_rpc.rs src-tauri/src/plugin_runtime/host_api.rs src-tauri/src/plugin_runtime/commands.rs src-tauri/src/lib.rs
git commit -m "feat(plugin-bridge): host.agent.watch 登记事件 + plugin_v2_window_push 回推命令"
```

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

### Task 8: 宿主守望器 `src/lib/agent-watch/` + 接线

**Files:**
- Create: `src/lib/agent-watch/store.svelte.ts`
- Test: `src/lib/agent-watch/store.test.ts`
- Modify: `src/App.svelte`(listen `agent-watch:add` + onMount resume;与既有 `listen('quick-note', ...)` 同段落接线)

**Interfaces:**
- Consumes: Tauri 事件 `agent-watch:add`(Task 3 emit,payload 见下);`invoke('plugin_v2_execute', {pluginId: entry.executor, command: 'run-status', context: {run_id, task}})`(与 `agent-workspace/store.svelte.ts:119` 同形);`invoke('plugin_v2_window_push', ...)`(Task 3)。**不消费任何通知 API**(Task 4 取消)。
- Produces:

```ts
export interface WatchEntry {
  executor: string      // 'notemd.claude-agent'
  task: string          // 'idea-proof'
  run_id: string
  requester: string     // 'notemd.idea-spark'
  notify: { title: string; body: string; open_path?: string }
}
export function addWatch(entry: WatchEntry): void      // 去重(run_id)、持久化、开始轮询
export function resumeWatches(): void                  // 启动时从 localStorage 恢复
export const WATCH_STORAGE_KEY = 'agent-watch.v1'

/** 完成提醒的挂钩。本期无注册者 = 不提醒(通知由统一托盘通知另行承接,Task 4 取消)。
 *  统一托盘通知落地后只需在启动时注册一次,守望器无需改动。 */
export type WatchNotifier = (n: { title: string; body: string; openPath?: string; status: string }) => void
export function registerWatchNotifier(fn: WatchNotifier | null): void
```

- 完成时推给发起插件窗口(window id 固定 `main`)的 payload:`{type: 'agent-run-done', run_id, task, status, message, open_path}`(status 取 record.status,lost 时为 `'lost'`)。

- [ ] **Step 1: 写失败测试**(测试缝:注入 execute/push + 注册通知挂钩,不碰真 invoke;轮询用假定时器)

```ts
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { addWatch, resumeWatches, registerWatchNotifier, WATCH_STORAGE_KEY, __setTransportsForTests } from './store.svelte'

const entry = { executor: 'notemd.claude-agent', task: 'idea-proof', run_id: 'r1',
  requester: 'notemd.idea-spark', notify: { title: 'T', body: 'B', open_path: 'inbox/ideas/a.proof.md' } }

describe('agent-watch', () => {
  beforeEach(() => { localStorage.clear(); vi.useFakeTimers(); registerWatchNotifier(null) })
  it('polls until done, then pushes + calls the notifier hook + clears storage', async () => {
    const execute = vi.fn()
      .mockResolvedValueOnce({ state: 'running', steps: 1 })
      .mockResolvedValueOnce({ state: 'done', record: { status: 'success', result: 'ok', artifacts: [] } })
    const push = vi.fn(); const notifier = vi.fn()
    __setTransportsForTests({ execute, push })
    registerWatchNotifier(notifier)
    addWatch(entry)
    expect(JSON.parse(localStorage.getItem(WATCH_STORAGE_KEY)!)).toHaveLength(1)
    await vi.advanceTimersByTimeAsync(2100); await vi.advanceTimersByTimeAsync(2100)
    expect(push).toHaveBeenCalledWith('notemd.idea-spark', 'main',
      expect.objectContaining({ type: 'agent-run-done', run_id: 'r1', status: 'success' }))
    expect(notifier).toHaveBeenCalledWith(expect.objectContaining({ title: 'T', status: 'success' }))
    expect(JSON.parse(localStorage.getItem(WATCH_STORAGE_KEY)!)).toHaveLength(0)
  })
  it('works with no notifier registered (this release: nothing to notify with)', async () => {
    const execute = vi.fn().mockResolvedValue({ state: 'done', record: { status: 'success' } })
    const push = vi.fn()
    __setTransportsForTests({ execute, push })
    addWatch({ ...entry, run_id: 'r3' })
    await expect(vi.advanceTimersByTimeAsync(2100)).resolves.not.toThrow()
    expect(push).toHaveBeenCalled()
  })
  it('lost run still pushes with status lost', async () => {
    const execute = vi.fn().mockResolvedValue({ state: 'lost' })
    const push = vi.fn()
    __setTransportsForTests({ execute, push })
    addWatch({ ...entry, run_id: 'r2' })
    await vi.advanceTimersByTimeAsync(2100)
    expect(push).toHaveBeenCalledWith(expect.anything(), 'main', expect.objectContaining({ status: 'lost' }))
  })
  it('resumeWatches restarts polling from storage and dedupes run_id', async () => {
    localStorage.setItem(WATCH_STORAGE_KEY, JSON.stringify([entry]))
    const execute = vi.fn().mockResolvedValue({ state: 'done', record: { status: 'success' } })
    __setTransportsForTests({ execute, push: vi.fn() })
    resumeWatches(); addWatch(entry) // 重复 add 不得双轮询
    await vi.advanceTimersByTimeAsync(2100)
    expect(execute).toHaveBeenCalledTimes(1)
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test src/lib/agent-watch`
Expected: FAIL

- [ ] **Step 3: 实现 store**

要点(全部体现在代码里,无留白):`$state` 的 entries 数组 + `Map<run_id, timer>`;`POLL_MS = 2000`;轮询体照 `agent-workspace/store.svelte.ts:116-151` 的 state 分支(running→续排,done/lost→终态);终态时 `push(requester, 'main', payload)` → 调通知挂钩(未注册则跳过)→ 从列表删除并 persist;`execute` 异常按 lost 处理(executor 被卸载等);持久化 read/write `localStorage[WATCH_STORAGE_KEY]`;生产 transports 与挂钩:

```ts
const transports = {
  execute: (executor: string, context: unknown) =>
    invoke('plugin_v2_execute', { pluginId: executor, command: 'run-status', context }),
  push: (pluginId: string, windowId: string, payload: unknown) =>
    invoke('plugin_v2_window_push', { pluginId, windowId, payload }),
}
export function __setTransportsForTests(t: Partial<typeof transports> | null): void { /* 合并/复位 */ }

// 通知挂钩:本期无人注册,守望器静默完成(通知由统一托盘通知另行承接)。
let notifier: WatchNotifier | null = null
export function registerWatchNotifier(fn: WatchNotifier | null): void { notifier = fn }
// 终态处调用:notifier?.({ title: e.notify.title, body: e.notify.body, openPath: e.notify.open_path, status })
```

App.svelte 接线:

```ts
import { addWatch, resumeWatches } from './lib/agent-watch/store.svelte'
// onMount 内:
resumeWatches()
listen('agent-watch:add', (e) => addWatch(e.payload as WatchEntry))
```

纪律:轮询一律 setTimeout 链,不进 `$effect`;若在 `$effect` 内调 store 函数必须 `untrack`(v4.2.4 教训)。

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test src/lib/agent-watch && pnpm check`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/agent-watch/ src/App.svelte
git commit -m "feat(agent-watch): 宿主守望器——轮询 run-status,终态回推插件窗口+预留通知挂钩,重启可恢复"
```

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
`title, editorPlaceholder, save, saved, delegate, delegating, waitHint, settings, ideaDir, history, statusDraft, statusRunning, statusDone, statusFailed, openResult, retry, needVault, agentMissing, agentMissingHint, celebrate, templateH1, templateHint, sectionDomain, sectionTransfer, sectionResources, sectionOutcome, deleteConfirm, close`

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

### Task 13: agent-client(探活/委托/watch 注册)

**Files:**
- Create: `plugins-src/idea-spark/src/lib/agent-client.ts` / Test: `agent-client.test.ts`

**Interfaces:**
- Consumes: `bridge().request(method, params)`(bridge.ts);Task 2/3 的桥方法;Task 12 的 `seedTaskTemplate`;Task 11 的 `proofPathFor`。
- Produces:

```ts
export const EXECUTOR = 'notemd.claude-agent'
export type DelegateResult = { ok: true; runId: string } | { ok: false; reason: 'agent-missing' | 'error'; message: string }
export async function delegateIdea(ideaRelPath: string, ideaTitle: string): Promise<DelegateResult>
```

`delegateIdea` 内部顺序(测试逐步校验):
1. `host.plugin.execute {plugin_id: EXECUTOR, command: 'tasks.list', context: {}}` 探活;抛错 → `{ok:false, reason:'agent-missing'}`(消息含引导安装文案 key 由 UI 层翻译)。
2. `seedTaskTemplate`(io 用 `host.vault.exists`/`host.vault.write`)。
3. `host.vault.info` 取 `root`,拼绝对路径 `${root}/${ideaRelPath}`(claude-agent 的 `run-note` 期望绝对路径,见其 `note_relative_to_vault`)。
4. `host.plugin.execute {plugin_id: EXECUTOR, command: 'run-note', context: {note_path: abs, task: 'idea-proof'}}` → `run_id`。
5. `host.agent.watch {executor: EXECUTOR, task: 'idea-proof', run_id, notify: {title, body, open_path: proofPathFor(ideaRelPath)}}`。
6. 返回 `{ok:true, runId}`。

- [ ] **Step 1: 写失败测试**(mock `window.notemd.request`,按调用序断言 method+params;探活失败路径断言 reason)
- [ ] **Step 2: 确认失败 → 实现 → 通过**

Run: `pnpm --filter idea-spark test`

- [ ] **Step 3: Commit**

```bash
git add plugins-src/idea-spark/src/lib/agent-client.ts plugins-src/idea-spark/src/lib/agent-client.test.ts
git commit -m "feat(idea-spark): agent-client——探活/种模板/run-note/watch 注册全链路"
```

---

### Task 14: App.svelte 主界面(编辑器/保存/历史/设置/委托/欢庆)

**Files:**
- Modify: `plugins-src/idea-spark/src/App.svelte`(替换 Task 10 占位壳)
- Create: `plugins-src/idea-spark/src/components/HistoryList.svelte`、`src/components/SettingsPopover.svelte`、`src/components/Celebration.svelte`
- Create: `plugins-src/idea-spark/src/lib/store.svelte.ts` / Test: `store.test.ts`(状态机部分)

**Interfaces:**
- Consumes: Editor Kit(动态 import `plugin://notemd.idea-spark/__host__/assets/editor-kit-v1.js` → `mountMarkdownEditor`,签名见 Task 5);Task 11/12/13 全部;`bridge().onMessage`(接 `{type:'agent-run-done'}` 与 `{type:'theme-changed'}`——后者 kit 自己消费)。
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

  - **启动序列**:`host.vault.info` → root 为 null 则整窗提示 `needVault`;否则读 `.notemd/idea-spark.json`(`host.vault.read`,不存在按 DEFAULT_STATE)→ `host.vault.list(ideaDir)`(目录不存在视为空)→ 对 pendingRuns 逐个 `run-status` 一次校正状态(done→移出 pending 并落盘,lost→标 failed)→ 动态 import kit 挂编辑器(失败——宿主过旧/404——显示错误文案并降级为纯 textarea)。
  - **保存**:按钮 + `Cmd/Ctrl+S` 键监听;`ideaFileName(md, today, taken)` 定名(today 用 `new Date()` 本地日期 YYYY-MM-DD),`buildIdeaDoc` 包 frontmatter,`host.vault.write(ideaDir/name)`;成功 toast `saved`,当前文档标记为该文件(再次保存=覆盖同文件,不再改名);编辑器内容非模板原样且未保存时关窗前 `host.toast` 提醒(尽力而为)。
  - **委托**:未保存先保存;`delegateIdea(rel, title)`;`agent-missing` → 弹层 `agentMissingHint`(引导插件市场装 claude-agent);成功 → pendingRuns[rel]=runId 落盘,该条状态 `running`,顶部提示 `waitHint`(可关窗)。
  - **完成推送**:`bridge().onMessage` 收 `{type:'agent-run-done', run_id, status, open_path}` → 从 pendingRuns 反查 rel;status success → 状态 done + `<Celebration/>`(纯 CSS confetti,2s 自动收)+「打开结果」按钮(`host.editor.open {path: open_path}`);其他 status → failed + `retry` 按钮(重走 delegateIdea)。
  - **历史列表**:`listIdeas` + `deriveStatus` 渲染;每条:名字(去日期前缀的 slug)、状态徽标(`statusDraft/Running/Done/Failed` 四语)、done 条目有「打开结果」、点击条目载入该 idea 到编辑器(`host.vault.read`,剥 frontmatter 显示 body——用复制的 concept.ts 侧不需要,简单按首个 `---…---` 剥离)。
  - **设置弹层**:ideaDir 文本框 + 保存(写 state 文件,重新 list);校验非空、不以 `/` 开头、不含 `..`。
  - **store.svelte.ts** 收拢上述状态(`docs: []`、`pending`、`failed`、`current`、`busy`),对可单测的纯状态迁移(agent-run-done 的四态迁移、pending 校正)写 vitest。

- [ ] **Step 1: store 状态机失败测试**(agent-run-done: success→done+庆祝标志、error→failed;pending 校正:done/lost 两分支)
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

**Files:**
- Modify: `docs/plugin-v2-development.md`(§5 capability 表补 `plugin.execute:<id>`/`agent.watch`/`editor.kit` 与新方法;§3 contributes 表顺手补上遗漏的 `tray` 字段)
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
git commit -m "docs(plugin-v2): 登记 plugin.execute/agent.watch/editor.kit 三个新 capability 与桥方法"
```

- [ ] **Step 3: dev 构建 + 交给用户的手动验证清单**(我方只起 dev,不做桌面自动化)

```bash
bash scripts/dev-install-plugin.sh idea-spark && pnpm tauri dev
```

清单(逐条人工过):
1. 插件菜单与托盘出现「奇思妙想」;点击开窗,标题中文。
2. 编辑器 rich 模式 live-preview 正常、与主程序主题一致;切主题后插件窗跟随;rich/source 切换保内容。
3. 写 idea → 保存 → vault `inbox/ideas/` 出现带 frontmatter 的 `.md`(Obsidian/CLI 可读)。
4. 未装 claude-agent 时点「委托」出引导;装好后委托 → 状态「论证中」。
5. 窗口开着跑完 → 就地欢庆 + 「打开结果」在主编辑器打开 `.proof.md`;`.proof.md` frontmatter 为 `type: Idea Proof` + `generated`。
6. 委托后关窗、跑完再开窗 → 历史列表该条为「已完成」,可打开结果(本期无系统通知,见 Task 4)。主程序重启后(运行中委托)守望恢复,重开窗仍能看到最终状态。
7. 设置里改 idea 目录生效;vault 未开时提示。

- [ ] **Step 4: 发布**(用户验证通过后;遵守既有纪律)

顺序与坑:
1. **宿主先发**:独立 worktree(`git worktree add` + `.env.release` + `pnpm install`,worktree 下先 `ln -s` moraya-core)跑 `release.sh`;确认 gh 账号 wizlijun;版本自动推导(≥6.804.2)。
2. **插件后发**:`bash scripts/release-plugins.sh --release idea-spark` → `node scripts/gen-plugin-index.mjs`(默认 merge 线上;注意本地 dist-plugins 里旧版/已 drop 包会被扫回索引的坑)→ 按脚本尾部打印的 wrangler 命令上传 R2+KV。
3. manifest `engines.notemd` 若与实际宿主发版号不符(跨天发布),先改成实际值再打包。

---

## Self-Review 记录

- Spec 覆盖:§1 manifest/托盘(Task 10)、§2 UX 全流程与状态推导(Task 11/14)、§2.1+§3.4 Editor Kit(Task 5/6/7)、§3.1 plugin.execute(Task 2)、§3.2 watch+持久化(Task 3/8;通知按用户 2026-08-04 决定移出本期,Task 4 取消,守望器留 `registerWatchNotifier` 挂钩)、§3.3 read_bytes(Task 1)、§4 任务模板(Task 12/13)、§5 OKF(Task 9/11/12)、§6 错误处理(agent 缺失 Task 13/14,无 vault Task 14,lost Task 8/14,engines 门 Task 10)、§7 测试(各任务)+§8 YAGNI(未引入批注/wikilink/数学)。
- 与 spec 的一处已知偏差:kit **不直接 import** `editor-bridge.ts`(它依赖 tabs/insights/Tauri adapters),改为复刻其 createEditor 选项并双向注释互指——spec §3.4 已按此修正。
- 类型一致性:`mountMarkdownEditor`/`KitEditor`(Task 5↔14)、`WatchEntry`/`agent-run-done` payload(Task 3↔8↔14)、`plugin_execute_gate` 三元组(Task 2)、`SparkState`(Task 11↔13↔14)已对齐。
- 外部 API 有两处以实际代码为准的适配点,均已在任务内标注兜底:`MorayaEditorInstance` 成员名(Task 5)、`AssetResolver::get`(Task 6)。
