# 会议记录插件

从 Hemory 将会议逐字稿、摘要和元数据单向归档到 note.md Vault。

## 命令行增量同步

升级到 note.md 6.906.2 或更新版本，在插件市场安装/更新并启用会议插件 1.0.3，配置好 Vault 后，执行：

```sh
notemd meetings-sync
```

无需打开会议插件窗口。省略来源时，会探测 `~/.hemory/vault` 下按名称排序的首个有效 Hemory Vault；目标使用当前 note.md Vault 的会议目录（默认 `ssot/meetings`，可在插件设置中修改）。

也可以在一条命令中指定来源、用户和历史时间的时区：

```sh
notemd meetings-sync /path/to/hemory/vault --user alice --timezone Asia/Taipei
```

来源参数支持 Hemory 根、用户根或 `conversation(s)` 目录。检测到多个用户时必须指定 `--user`；历史时间缺少时区偏移时需指定 IANA `--timezone`。

```sh
# 仅预览，不写入归档或同步状态
notemd meetings-sync --dry-run

# 输出完整 JSON 报告，供脚本读取
notemd --json meetings-sync
```

首次同步导入全部合格会议；之后只写入新增或变化的会议，未变记录保持原文件和修改时间。目标存在本地修改时报告冲突并保留内容；Hemory 删除会议不会删除已归档副本。不复制音频。

普通 stdout 输出完整 JSON 报告，包含新增、更新、跳过、冲突、受阻及实际提交数量；冲突/受阻时 stderr 额外显示计数摘要。干净完成退出 `0`，冲突、受阻或插件执行失败退出 `4`；参数错误由宿主返回 `2`。`--json` 输出 `{ "ok": true/false, "data": ... }`，冲突/受阻时仍保留完整报告；执行失败则返回 `error`。

原有 `notemd meetings-import-hemory [source]` 命令继续支持，默认同样增量执行；需要全量重新校验时可使用其 `--full` 参数。两个入口与 UI 共用同一同步记录，重复调用不会重复归档。
