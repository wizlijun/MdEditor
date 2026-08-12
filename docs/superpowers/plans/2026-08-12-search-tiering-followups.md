# 检索索引:合并后的遗留清单(2026-08-12)

> 两个项目已合入 main(`bbc2753`):
> - A —— 索引与搜索设置页(`docs/superpowers/plans/2026-08-11-search-index-settings.md`)
> - B —— md 分级与检索优先级(`docs/superpowers/plans/2026-08-11-md-origin-tiering.md`)
>
> 本文只记**被评审发现、经裁决后没有当场修**的东西,以及尚未进行的人工验证。
> 每条都写了「为什么当时不修」,因为那个理由决定了它现在值不值得修。
>
> 落这份文档的原因:上述判断原本只存在于 gitignored 的 SDD ledger 里,
> 而那个 worktree 已经关闭。

## 1. 尚未进行的人工验证(优先级最高)

这三份清单**一条都没跑过**,而合并后的搜索面板同时包含两个会话各自改的 UI,
从未有人看它跑起来。

- **B 计划 GUI 8 条** —— 见 `2026-08-11-md-origin-tiering.md` 末尾「人工 GUI 验收清单」
- **A 计划 GUI 4 条** —— 见 `2026-08-11-search-index-settings-design.md` §8「人工(GUI)」。
  其中「重建期间查询被正确禁用」**已作废**:终审把两个搜索命令改成了异步,
  重建期间应当能查、不卡,而不是被禁用。
- **Windows 9 条** —— 项目 0 的 CLI shim 与路径契约,需在 Windows 发布机上跑。
  清单在 `vault-search-index` worktree 的 `.superpowers/` 里(gitignored)。

升级后**首次启动会重建一次索引**(`SCHEMA_VERSION` 1→2,约 10 秒,不丢数据)。
这是设计路径,不是故障;写 release note 时值得单独提一句。

## 2. 已知会被用户撞见的行为(均已裁决,非缺陷)

- **没有 frontmatter 的人工笔记会被判成「原始资料」吃 ×0.9**,可能排在 AI 摘要之后。
  规则 6 的刻意误判,理由见 tiering spec §3.2;修正手段是给文件加 frontmatter,不是改权重。
- **中间层分组标题是英文原始类型名**(`BOOK SUMMARY`、`ANSWER`),中日德界面下混排。
  经用户拍板照 spec §5 保留。若日后要改,做法是给注册类型加 i18n 键、原始串兜底,
  这样 spec 里「插件加类型不改代码就自动多一组」的承诺仍然成立。
- **改 `syncDir` 会触发一次完整重建**(约 10 秒搜索不可用)。盖章设计的必然代价,
  只有真正改了值才付。

## 3. 遗留的小项(按性价比排序)

**值得顺手修:**

- `src/lib/sotvault.svelte.ts` —— `vaultRootChangedHandler` 在**每次** `refreshSotvault()`
  都触发,没有比较根路径,而 `App.svelte` 把 `indexStatus.reset()` 挂在上面。
  约 8 个非切库调用点(手记同步、提问捕获、设置保存)都会命中;若此时搜索 tab 开着,
  分层表会掉回 `—` 并**停在那**(tab 入口 effect 因 `selectedTab` 未变而不重跑)。
  两行守卫即可。B 计划扩大了它的影响面 —— 现在掉的是 spec §9 指定的那块发现面板。
- `src-tauri/src/search/mod.rs` —— `notemd_vault_settings_set` 现在收 8 个同类型相邻的
  位置参数,编译器挡不住调换。改成 options struct。
- `src/components/side-panel/SearchPanel.svelte` 的 `--help` / `AGENTS.md` 一致性:
  `src-tauri/src/cli/builtin.rs` 的 `--help` 文本没有任何测试钉住,可以和
  `agents_sync` 的 `SEARCH_SECTION` 漂移。今天两者一致。

**可以一直放着:**

- `searchidx/src/origin.rs` 的 sync_dir 匹配是**大小写与分隔符字面**的
  (`Sync/a.md` 不匹配 `sync`)。写入侧 `validate_rel_dir` 已规范化,最坏是一层判错,
  改回目录名即自愈。
- `origin.rs` 与 `scan.rs` 的目录前缀匹配器逐字重复三行,两处各有自己的负向测试。
- `src-tauri/src/search/mod.rs` 两处测试字面量改用了 `..Default::default()`,
  丢掉了「新增 `ScanOptions` 字段必须逐点确认」的编译期守卫 —— 而正是那个编译错误
  在本次暴露了这两处。下次动那两行时顺手恢复。
- `src/lib/i18n/en.ts` 的 `search.group.count` 仍是裸 `{n}`,而 zh/ja/de 都带了单位。
- `notemd search --stats --json` 的键是 snake_case(`origin_counts`/`type_counts`),
  与该 payload 其余键一致;camelCase 属于 GUI 的 DTO。这是有意的,不要「统一」。

## 4. 必须保持的不变量(改这块代码前先读)

- **`SELECT_COLS` 按位置消费**:`f.origin` 在 10、`f.concept_type` 在 11、
  `fts_search` 的 `rank` 在 12、`is_annotation` 在 9。位移到另一个 TEXT 列是
  **完全静默**的 —— 不报类型错,经未知值兜底解析,整个分级退化成 no-op 而测试全绿。
  本次开发中这个陷阱被三个不同的评审各抓到一次。唯一的防线是
  `a_hits_origin_round_trips_through_the_real_index` 和
  `a_hits_concept_type_round_trips_through_the_real_index` 这两条端到端测试,
  **不要弱化它们**:排序测试和 50 条回归集在列位移下都保持绿。
- **`origin::derive` 的 `Some(&Frontmatter::default())` 不等于 `None`**。
  调用方若已把「没有 frontmatter」塌缩成「空 frontmatter」,规则 6 就永不命中,
  所有无 frontmatter 的文件会判成 `Derived` 而非 `Source`,方向整个反过来。
- **`notemd search` 默认输出必须逐字保持 `path:line:text`**,agent 按行解析它。
  分组是纯 UI 的事。
- **`retrievability.json` 的期望值不得照着新输出批量刷新**,每条变更须单独裁决。
- **`sync_dir` 不匹配走原地重建,绝不 unlink** —— GUI 可能正握着活的 WAL 连接。
