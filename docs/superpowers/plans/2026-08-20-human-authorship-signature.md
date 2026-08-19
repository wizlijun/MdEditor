# 人写署名(Human Authorship Signature)实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让主程序与插件里每一个「人亲手创建文档」的入口,在 frontmatter 写下
`generated: { by: human:<id>, at }`,把人机区分从「靠缺席推断」变成元数据里的明话。

**Architecture:** 身份沿用既有的 `humanActorId` 规则(git 邮箱 → 用户名 → 系统用户名 → `local`),
主程序经 `src/lib/okf/identity.ts` 的会话缓存 + 启动预热拿到同步值;插件经**扩展后的**
`host.vault.info` 多出的 `author` 字段拿到。所有文本构造函数保持纯函数,署名作为参数注入。
只在**创建**时签,既有文件再保存绝不回填。

**Tech Stack:** TypeScript / Svelte 5(runes)/ Rust(Tauri 2)/ vitest / cargo test /
`yaml` 包 / 项目自有 `scripts/okf-lint-core.mjs`

**Spec:** `docs/superpowers/specs/2026-08-20-human-authorship-signature-design.md`

## Global Constraints

- **只补缺失键**:一切 frontmatter 写入走 `touchConceptFrontmatter` / `conceptFileText`,
  已有键的值与顺序一律不动(OKF §4.1 生产者规则)。
- **actor 格式**:人一律 `human:<id>`(§7)。**任何 agent 侧代码都不得自签 `human:`**。
- **只在创建时签,绝不回填**:`src/lib/outline/store.svelte.ts:145` 与 `:172` 的
  `touchFrontmatter` 调用**不得**传 `generated`。
- **不签的清单**(spec §2.2):roam-import、ebook `book.md`、sync 镜像、既有文件再保存、
  insights 报告、decision-log 看板。
- **身份 id 规则不新造**:`src/lib/okf/actor.ts:32` 的 `humanActorId` 与
  `src-tauri/src/okf/mod.rs:13` 的 `human_id_from` 是唯一两处实现,由
  `scripts/fixtures/okf-human-id.json` 钉住,**本次不改它们的逻辑**。
- **CHANGELOG 是硬门禁**:`CHANGELOG.md` 与 `CHANGELOG.zh-CN.md` 的「未发布 / Unreleased」
  区都必须加条目,否则 `scripts/release.sh` 在 pre-flight 直接停住。
- 全绿标准:`pnpm test`、`pnpm check`、`cargo test`(在 `src-tauri/`、`searchidx/`)。

---

### Task 1: 后端把 vault 的人类身份挂到 `host.vault.info`

插件跑在隔离 webview 里,今天拿不到任何身份。扩 `host.vault.info` 加 `author` 字段
——身份本就由 vault 的 git 配置推出,复用既有 `vault.read` 门禁,纯增字段,老插件读到
`undefined` 优雅降级。

**Files:**
- Modify: `src-tauri/src/okf/mod.rs:47-58`(把 `notemd_okf_human_id` 的主体抽成可复用的 `pub fn`)
- Modify: `src-tauri/src/plugin_runtime/ui_rpc.rs:575-588`(`vault_info`)
- Test: `src-tauri/src/okf/mod.rs` 的 `mod tests`、`src-tauri/src/plugin_runtime/ui_rpc.rs` 的 `mod tests`

**Interfaces:**
- Produces: `crate::okf::human_actor_for_vault(vault: Option<&std::path::Path>) -> String`
  —— 返回**带前缀**的完整 actor 串(如 `human:bruce`),供 Task 1 的 `vault_info` 与
  `notemd_okf_human_id` 共用。
- Produces: `host.vault.info` 的响应新增 `"author": "human:<id>"`;无 vault 时为 `null`。

- [ ] **Step 1: 写失败的测试(后端身份可复用)**

在 `src-tauri/src/okf/mod.rs` 的 `mod tests` 里加:

```rust
    /// `human_actor_for_vault` 是给桥用的完整 actor 串(带 `human:` 前缀),
    /// 不是裸 id —— 插件拿到就能直接写进 frontmatter,不必各自拼前缀。
    #[test]
    fn human_actor_for_vault_carries_the_okf_prefix() {
        let got = human_actor_for_vault(None);
        assert!(got.starts_with("human:"), "got: {got}");
        assert!(got.len() > "human:".len(), "id must not be empty: {got}");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test okf::tests::human_actor_for_vault_carries_the_okf_prefix`
Expected: FAIL,`cannot find function human_actor_for_vault`

- [ ] **Step 3: 抽出可复用函数**

在 `src-tauri/src/okf/mod.rs` 里,把现有 `notemd_okf_human_id` 改成薄壳,并新增:

```rust
/// 本机人类身份的**完整 OKF actor 串**(`human:<id>`,§7)。`vault` 为 `None`
/// 或不是目录时只用系统用户名。插件桥(`host.vault.info` 的 `author`)与
/// `notemd_okf_human_id` 共用这一条路径,保证同一个人在两条通道上署名相同。
pub fn human_actor_for_vault(vault: Option<&Path>) -> String {
    format!("human:{}", human_id_for_vault(vault))
}

/// 裸 id(不带前缀)。`notemd_okf_human_id` 的实现主体。
pub fn human_id_for_vault(vault: Option<&Path>) -> String {
    let os_user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    match vault.filter(|p| p.is_dir()) {
        Some(d) => human_id_from(&git_config(d, "user.name"), &git_config(d, "user.email"), &os_user),
        None => human_id_from("", "", &os_user),
    }
}

#[tauri::command]
pub fn notemd_okf_human_id(vault_path: Option<String>) -> String {
    human_id_for_vault(vault_path.as_deref().map(Path::new))
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test okf::`
Expected: PASS(含既有的 `matches_the_shared_fixture_with_the_frontend`)

- [ ] **Step 5: 写失败的测试(桥暴露 author)**

在 `src-tauri/src/plugin_runtime/ui_rpc.rs` 的 `mod tests` 里,紧挨着既有的
`vault_info_reports_root_and_dirs` 加:

```rust
    /// 插件在隔离 webview 里没有任何 IPC,`vault.info` 是它唯一能问到「我是谁」
    /// 的地方。字段是纯增的:老插件读不到就是 undefined,不会炸。
    #[tokio::test]
    async fn vault_info_carries_the_human_author() {
        let s = StubServices::default();
        let r = run(&s, &["vault.read"], "host.vault.info", serde_json::json!({})).await;
        let author = r.result.expect("vault.info must answer")["author"].clone();
        assert!(
            author.as_str().is_some_and(|a| a.starts_with("human:")),
            "author must be a full OKF actor string, got: {author}"
        );
    }
```

