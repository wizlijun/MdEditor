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
  '   (例:输出是 `traces/2026-08-18-143012.md`,材料就在',
  '   `traces/2026-08-18-143012/01-karpathy-blog.md`)。材料 frontmatter 必写:',
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
  '     { by: process:trace-source, at: <ISO 8601 时间> }`、`sources:` 列出全部',
  '     核验过的出处 URL。',
  '   - `## 缘起`:原样引用待溯源引文(引用块),下一行给出源文档链接——',
  '     `源文档:` 是 vault 内相对路径时写 `[<文件名>](<相对路径>)` 形式的 markdown',
  '     链接,vault 外或缺失时原样写路径纯文本。',
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
