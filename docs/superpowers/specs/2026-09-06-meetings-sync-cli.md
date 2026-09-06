# 会议插件单命令增量同步

日期：2026-09-06

## 命令契约

新增 `notemd meetings-sync [source] [--user ID] [--timezone IANA] [--dry-run]`，通过原生插件 CLI 执行，不要求打开会议窗口或文档。

- `source` 可省略，沿用现有 `~/.hemory/vault` 首个有效 Vault 探测；显式来源接受 Hemory 根、用户根或 conversation(s) 目录。
- 固定使用 `MigrationMode::Incremental`，不提供 `--full`；原 `meetings-import-hemory` 及其全量参数继续兼容。
- 目标为当前 note.md Vault；归档目录读取该 Vault 的 `.notemd/meetings.json`，缺省为 `ssot/meetings`。
- 与 UI、旧导入命令共用 `MigrationService`、来源身份、ledger、锁和逐会议原子写入。首次同步导入全部合格会议；重复执行跳过未变记录；来源新增或变更时创建/更新，目标本地改动只报告冲突；来源删除不删除归档。
- `--dry-run` 仅预检，不能写归档、ledger 或来源绑定。多用户来源要求 `--user`，历史无 offset 时间要求 `--timezone`，不猜用户或时区。
- 普通 stdout 输出包含新增、更新、跳过、冲突和受阻计数的完整 JSON 报告，非零退出时 stderr 额外显示计数摘要。宿主全局 `--json` 包装完整 `MigrationReport` 为 `{ok,data}`；干净完成退出 0，冲突/受阻退出 4，执行错误沿用宿主错误契约。

## 实现与验证

修改会议插件的 manifest、backend CLI 适配、测试和使用说明，不增加新的同步引擎。宿主 `CliRunner` 目前把任意 path 参数当作文档读取，阻断目录来源；修正为 `requires_tab_context: false` 时跳过文档构建，现有 PDF 文件命令继续读取并渲染文档，不增加会议插件特例。宿主把原 manifest 中缺省的该字段归一为 false；需要文档内容的第三方 CLI 应声明 true。

先用 manifest 契约验证 CLI 可发现及参数，再从编译后的插件 JSON-RPC 入口验证默认来源、首次同步、幂等重跑、源变更、新增会议、目标冲突、dry-run、自定义目录及旧命令兼容；运行现有迁移核心测试和插件前端检查/构建。