- [ ] **Step 6: 跑测试确认失败**

Run: `cd src-tauri && cargo test plugin_runtime::ui_rpc::tests::vault_info_carries_the_human_author`
Expected: FAIL,`author` 是 `Null`

- [ ] **Step 7: 在 `vault_info` 里补上 author**

`src-tauri/src/plugin_runtime/ui_rpc.rs:575-588` 改成:

```rust
/// `{} → { root, wiki_dir, daily_dir, author }`(无 vault 时 root/dir 为 null;
/// dir 名未设时回退到前端默认值)。`author` 是本机的完整 OKF actor 串
/// (`human:<id>`,§7)——插件写「人亲手敲的」文档时的署名来源。它由 vault 的
/// git 配置推出,所以就归在 vault info 下,复用同一个 `vault.read` 门禁。
pub(crate) fn vault_info(services: &dyn HostServices) -> serde_json::Value {
    match services.vault_root() {
        None => serde_json::json!({
            "root": null, "wiki_dir": null, "daily_dir": null,
            "author": crate::okf::human_actor_for_vault(None),
        }),
        Some(root) => {
            let (wiki, daily) = services.wiki_daily_dirs();
            serde_json::json!({
                "root": root.to_string_lossy(),
                "wiki_dir": wiki.unwrap_or_else(|| DEFAULT_WIKI_DIR.into()),
                "daily_dir": daily.unwrap_or_else(|| DEFAULT_DAILY_DIR.into()),
                "author": crate::okf::human_actor_for_vault(Some(root.as_path())),
            })
        }
    }
}
```

- [ ] **Step 8: 跑测试确认通过**

Run: `cd src-tauri && cargo test plugin_runtime::ui_rpc::tests::vault_info`
Expected: PASS(两条 `vault_info_*` 都过)

- [ ] **Step 9: 提交**

```bash
git add src-tauri/src/okf/mod.rs src-tauri/src/plugin_runtime/ui_rpc.rs
git commit -m "feat(okf): host.vault.info 带上本机 human: 署名,供插件写人工文档时使用"
```

---

### Task 2: 前端同步取身份(预热 + 同步取值器)

⌘N 是最热的路径,不能为它阻塞两次 `git config` 子进程。缓存已经有了,补预热和同步取值。

**Files:**
- Modify: `src/lib/okf/identity.ts`
- Modify: `src/App.svelte:138`(`onMount` 里预热)
- Test: `src/lib/okf/identity.test.ts`(新建)

**Interfaces:**
- Consumes: `humanActor(): Promise<string>`(已存在)、`resetHumanActor(): void`(已存在)
- Produces: `humanActorNow(): string | null` —— 同步返回已缓存的完整 actor 串,未预热返回 `null`
- Produces: `warmHumanActor(): void` —— 触发一次后台解析,不返回、不抛错

- [ ] **Step 1: 写失败的测试**

新建 `src/lib/okf/identity.test.ts`:

```ts
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { humanActor, humanActorNow, warmHumanActor, resetHumanActor } from './identity'

vi.mock('@tauri-apps/api/core', () => ({ invoke: async () => 'bruce' }))
vi.mock('../sotvault.svelte', () => ({ sotvaultStore: { vaultRoot: '/v' } }))

describe('humanActorNow', () => {
  beforeEach(() => resetHumanActor())

  it('冷启动时返回 null——宁可漏签,也不阻塞热路径', () => {
    expect(humanActorNow()).toBeNull()
  })

  it('预热后同步就能拿到完整 actor 串', async () => {
    warmHumanActor()
    await vi.waitFor(() => expect(humanActorNow()).toBe('human:bruce'))
  })

  it('await 过 humanActor 之后同步取值器也就热了', async () => {
    expect(await humanActor()).toBe('human:bruce')
    expect(humanActorNow()).toBe('human:bruce')
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/lib/okf/identity.test.ts`
Expected: FAIL,`humanActorNow is not a function`

- [ ] **Step 3: 实现**

在 `src/lib/okf/identity.ts` 末尾(`resetHumanActor` 之前)加:

```ts
/**
 * 已解析的身份,**同步**取。未预热时返回 null —— 调用方(⌘N 这类热路径)
 * 拿 null 就不签,绝不为了一个署名去阻塞两次 git 子进程。窗口由
 * `warmHumanActor()` 在启动时关掉。
 */
export function humanActorNow(): string | null {
  return cached
}

/** 后台预热缓存。不返回、不抛错——预热失败最多就是这一轮不签。 */
export function warmHumanActor(): void {
  void humanActor().catch(() => {})
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/lib/okf/identity.test.ts`
Expected: PASS(3 条)

- [ ] **Step 5: 在应用启动时预热**

`src/App.svelte` 的 `onMount(() => {` 内部(第 138 行起的回调体最前面)加:

```ts
    // 人写署名要在 ⌘N 这类同步热路径上同步取到(src/lib/okf/identity.ts)。
    void import('./lib/okf/identity').then(m => m.warmHumanActor())
```

- [ ] **Step 6: vault 切换时重取身份**

身份来自 vault 的 git 配置,换 vault 就换人。`src/lib/sotvault.svelte.ts:66-71`
已经有一个「根变了」的判定,挂在那里:

```ts
    const rootChanged = sotvaultStore.vaultRoot !== root
    sotvaultStore.vaultRoot = root
```

在同一函数末尾的 `if (rootChanged) vaultRootChangedHandler?.()`(第 71 行)之前插入:

```ts
    // 身份由 vault 的 git 配置推出,换 vault 就换人。
    if (rootChanged) {
      void import('./okf/identity').then(m => { m.resetHumanActor(); m.warmHumanActor() })
    }
```

- [ ] **Step 7: 全量回归**

Run: `pnpm test && pnpm check`
Expected: 全绿

- [ ] **Step 8: 提交**

```bash
git add src/lib/okf/identity.ts src/lib/okf/identity.test.ts src/App.svelte src/lib/sotvault.svelte.ts
git commit -m "feat(okf): 人类身份启动预热 + 同步取值器,热路径不为署名阻塞"
```

---

### Task 3: ⌘N 新建与快速笔记签名

**Files:**
- Modify: `src/lib/new-file.ts`
- Modify: `src/lib/tabs.svelte.ts:90-91`
- Modify: `src/lib/quick-note.svelte.ts:70`
- Test: `src/lib/new-file.test.ts`

**Interfaces:**
- Consumes: `humanActorNow()`(Task 2)、`CONCEPT_TYPE` / `conceptFileText`(已存在)、
  `ConceptMeta['generated']` = `{ by: string; at: string }`(`src/lib/okf/concept.ts:88` 已定义)
