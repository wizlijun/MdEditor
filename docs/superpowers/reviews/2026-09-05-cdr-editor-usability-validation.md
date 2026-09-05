# CDR 编辑可用性重构验收

日期：2026-09-05。实现分支：`fix/cdr-editor-complete`，基于 `fe6eb227`（Host 6.905.5 / Memory 2.5.1）。

## 结论与范围

本轮完成 [主 spec](../specs/2026-09-04-governed-collaborative-document-runtime-design.md) 的 Stage 1B 本机编辑范围，并按 [编辑可用性补充契约](../specs/2026-09-05-cdr-editor-usability.md) 验证。代码及浏览器生产 Kit 验收通过，尚未发布或更新用户已安装的应用。

通用 Core、Editor Kit 和 Memory Profile 的边界保留。没有新增 Claim 写入权限，没有把工作正文自动发布为 Agent context，也没有实现 Projection、Publication、Annotation、跨设备 Yjs 或完整 MEMORY MVP。

## 修复的根因

- 原结构门禁将合法的跨块键盘事务直接拒绝。现在统一转换成有序原子 OperationBatch，支持插入、删除、移动、跨块替换和全文清空；末尾保留可编辑空块。
- 原保存串行化连带锁住输入。现在分离已提交版本、唯一在途请求和当前编辑稿；保存回执只合并对应部分，不覆盖后续输入或正在进行的 composition。
- Markdown 往返会吞掉空段／文末硬换行，并引入未编辑块的格式差异。内部稳定身份、空段映射和原文／规范化比较共同避免身份丢失与虚假未保存状态。
- 原 live-preview 插件会在光标进入链接时改写正文。仅 CDR 实例使用真实 mark，保留 Editor Kit v1 原有体验。
- 原位置式撤销在远端移动后可能失效或误定位。继续使用唯一 ProseMirror history，由稳定块 ID 和精确正文前置条件决定 inverse；组内任一步冲突就拒绝整组，不消费半笔历史。
- 失败稿原先缺少完整恢复入口。现在可比较、复制、下载、显式重试或确认放弃；重开恢复的本地副本不冒充已保存。

## 验证结果

| 检查 | 结果 |
| --- | --- |
| Host 全量 Vitest | 263 个文件，2,786 项通过 |
| Memory 全量 Vitest | 12 个文件，156 项通过 |
| Host Svelte / TypeScript | 0 error，43 个既有 warning |
| Memory Svelte / TypeScript | 0 error，0 warning |
| Host 与 Memory 生产构建 | 通过；Editor Kit 入口、API 标记与依赖资源检查通过 |
| 宿主插件协议生成检查 | 通过，生成物无漂移 |
| Chromium 源码矩阵 | 27 / 27 通过 |
| Chromium 生产 Kit 矩阵 | 27 / 27 通过；未捕获 pageerror 为 0，入口／资源 HTTP 错误为 0 |
| 独立 Core 与 UI 终审 | 未发现新增 P0 / P1；已更新主 spec 的过时阶段说明 |

浏览器版本为 Chromium `140.0.7339.16`。矩阵使用真实键盘、系统剪贴板和 contenteditable 行为，覆盖 Enter／Shift+Enter、连续空段、复制／剪切／多段粘贴、跨块替换、全选清空后继续输入、保存期间输入、IME composition、移动后的撤销／重做、格式／列表／表格、历史恢复、失败重试及重开。实际 Memory 界面还覆盖 Agent 提案采纳、链接表单的目标版本检查和侧栏可达性。

生产模式加载 `dist/assets/editor-kit-v2.js` 及产物 CSS，没有源码 Kit CSS 兜底。Memory UI 则由 Vite 编译真实 Svelte 组件；Application、Session 和 ManagedStore 为真实实现，Host CAS 存储与 Agent 终态使用内存测试替身。它不是插件 ZIP 在原生窗口中的端到端测试。

全量 Vitest 退出码为 0；既有 v1 DOM 测试仍可能输出 happy-dom 样式请求／销毁阶段噪声，不能将这些日志描述为完全无警告。独立生产浏览器检查没有资源请求错误。

## 复现入口

```sh
node node_modules/vitest/vitest.mjs run
node node_modules/svelte-check/bin/svelte-check --tsconfig ./tsconfig.json
node node_modules/vite/bin/vite.js build
node scripts/check-editor-kit-build.mjs

# 使用已安装的 Playwright；不为验收修改项目依赖。
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node scripts/check-cdr-browser.mjs
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs CDR_BROWSER_BUILT=1 node scripts/check-cdr-browser.mjs
```

Memory 的 `test`、`check`、`build` 在 `plugins-src/memory` 单独执行。

## 交付与剩余门禁

1. 本次没有更改正式版本号、打 tag、推送或发布插件。正式交付必须递增 Host 和 Memory 版本，并提升插件 minimum host；不能将中间构建覆盖既有 Memory 2.5.1 包。插件另有 `documentEditorApiVersion === 2` 运行时检查，旧宿主会明确提示升级。
2. session v2–v5 严格迁移至 v6；v6 写入后旧插件不支持原地降级。正式升级前应备份原始文档及对应 aggregate；回退须恢复匹配的数据与软件，不能只降插件版本。
3. 原生 macOS WKWebView、系统中文输入法候选窗口、原生剪贴板菜单和插件 ZIP 安装后的回归尚未执行。CDP composition 不等于真实中文输入法实机验收；发布前仍须用隔离测试 Vault 验证，不应为测试启动用户真实 Vault 的自动同步。
4. 同块正文冲突或后续结构冲突会安全拒绝撤销，保留历史；用户可使用明确的历史版本恢复入口。没有加入不安全的自动覆盖。
5. localStorage 恢复仅是尽力而为的本机副本；禁用或容量不足时不能承诺崩溃重开后仍有草稿。失败界面明确提示关闭前复制或下载，并由组件回归测试固定该提示。
6. 当前面向 Memory-first 小文档验证，尚未建立大文档／长历史的性能上界。既有外部文件最后检查与替换间的竞争窗口、完整故障注入和 MEMORY MVP 闸门保持原 spec 的限制。

实现位于独立 worktree，未覆盖原工作区的 Canvas、目录、Frontmatter 等并行未提交修改。
