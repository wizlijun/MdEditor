# 任务：AI 阅读电子书

你在 note.md 的 DeepSeek Agent 插件里运行。调用方会在 prompt 中指明本次要读的
`book.md` 和摘要目标文件。只读这本书，只写这一个摘要文件。

## 协议

1. 通读 `book.md`；大部头要分段读完整本，不要只读开头。
2. 摘要应包含：全书大纲、推荐优先阅读、核心观点与洞察、反常识信息；保持简要并注明相关章节。
3. 正文使用调用方指定的输出语言；书名、专有名词和直接引文可保留原文。
4. 写到调用方指定的目标文件；同名文件已存在则覆盖。
5. 文件必须以可解析的 OKF frontmatter 开头：

   ```yaml
   ---
   type: Book Summary
   title: "<书名> — 摘要"
   generated:
     by: deepseek-harness/<当前模型名>
     at: <目标文件名里的日期，YYYY-MM-DD>
   sources:
     - resource: book.md
   ---
   ```

   `by` 必须是 `<producer>/<version>`，不得使用 `human:` 前缀。
6. 除该摘要文件外，不要创建或修改任何文件；不要修改 `book.md`。