- Produces: `newFileText(body: string, author?: { by: string; at: string }): string`

- [ ] **Step 1: 写失败的测试**

在 `src/lib/new-file.test.ts` 的 `describe` 里追加:

```ts
  it('带上人写署名时写进 generated(OKF §5.2/§7)', () => {
    const text = newFileText('# 标题\n\n正文\n', { by: 'human:bruce', at: '2026-08-20T10:31:00.000Z' })
    expect(text).toBe(
      `---\ntype: ${CONCEPT_TYPE.note}\ntitle: 标题\ngenerated:\n  by: human:bruce\n  at: 2026-08-20T10:31:00.000Z\n---\n# 标题\n\n正文\n`,
    )
  })

  it('署名后仍满足 OKF 硬约束', () => {
    const text = newFileText('# 标题\n\n正文\n', { by: 'human:bruce', at: '2026-08-20T10:31:00.000Z' })
    expect(lintText('untitled.md', text)).toEqual([])
  })

  it('拿不到身份就不签——宁可无署名,也不写一个假的', () => {
    expect(newFileText('# 标题\n', undefined))
      .toBe(`---\ntype: ${CONCEPT_TYPE.note}\ntitle: 标题\n---\n# 标题\n`)
  })
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/lib/new-file.test.ts`
Expected: FAIL —— 前两条,实际输出里没有 `generated`

- [ ] **Step 3: 实现**

`src/lib/new-file.ts` 全文替换为:

```ts
// 新建文档的文本:正文 + OKF 概念文档头(§4.1 的必填 type)。
// `author` 在场时写 §5.2 的 `generated` —— 这是「人写的、是谁写的」在元数据层
// 的唯一表达(spec: docs/superpowers/specs/2026-08-20-human-authorship-signature-design.md)。
// 拿不到身份时**不签**:一个缺席的署名是诚实的,一个编出来的不是。
import { CONCEPT_TYPE, conceptFileText, type ConceptMeta } from './okf/concept'

const H1 = /^#\s+(.+?)\s*$/m

