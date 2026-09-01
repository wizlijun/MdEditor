# Ebook Import 主题分类与索引实施计划

参考设计：`docs/superpowers/specs/2026-09-01-ebook-topic-index-design.md`

## 成功标准

- 书库主题为 1–5 个；Agent proposal 为 2–5 个。
- 新 GUI/CLI 导入没有有效 `topic_id` 时在转换前失败。
- `topics.yml` 与每书 `meta.yml.topic_id` 是唯一权威数据，主题 index 可确定性重建。
- 旧书缺分类仍可见，可逐本或由 Agent proposal 批量补齐。
- Claude、Codex、DeepSeek 均能发现 `organize-ebook-topics` 独立任务。
- 专项、全量、OKF/search origin、协议、类型检查、构建与实机交互验证通过。

## 执行计划

1. [x] 后端主题领域层：schema、meta 读写、扫描、index projector、原子写与冲突保护。
2. [x] 导入链路：queue/RPC/CLI/pipeline 强制 topic，完成后 reconcile index。
3. [x] 书库链路：返回主题与 metadata，旧书诊断、主题筛选和逐书归类。
4. [x] 前端交互：主题卡、每行主题、管理 sheet、onboarding、i18n。
5. [x] Agent：inventory、模板播种、run/status、proposal 校验、预览和应用。
6. [x] OKF：登记 `Book Topic Index` 并同步 search origin derived mapping/fixture。
7. [x] 兼容与恢复：旧书、损坏 YAML、同名手写 index、并发和崩溃 reconcile。
8. [x] 验证：Rust/TS/检查/构建/协议/宿主回归。
9. [x] 复核 diff、更新版本和回顾，建立独立分支并提交，不发布。

## 当前约束

- 不触碰工作区已有的 Memory v2 改动。
- 不改变 `YYYY-MM/<书名>/` 物理目录。
- 不让 Agent 直接修改 canonical 主题、书籍 meta 或最终 index。

## 实施回顾

- `notemd.ebook-import` 升级为 `1.3.0`；书库 taxonomy、单书归属和生成索引分别落在
  `topics.yml`、`meta.yml.topic_id` 与 `<主题>.index.md`，物理目录保持不变。
- GUI 和 CLI 都在转换前强制有效主题；旧书继续可见，可逐书迁移或通过 2–5 主题的
  Agent proposal 一次补齐。主题管理与主流程均提供英、中、日、德文案。
- 所有 canonical 变更共用跨进程锁并以原子单文件写入；Agent apply 使用 durable journal，
  启动时验证原 inventory/proposal 后幂等恢复。
- 验证通过：Ebook 后端 147 项、前端 106 项、search origin 32 项、宿主 2337 项测试；
  插件/宿主类型检查、插件/宿主生产构建与协议一致性检查均通过。