/** 给一段新建正文补上 frontmatter;已有 frontmatter 的正文原样返回。 */
export function newFileText(body: string, author?: ConceptMeta['generated']): string {
  if (body.startsWith('---\n')) return body
  const title = body.match(H1)?.[1]
  return conceptFileText({ type: CONCEPT_TYPE.note, title, generated: author }, body)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/lib/new-file.test.ts`
Expected: PASS(全部 7 条)

- [ ] **Step 5: 接线 ⌘N**

`src/lib/tabs.svelte.ts` 顶部 import 区把第 13 行改为同时引入身份:

```ts
import { newFileText } from './new-file'
import { humanActorNow } from './okf/identity'
```

第 91 行改为:

```ts
  const by = humanActorNow()
  const content = newFileText(
    newFileTemplates[Math.floor(Math.random() * newFileTemplates.length)],
    by ? { by, at: new Date().toISOString() } : undefined,
  )
```

- [ ] **Step 6: 接线快速笔记**

`src/lib/quick-note.svelte.ts` 第 70 行改为:

```ts
      const by = (await import('./okf/identity')).humanActorNow()
      await openPathBackedMarkdownDraft(
        fullPath,
        newFileText('', by ? { by, at: new Date().toISOString() } : undefined),
        { skipEmptySave: true },
      )
```

- [ ] **Step 7: 回归**

Run: `pnpm test && pnpm check`
Expected: 全绿。若 `src/lib/tabs.test.ts` 因新增 frontmatter 行而红,按实际输出更新期望值
——**不要**为了让测试变绿去掉署名。

- [ ] **Step 8: 提交**

```bash
git add src/lib/new-file.ts src/lib/new-file.test.ts src/lib/tabs.svelte.ts src/lib/quick-note.svelte.ts src/lib/tabs.test.ts
git commit -m "feat(okf): ⌘N 新建与快速笔记写下 human: 署名"
```

---

### Task 4: `.note.md` 手记 / 日记 / vault 内 wikipage 签名

这三条路共用 `newOutlineFileText` → `touchFrontmatter`。**关键红线**:
`touchFrontmatter` 也被保存路径用(`store.svelte.ts:145,172`),那两处**绝不能**传
`generated`,否则一个装着 agent 答复的旧 `.note.md` 会因为你保存了一次就获得人写署名。

**Files:**
- Modify: `src/lib/outline/frontmatter.ts:6-18`(`TouchOpts`)、`:33-36`
- Modify: `src/lib/outline/create.ts:8-11`(`newOutlineFileText`)、`:24-29`(`ensureOutlineFile`)
- Test: `src/lib/outline/create.test.ts`、`src/lib/outline/frontmatter.test.ts`

**Interfaces:**
- Consumes: `humanActorNow()`(Task 2)
- Produces: `TouchOpts.generated?: { by: string; at: string }`
- Produces: `newOutlineFileText(title: string, now?: string, type?: string, author?: { by: string; at: string }): string`
- Produces: `ensureOutlineFile(path: string, title?: string, type?: string): Promise<string>`
  —— 签名不变,内部自取身份(它是 async,可以直接 `await humanActor()`)

- [ ] **Step 1: 写失败的测试**

在 `src/lib/outline/create.test.ts` 的 `describe('newOutlineFileText')` 里追加:

```ts
  it('带署名时写 generated,且排在 updated 之前(只补缺失键,顺序即写入序)', () => {
    const text = newOutlineFileText('我的笔记', '2026-07-10T09:00:00.000Z', undefined, {
      by: 'human:bruce', at: '2026-07-10T09:00:00.000Z',
    })
    expect(text).toContain('generated:\n  by: human:bruce\n  at: 2026-07-10T09:00:00.000Z')
    expect(lintText('我的笔记.note.md', text)).toEqual([])
  })

  it('不带署名时逐字保持原样——旧行为零变化', () => {
    expect(newOutlineFileText('我的笔记', '2026-07-10T09:00:00.000Z'))
      .not.toContain('generated')
  })
```

在 `src/lib/outline/frontmatter.test.ts` 里追加(这条是防回填的红线测试):

```ts
  it('已有文件再 touch 不会长出 generated——只在创建时签', () => {
    const raw = 'type: Outline Note\ntitle: T\ncreated: 2026-01-01T00:00:00.000Z'
    const out = touchFrontmatter(raw, { title: 'T', now: '2026-08-20T10:00:00.000Z' })
    expect(out).not.toContain('generated')
  })

  it('已有 generated 的文件,再传一个署名也不覆盖', () => {
    const raw = 'type: Outline Note\ntitle: T\ngenerated:\n  by: claude-code/opus-5\n  at: 2026-01-01T00:00:00.000Z'
    const out = touchFrontmatter(raw, {
      title: 'T', now: '2026-08-20T10:00:00.000Z',
      generated: { by: 'human:bruce', at: '2026-08-20T10:00:00.000Z' },
    })
    expect(out).toContain('by: claude-code/opus-5')
    expect(out).not.toContain('human:bruce')
  })
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/lib/outline/create.test.ts src/lib/outline/frontmatter.test.ts`
Expected: FAIL —— `newOutlineFileText` 只收 3 个参数;`TouchOpts` 没有 `generated`

- [ ] **Step 3: 给 TouchOpts 加 generated**

`src/lib/outline/frontmatter.ts` 的 `TouchOpts` 接口里,`sources` 之后加:

```ts
  /** 缺 generated 时写入的生成署名(§5.2)。**只有创建路径才传** ——
   *  保存路径传了会让旧文件凭一次保存就获得人写署名(spec §2.2)。 */
  generated?: ConceptMeta['generated']
```

`touchFrontmatter` 里 `touchConceptFrontmatter(raw, { ... })` 的对象字面量补一行:

```ts
    generated: opts.generated,
```

- [ ] **Step 4: 给创建路径接上署名**

`src/lib/outline/create.ts` 改这两个函数:

```ts
/** 新大纲文件的完整文本:front-matter + 单个空节点(空大纲)。
 *  type 缺省由 touchFrontmatter 填 Outline Note(OKF §4.1)。
 *  `author` 在场时写 §5.2 的 `generated` —— 只有这条创建路径传,保存路径不传。 */
export function newOutlineFileText(
  title: string, now?: string, type?: string, author?: ConceptMeta['generated'],
): string {
  const fm = touchFrontmatter(null, { title, now, type, generated: author })
  return `---\n${fm}\n---\n- \n`
}
```

```ts
/** 确保 .note.md 存在(不存在则以空大纲创建)。title 缺省取文件名;
 *  wikipage 建页传原始标题(spec §5:文件名 slug 化、fm title 存原文)。
 *  新建的文件签人写署名 —— 手记/日记/wikipage 都是你的一个动作直接产生的。 */
export async function ensureOutlineFile(path: string, title?: string, type?: string): Promise<string> {
  const { exists, writeTextFile } = await import('@tauri-apps/plugin-fs')
  if (!(await exists(path).catch(() => false))) {
    const { humanActor } = await import('../okf/identity')
    const by = await humanActor().catch(() => null)
    const author = by ? { by, at: new Date().toISOString() } : undefined
    await writeTextFile(path, newOutlineFileText(title ?? pageNameOf(path), undefined, type, author))
  }
  return path
}
```

顶部 import 补 `type ConceptMeta`:

```ts
import { CONCEPT_TYPE, conceptFileText, isReservedConceptName, type ConceptMeta } from '../okf/concept'
```

- [ ] **Step 5: 跑测试确认通过**

Run: `pnpm vitest run src/lib/outline/`
Expected: PASS

- [ ] **Step 6: 确认保存路径没被污染**

Run: `rg -n "generated" src/lib/outline/store.svelte.ts`
Expected: **零命中**。有命中就是踩了 Global Constraints 的红线,删掉。

- [ ] **Step 7: 回归 + 提交**

```bash
pnpm test && pnpm check
git add src/lib/outline/frontmatter.ts src/lib/outline/frontmatter.test.ts src/lib/outline/create.ts src/lib/outline/create.test.ts
git commit -m "feat(okf): 手记/日记/wikipage 新建时写下 human: 署名(保存路径不回填)"
```

---

### Task 5: vault 外建页签名,并堵上富文本 wikilink 的 0 字节洞

`src/components/RichEditor.svelte:405` 对不存在的 `[[链接]]` 直接写 `''` ——
0 字节 `.md`,无 frontmatter、无 `type`,违反 OKF §4.1 唯一的硬约束,
`pnpm okf:lint` 会报,索引里落进最低档 `Unlabeled`。
`src/lib/outline/backlinks-io.svelte.ts:138` 对同一件事早就走 `newPageFileText` 了。

**Files:**
- Modify: `src/lib/outline/create.ts:16-21`(`newPageFileText`)
- Modify: `src/lib/outline/backlinks-io.svelte.ts:136-139`
- Modify: `src/components/RichEditor.svelte:403-406`
- Test: `src/lib/outline/create.test.ts`

**Interfaces:**
- Produces: `newPageFileText(title: string, author?: { by: string; at: string }): string`

- [ ] **Step 1: 写失败的测试**

在 `src/lib/outline/create.test.ts` 追加:

```ts
  it('newPageFileText 带署名时写 generated', () => {
    const text = newPageFileText('某个概念', { by: 'human:bruce', at: '2026-08-20T10:31:00.000Z' })
    expect(text).toContain('generated:\n  by: human:bruce')
    expect(lintText('某个概念.md', text)).toEqual([])
  })

  it('保留名不因为署名而破例——index/log 仍然只写正文', () => {
    expect(newPageFileText('index', { by: 'human:bruce', at: '2026-08-20T10:31:00.000Z' }))
      .toBe('# index\n')
  })
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/lib/outline/create.test.ts`
Expected: FAIL —— 第一条,输出里没有 `generated`

- [ ] **Step 3: 实现**

`src/lib/outline/create.ts` 的 `newPageFileText` 改为:

```ts
/** 新普通页(解析 wikilink 时建的 `.md`)的完整文本。
 *  [[index]] / [[log]] 会落到保留文件名上:这类文件 **MUST NOT** 是概念文档
 *  (§8/§9),所以只写正文,不盖 frontmatter —— 文件名保持用户看到的样子,
 *  署名也一并不写(没有 frontmatter 可挂)。 */
export function newPageFileText(title: string, author?: ConceptMeta['generated']): string {
  const body = `# ${title}\n`
  if (isReservedConceptName(`${title}.md`)) return body
  return conceptFileText({ type: CONCEPT_TYPE.note, title, generated: author }, body)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/lib/outline/create.test.ts`
Expected: PASS

- [ ] **Step 5: 接线 vault 外建页**

`src/lib/outline/backlinks-io.svelte.ts` 第 136-139 行改为:

```ts
    const { exists, writeTextFile } = await import('@tauri-apps/plugin-fs')
    if (!(await exists(path).catch(() => false))) {
      const { humanActor } = await import('../okf/identity')
      const by = await humanActor().catch(() => null)
      await writeTextFile(path, newPageFileText(safe, by ? { by, at: new Date().toISOString() } : undefined))
    }
```

- [ ] **Step 6: 堵上富文本那条洞**

`src/components/RichEditor.svelte` 的 `openWikilink` 里,把

```ts
      const { exists, writeTextFile } = await import('@tauri-apps/plugin-fs')
      if (!(await exists(abs))) await writeTextFile(abs, '')
```

换成:

```ts
      const { exists, writeTextFile } = await import('@tauri-apps/plugin-fs')
      if (!(await exists(abs))) {
        // 0 字节文件违反 OKF §4.1(必须有可解析 frontmatter + 非空 type),
        // 而且在索引里落进最低档 Unlabeled。走和大纲面板同一条建页路径。
        const [{ newPageFileText }, { humanActor }, { basename }] = await Promise.all([
          import('../lib/outline/create'),
          import('../lib/okf/identity'),
          import('../lib/paths'),
        ])
        const by = await humanActor().catch(() => null)
        const title = basename(abs).replace(/\.md$/i, '')
        await writeTextFile(abs, newPageFileText(title, by ? { by, at: new Date().toISOString() } : undefined))
      }
```

- [ ] **Step 7: 回归**

Run: `pnpm test && pnpm check`
Expected: 全绿

- [ ] **Step 8: 提交**

```bash
git add src/lib/outline/create.ts src/lib/outline/create.test.ts src/lib/outline/backlinks-io.svelte.ts src/components/RichEditor.svelte
git commit -m "feat(okf): vault 外建页签署名;富文本 wikilink 不再建 0 字节非法文档"
```

---

### Task 6: idea-spark 的 idea 原文签名

idea 原文是这个插件的全部意义 —— 打开就写字,一个字都是你的。它今天的注释明写
「human-authored,所以不盖 `generated`」,正是本次要翻过来的那句话。

**Files:**
- Modify: `plugins-src/idea-spark/src/lib/bridge.ts`(`VaultInfo` 加 `author`)
- Modify: `plugins-src/idea-spark/src/lib/idea-doc.ts:1-11`
- Modify: `plugins-src/idea-spark/src/lib/store.svelte.ts:296-301`
- Test: `plugins-src/idea-spark/src/lib/idea-doc.test.ts`

**Interfaces:**
- Consumes: `host.vault.info` 的 `author` 字段(Task 1)
- Produces: `buildIdeaDoc(body: string, nowIso: string, author?: string): string`
  —— `author` 是**完整 actor 串**(`human:bruce`),不是裸 id
- Produces: `ideaDocText(s: SparkStore, markdown: string, nowIso: string, author?: string): string`

- [ ] **Step 1: 写失败的测试**

在 `plugins-src/idea-spark/src/lib/idea-doc.test.ts` 追加:

```ts
  it('带署名时写 generated —— idea 原文是你亲手敲的', () => {
    const doc = buildIdeaDoc('一个想法\n', '2026-08-20T10:31:00.000Z', 'human:bruce')
    expect(doc).toBe(
      '---\ntype: Idea\ncreated: 2026-08-20T10:31:00.000Z\n' +
      'generated:\n  by: human:bruce\n  at: 2026-08-20T10:31:00.000Z\n---\n一个想法\n',
    )
  })

  it('宿主太老、拿不到 author 时不签,不炸', () => {
    expect(buildIdeaDoc('一个想法\n', '2026-08-20T10:31:00.000Z', undefined))
      .toBe('---\ntype: Idea\ncreated: 2026-08-20T10:31:00.000Z\n---\n一个想法\n')
  })

  it('重存已有 idea 不会补签 —— 只在创建时签', () => {
    const out = rebuildIdeaDoc('type: Idea\ncreated: 2026-01-01T00:00:00.000Z', '正文\n', '2026-08-20T10:31:00.000Z')
    expect(out).not.toContain('generated')
  })
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd plugins-src/idea-spark && pnpm vitest run src/lib/idea-doc.test.ts`
Expected: FAIL —— `buildIdeaDoc` 只收两个参数

- [ ] **Step 3: 实现**

`plugins-src/idea-spark/src/lib/idea-doc.ts` 的文件头注释与 `buildIdeaDoc` 改为:

```ts
// Builds the on-disk text for an idea's original `.md` file. A thin wrapper
// over the vendored OKF writer (./okf/concept.ts). Idea originals are
// human-authored and now say so: `generated: { by: human:<id>, at }` (OKF
// §5.2/§7). The actor comes from the host (`host.vault.info` → `author`);
// when the host is too old to answer, we leave it unsigned rather than
// inventing one.
```

```ts
/** Full idea document text: OKF frontmatter (`type: Idea`, `created`,
 *  and — when the host told us who you are — `generated`) + body. */
export function buildIdeaDoc(body: string, nowIso: string, author?: string): string {
  return conceptFileText(
    {
      type: CONCEPT_TYPE.idea,
      created: nowIso,
      generated: author ? { by: author, at: nowIso } : undefined,
    },
    body,
  )
}
```

注意 `rebuildIdeaDoc` **不动** —— 它是重存路径,不签(测试第 3 条钉住)。
其内部 `buildIdeaDoc(...)` 的兜底调用保持两参数形式。

- [ ] **Step 4: 跑测试确认通过**

Run: `cd plugins-src/idea-spark && pnpm vitest run src/lib/idea-doc.test.ts`
Expected: PASS

- [ ] **Step 5: bridge 类型加 author**

`plugins-src/idea-spark/src/lib/bridge.ts` 的 `VaultInfo` 接口加:

```ts
  /** 本机人类身份的完整 OKF actor 串(`human:<id>`)。宿主 < 本次发布时为 undefined。 */
  author?: string
```

- [ ] **Step 6: 接线 store**

`plugins-src/idea-spark/src/lib/store.svelte.ts` 的 `ideaDocText` 加一个可选参数并透传:

```ts
export function ideaDocText(s: SparkStore, markdown: string, nowIso: string, author?: string): string {
  return s.currentFrontmatter === null
    ? buildIdeaDoc(markdown, nowIso, author)
    : rebuildIdeaDoc(s.currentFrontmatter, markdown, nowIso)
}
```

身份在 `boot()` 里随 `vaultInfo()` 一起取到就存进 store,**不要**在每次保存时
再发一次 RPC。三处改动:

`SparkStore` 接口(第 29 行 `vaultRoot: string | null` 之后)加:

```ts
  /** 本机人类身份的完整 OKF actor 串(`human:<id>`);宿主太老时为 null。 */
  author: string | null
```

初始值(第 113 行 `vaultRoot: null,` 之后)加:

```ts
    author: null,
```

`boot()` 里(第 592 行 `state.vaultRoot = info?.root ?? null` 之后)加:

```ts
    state.author = info?.author ?? null
```

`saveIdea()` 里(第 774 行)改为:

```ts
    const text = ideaDocText(state, markdown, new Date().toISOString(), state.author ?? undefined)
```

注意 `boot()` 第 591 行的 `.catch()` 兜底用的是一个**就地收窄的**类型(不是 `VaultInfo`
——那个接口要求 `wiki_dir`/`daily_dir`,补全只是噪音)。跟着它加一个字段即可:

```ts
    const info = await vaultInfo().catch(
      () => ({ root: null, author: undefined }) as { root: string | null; author?: string },
    )
```

- [ ] **Step 7: 回归 + 提交**

```bash
cd plugins-src/idea-spark && pnpm vitest run && pnpm build && cd -
git add plugins-src/idea-spark/src
git commit -m "feat(idea-spark): idea 原文写下 human: 署名(宿主太老则不签)"
```

---

### Task 7: trace-source 的委托稿走 OKF 枢纽并签名

委托稿是你自己的话,在 agent 介入之前就落盘。它今天是手拼字符串 + 手写 YAML 引号
转义 —— 往里塞一个**嵌套的** `generated:` mapping 只会把引号 bug 翻倍。
按 idea-spark 的既有做法 vendor 一份 `concept.ts`(CLAUDE.md:任何新写入点都必须经它)。

**Files:**
- Create: `plugins-src/trace-source/src/lib/okf/concept.ts`(从 `src/lib/okf/concept.ts` 逐字复制)
- Modify: `plugins-src/trace-source/src/lib/bridge.ts`(`VaultInfo` 加 `author`)
- Modify: `plugins-src/trace-source/src/lib/inbox.ts:136-156`
- Modify: `plugins-src/trace-source/src/App.svelte:350`
- Test: `plugins-src/trace-source/src/lib/inbox.test.ts`

**Interfaces:**
- Consumes: `host.vault.info` 的 `author` 字段(Task 1)
- Produces: `buildRequestDoc(text: string, author?: string): string`

- [ ] **Step 1: vendor concept.ts**

```bash
mkdir -p plugins-src/trace-source/src/lib/okf
cp src/lib/okf/concept.ts plugins-src/trace-source/src/lib/okf/concept.ts
```

改 vendored 副本第 5 行的 import 路径(宿主版是 `from '../paths'`),
按 trace-source 的实际结构指向它自己的 `basename`;若该插件没有 `paths` 模块,
把 `basename` 就地实现在文件顶部:

```ts
const basename = (p: string): string => p.split('/').pop() ?? p
```

并删掉那行 `import { basename } from '../paths'`。

- [ ] **Step 2: 写失败的测试**

在 `plugins-src/trace-source/src/lib/inbox.test.ts` 的 `describe('buildRequestDoc')` 里追加:

```ts
  it('带署名时写 generated —— 委托稿是你自己的话', () => {
    const doc = buildRequestDoc('> 这段话是谁说的\n', 'human:bruce')
    expect(doc).toContain('type: Trace Request')
    expect(doc).toContain('generated:\n  by: human:bruce\n  at: ')
  })

  it('宿主太老、拿不到 author 时不签,产物仍合规', () => {
    const doc = buildRequestDoc('> 这段话是谁说的\n')
    expect(doc).not.toContain('generated')
    expect(doc).toContain('type: Trace Request')
  })

  it('标题里的冒号/引号仍被 YAML 安全处理(交给 concept.ts,不再手拼)', () => {
    const doc = buildRequestDoc('> a: "b" \\ c\n')
    expect(() => stripFrontmatter(doc)).not.toThrow()
    expect(doc).toContain('type: Trace Request')
  })
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cd plugins-src/trace-source && pnpm vitest run src/lib/inbox.test.ts`
Expected: FAIL —— `buildRequestDoc` 只收一个参数

- [ ] **Step 4: 改写 buildRequestDoc**

`plugins-src/trace-source/src/lib/inbox.ts` 里,`buildRequestDoc` 改为:

```ts
/**
 * The request document written to `requestPathFor(...)` at delegation time —
 * the user's own words, saved BEFORE the agent is involved so nothing about
 * the ask is lost to a crash, a failed run, or a closed window.
 *
 * OKF: `type: Trace Request` (registered host-side in concept.ts; Human tier
 * in searchidx origin mapping) plus, when the host told us who you are,
 * `generated: { by: human:<id>, at }` — this is a document you wrote by hand.
 * YAML escaping is the vendored writer's job now, not a hand-rolled quote.
 */
export function buildRequestDoc(text: string, author?: string): string {
  const firstLine =
    text
      .split('\n')
      .map((l) => l.replace(/^>\s*/, '').trim())
      .find((l) => l !== '') ?? ''
  const cut = firstLine.length > TITLE_MAX ? `${firstLine.slice(0, TITLE_MAX)}…` : firstLine
  const now = new Date().toISOString()
  return conceptFileText(
    {
      type: CONCEPT_TYPE.traceRequest,
      title: cut || 'Trace request',
      generated: author ? { by: author, at: now } : undefined,
    },
    `\n${text.replace(/\s+$/, '')}\n`,
  )
}
```

文件顶部 import 补:

```ts
import { CONCEPT_TYPE, conceptFileText } from './okf/concept'
```

并删掉原来手写的 `needsQuote` / `yamlTitle` 两行(现在由 `yamlSafeNode` 与 `yaml` 包负责)。

- [ ] **Step 5: 跑测试确认通过**

Run: `cd plugins-src/trace-source && pnpm vitest run src/lib/inbox.test.ts`
Expected: PASS。若 `stripFrontmatter` 的既有往返测试因正文首行空行数变化而红,
按实际输出调期望值 —— 但**必须**确认 `stripFrontmatter(buildRequestDoc(t))` 仍能
原样取回 `t`,那是「载回编辑器再来一次」的功能前提。

- [ ] **Step 6: bridge 类型加 author + 接线**

`plugins-src/trace-source/src/lib/bridge.ts` 的 `VaultInfo` 接口加:

```ts
  /** 本机人类身份的完整 OKF actor 串(`human:<id>`)。宿主 < 本次发布时为 undefined。 */
  author?: string
```

`plugins-src/trace-source/src/App.svelte:350` 改为:

```ts
        const author = (await vaultInfo().catch(() => null))?.author
        await vaultWrite(`${settings.traceDir}/${requestPathFor(reportName)}`, buildRequestDoc(text, author))
```

- [ ] **Step 7: 回归 + 提交**

```bash
cd plugins-src/trace-source && pnpm vitest run && pnpm build && cd -
git add plugins-src/trace-source/src
git commit -m "feat(trace-source): 委托稿走 OKF 枢纽并写下 human: 署名"
```

---

### Task 8: 在 searchidx 钉住「三态可分」与不签清单

读侧本来就在等这个署名(`origin.rs:160-163` 的规则 2 已按 `human:` 前缀分档),
本次不改它的逻辑,但要用测试钉住**现在真的有人写了**之后的三态行为,
以及 `origin.rs:141-163` 自己注释里预警过的滑档陷阱:规则 2 先于规则 4,
一旦有人给 `type: Book` 补了 `generated`,全部导入电子书会静默从 `Source` 掉到 `Derived`。

**Files:**
- Modify: `searchidx/src/origin.rs` 的 `mod tests`(只加测试,不动 `derive` 的任何一行)

**Interfaces:**
- Consumes: `derive(rel_path, fm, globs)`、测试助手 `fm(&str)` / `globs(&[&str])`(均已存在)

- [ ] **Step 1: 写测试**

在 `searchidx/src/origin.rs` 的 `mod tests` 末尾加:

```rust
    /// 三态可分 —— 这是「人写署名」整件事存在的理由。此前只有前两态,
    /// 而且第一态没人写得出来。
    #[test]
    fn the_three_states_of_authorship_are_distinguishable() {
        let human = fm("type: Note\ngenerated:\n  by: human:bruce\n  at: 2026-08-20T10:31:00.000Z");
        assert_eq!(derive("a.md", Some(&human), &globs(&[])), Origin::Human);

        let machine = fm("type: Note\ngenerated:\n  by: claude-code/opus-5\n  at: 2026-08-20T10:31:00.000Z");
        assert_eq!(
            derive("b.md", Some(&machine), &globs(&[])), Origin::Derived,
            "a generator stamp must not inherit Note's Human tier"
        );

        let unclaimed = fm("type: Note");
        assert_eq!(
            derive("c.md", Some(&unclaimed), &globs(&[])), Origin::Human,
            "no stamp falls back to the type mapping (rule 4) — unchanged behaviour"
        );
    }

    /// 不签清单的守卫(spec §2.2)。`book.md` 至今不带 `generated`,所以它走
    /// 规则 4 落在 `Source`。谁哪天给 ebook 导入补了一行 `generated`,
    /// 规则 2 会抢在规则 4 前面把每一本导入的书悄悄挪进 `Derived` —— 这条
    /// 测试就是那一刻的红灯。真要改,去改规则顺序和 spec,不要绕过它。
    #[test]
    fn an_imported_book_stays_source_because_nobody_stamps_it() {
        assert_eq!(derive("books/x.md", Some(&fm("type: Book")), &globs(&[])), Origin::Source);

        let stamped = fm("type: Book\ngenerated:\n  by: process:ebook-import\n  at: 2026-08-20T10:31:00.000Z");
        assert_eq!(
            derive("books/x.md", Some(&stamped), &globs(&[])), Origin::Derived,
            "rule 2 precedes rule 4 — this is why ebook-import must not stamp `generated`"
        );
    }

    /// 导入页(roam-import)不签 `generated`,照样落在 Human 档 —— 搬运不是撰写,
    /// 但内容确实是人在别处写的。两条路都通:`.note.md` 的后缀由规则 1 直接兜住,
    /// 后缀之外则由规则 4 的 type 映射兜住。断言分开写,免得一条掩盖另一条。
    #[test]
    fn imported_pages_reach_human_without_any_stamp() {
        let page = fm("type: Wiki Page\ntitle: 回顾系统\ncreated: 2026-08-02T00:00:00.000Z");
        // 后缀命中规则 1(与 frontmatter 无关),这是 roam 导入页的实际形态。
        assert_eq!(derive("wikipage/回顾系统.note.md", Some(&page), &globs(&[])), Origin::Human);
        // 去掉后缀后规则 1 不再触发,靠规则 4 的 `Wiki Page` → Human 映射。
        assert_eq!(derive("wikipage/回顾系统.md", Some(&page), &globs(&[])), Origin::Human);
    }
```

- [ ] **Step 2: 跑测试**

Run: `cd searchidx && cargo test origin::tests`
Expected: PASS —— 全部通过。**这三条应当一次就绿**:读侧本来就支持,
本次只是把行为钉住。若有红,说明前面某个任务改动了不该改的东西,回去查。

- [ ] **Step 3: 提交**

```bash
git add searchidx/src/origin.rs
git commit -m "test(searchidx): 钉住人/机/无主三态可分,与不签清单的滑档红线"
```

---

### Task 9: 把「人也要签」写进公共约定与文档

`src-tauri/templates/AGENTS.md` 是每个 vault 都会拿到的公共约定,外部 agent 照它抄。
今天它只对 AI 单向要求。约定要变成双向的,不然「`human:` 是人机分界线」这句话
在 vault 里依然无从验证。

**Files:**
- Modify: `src-tauri/templates/AGENTS.md:86-92`
- Modify: `docs/okf-v0.2-conformance-audit.md:0`(整改进度表加第 4 步)
- Modify: `docs/plugin-v2-development.md:331`(§9.1)
- Modify: `CHANGELOG.md`、`CHANGELOG.zh-CN.md`(未发布区)

- [ ] **Step 1: AGENTS.md 补人侧义务**

在 `src-tauri/templates/AGENTS.md` 的 **Never sign your own work `human:`** 那段之后,
加一段:

```markdown
The reverse duty is note.md's, not yours, but it is worth knowing it holds:
every document a person creates through the app — a new note, a quick note, a
companion `.note.md`, a daily note, a wiki page, an idea, a trace request — is
written with `generated: { by: human:<id>, at }`. So the three states are
distinguishable rather than guessed at: a `human:` actor means a person wrote
it, a `<producer>/<version>` actor means a generator did, and no `generated`
key at all means nobody has claimed it. Do not strip or rewrite a `human:`
stamp you find; it is the one signal that cannot be regenerated.
```

- [ ] **Step 2: 审计文档记进度**

在 `docs/okf-v0.2-conformance-audit.md` 的「0. 整改进度」里,第 3 步表格之后加:

```markdown
**第 4 步(F4/F6 的人侧)已完成**(2026-08-20):

| 项 | 落点 |
|----|------|
| 人写署名 | 9 个人工创建入口写 `generated: { by: human:<id>, at }`;身份走既有 `humanActorId` |
| 热路径不阻塞 | `src/lib/okf/identity.ts` 的 `warmHumanActor()` + 同步 `humanActorNow()` |
| 插件取身份 | `host.vault.info` 增 `author` 字段(纯增,复用 `vault.read` 门禁) |
| 只签不回填 | 保存路径(`store.svelte.ts`)不传 `generated`,由测试钉住 |
| 三态可分 | `searchidx/src/origin.rs` 的测试钉住人/机/无主三态,并给「不签清单」立红线 |
| 顺手补的硬伤 | 富文本 `[[链接]]` 建页不再写 0 字节文件,改走 `newPageFileText` |
| 公共约定 | `src-tauri/templates/AGENTS.md` 补人侧义务,约定从单向变双向 |

设计见 `docs/superpowers/specs/2026-08-20-human-authorship-signature-design.md`。
```

- [ ] **Step 3: 插件开发规范补一行**

`docs/plugin-v2-development.md` §9.1 的「唯一生产入口」那段之后加:

```markdown
**人工创建的文档必须签名**:插件里凡是「用户亲手敲进去的原始稿」(idea 原文、
委托稿一类),写盘时必须带 `generated: { by: <host.vault.info 的 author>, at }`。
`author` 是完整 OKF actor 串(`human:<id>`),宿主给不出时(老版本)**不签**,
绝不自己编一个。机器产出的文档反过来:签 `<producer>/<version>`,
**永远不得**自签 `human:`。
```

- [ ] **Step 4: 双语 CHANGELOG(硬门禁)**

`CHANGELOG.zh-CN.md` 的「## 未发布」→「### 新增」区顶部加:

```markdown
- **你亲手写的东西,现在文件里就写着是你写的。** 新建文档、快速笔记、手记 `.note.md`、日期笔记、wikilink 建页、奇思妙想的 idea 原文、溯源的委托稿 —— 这些由你一个动作直接产生的文件,元信息里多了一行署名(`generated: by: human:<你的 git 身份>`)。此前「是人写的」只能靠「没有 AI 署名」反推,一份忘了盖章的 AI 产物和你的手稿在元数据上分不开;现在三种状态各说各话:人写的、机器写的、没人认领的。署名只在**创建**时写一次,打开旧笔记再保存不会凭空长出署名;从别处导入的内容(Roam、电子书)也不签 —— 搬运不是撰写。换到 Obsidian 或 grep 里一样读得出来。
```

`CHANGELOG.zh-CN.md` 的「### 修复」区加:

```markdown
- **富文本里点一个还不存在的 `[[链接]]`,不再建出一个空壳文件。** 它此前写的是 0 字节文件:没有标题、没有元信息,格式校验会报错,检索里也排在最末。现在和大纲面板走同一条建页路径,建出来就是一份合规文档。
```

`CHANGELOG.md` 的 `## Unreleased` → `### Added` 顶部加:

```markdown
- **What you wrote by hand now says so, in the file.** New documents, quick notes, companion `.note.md` files, daily notes, pages created from a `[[wikilink]]`, Idea Spark originals and Trace Source requests — anything produced by a direct action of yours — now carry a signature in their metadata (`generated: by: human:<your git identity>`). Until now "a person wrote this" could only be inferred from the *absence* of an AI stamp, which made an unstamped AI dump indistinguishable from your own draft. Three states, each saying its own name: written by a person, written by a generator, claimed by nobody. The signature is written once, at creation — reopening and saving an old note will not grow one — and imported content (Roam, ebooks) stays unsigned, because carrying something across is not writing it. It reads the same in Obsidian, or in grep.
```

`CHANGELOG.md` 的 `### Fixed` 区加:

```markdown
- **Clicking a `[[wikilink]]` that doesn't exist yet, in rich text, no longer creates an empty shell.** It used to write a 0-byte file: no title, no metadata, failing the format check and ranked last in search. It now takes the same page-creation path as the outline panel, and what lands is a well-formed document.
```

- [ ] **Step 5: 门禁自检**

Run: `node scripts/changelog.mjs check`
Expected: 通过(两份未发布区都非空)

- [ ] **Step 6: 全量回归**

```bash
pnpm test && pnpm check
cd src-tauri && cargo test && cd -
cd searchidx && cargo test && cd -
```
Expected: 全绿

- [ ] **Step 7: 端到端自检(spec §6 的验收)**

用 `pnpm okf:lint` 扫一个临时 vault,确认新建产物零违规:

```bash
pnpm okf:lint /tmp/notemd-signature-check
```
Expected: 零违规。(该目录由下面的 GUI 验证步骤产生。)

- [ ] **Step 8: 提交**

```bash
git add src-tauri/templates/AGENTS.md docs/okf-v0.2-conformance-audit.md docs/plugin-v2-development.md CHANGELOG.md CHANGELOG.zh-CN.md
git commit -m "docs(okf): 公共约定补人侧署名义务;审计进度与双语 CHANGELOG"
```

---

## GUI 实机验证(合并前必做,由用户执行)

本次改动落在**新建文档**这条最热的交互路径上,单测挡不住「⌘N 之后编辑器里
多出两行 YAML 看着别扭」「快速笔记的空草稿被署名撑成非空、`skipEmptySave` 失效」
这类问题。按项目惯例(`feedback_no_ui_automation_user_tests`),GUI 由用户手测。

起 dev 构建后,逐条走:

1. ⌘N 新建 → 保存 → 文件头应有 `generated: by: human:<你>`;
2. 托盘快速笔记 → **不输入任何内容就关掉** → 确认**没有**留下文件
   (`shouldSkipEmptySave` 的「去掉 frontmatter 后无正文即空」判定必须仍然成立);
3. 富文本模式敲 `[[一个新名字]]` → 点它 → 新文件应有 H1 + frontmatter + 署名,
   **不是**空白页;
4. 在正文旁开手记 `.note.md` → 写一句 → 保存 → 应有署名;
5. **打开一份既有的旧 `.note.md`,改一个字保存 → 确认没有长出 `generated`**;
6. 奇思妙想写一条 idea 保存 → 应有署名;溯源委托一次 → `00-request.md` 应有署名;
7. `pnpm okf:lint <vault>` 零违规。

---

## 不在本次范围(遗留项)

- decision-log 的裁决签 `verified: [{ by: human:<id>, at }]`(审计 F9;
  那是**人工确认**语义,与本次的**创建**语义不同)
- insights 报告 / decision-log 看板补机器侧 `generated`(审计 F6)
- `agent-run-core` 的 `stamped()` 遇已有 frontmatter 就放弃补 `generated`
  (`plugins-src/agent-run-core/src/okf.rs:36-38`)—— agent 自己写了 `type`
  但忘了 `generated` 时兜底不生效
- 存量文件批量迁移 —— 用户 2026-08-04 已决定不做
