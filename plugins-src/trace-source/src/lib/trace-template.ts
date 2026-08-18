// trace-source 任务模板:溯源。
//
// 每个文件体都用「数组拼行 + join('\n')」而不是模板字符串,两个坑逼出来的:
//   1. CLAUDE.md 自身含 ``` 围栏,会提前终结 `...` 模板字面量;
//   2. 多处含字面量 `${VAULT}` / `${NOTE}`——那是 claude-agent 运行时替换的
//      占位符,真模板字面量里会被 JS 求值成 ReferenceError。
//
// 协议键(委托文本里的字段名)是语言中立的英文:`Source-Doc:` / `Output:`。
// 宿主右键预填(src/lib/context-menu/trace-action.ts)与本插件的委托调用
// (delegate.ts)写的就是这两个键,三处必须一致。
export const TRACE_TASK_ID = 'trace-source'

/** Bumped when the seeded protocol changes shape. Seeding compares this
 *  against the on-disk task.json: an older/absent marker means the whole
 *  template is rewritten once (migration); a current one means existing files
 *  — including user-edited prompts — are never touched. */
export const TEMPLATE_VERSION = 2

const BASE = `.notemd/agent-tasks/${TRACE_TASK_ID}`

const TASK_JSON = [
  '{',
  '  "name": "溯源",',
  '  "description": "为一段话找到原始出处(YouTube/论文/博客),下载字幕或正文,生成带反向链接的摘要。",',
  '  "prompt": "按 CLAUDE.md 的协议为委托文本中的引文溯源。产物只写 Output 指定的报告文件及其同名材料目录。",',
  '  "max_turns": 100,',
  '  "timeout_seconds": 2700,',
  '  "precheck": "precheck.sh",',
  '  "okf_type": "Trace Report",',
  `  "template_version": ${TEMPLATE_VERSION}`,
  '}',
  '',
].join('\n')

const PRECHECK_SH = [
  '#!/bin/sh',
  '# vault 必须可写,否则这次运行注定白跑。yt-dlp 缺失不拦——协议内降级。',
  '# 报告目录由委托文本的 Output 行指定(用户可在设置里改),写入时自建父目录。',
  '[ -n "$NOTEMD_VAULT" ] || { echo "缺少 vault 参数"; exit 1; }',
  '[ -w "$NOTEMD_VAULT" ] || { echo "vault 不可写"; exit 1; }',
  'exit 0',
  '',
].join('\n')

// 报告目录是用户可改的设置,所以写权限按**文件名约定**圈定而不是钉死目录——
// 与 idea-proof 的 `**/*.proof.md` 同一手法:目录随设置换,可写面不变。
const SETTINGS_ALLOW = [
  '      "Read(${VAULT}/**)",',
  '      "Write(${VAULT}/**/*-source-trace.md)",',
  '      "Edit(${VAULT}/**/*-source-trace.md)",',
  '      "Write(${VAULT}/**/*-source-trace/**)",',
  '      "Edit(${VAULT}/**/*-source-trace/**)",',
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
  '委托文本的结构(字段名固定为英文键,不随界面语言变化):',
  '',
  '- `> ` 引用块 = 待溯源的原文(用户从某篇文档里选出来的一段话)。',
  '- `Source-Doc: <路径>` 行 = 这段话所在的文档,摘要要反向链接回它。可能缺失。',
  '- `Output: <路径>` 行 = 摘要文件的落点(vault 相对路径),**不得改名**。',
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
  '4. 每份取到的全文材料写一个文件,放进**与摘要同目录、同名去 `.md` 的子目录**:',
  '   `<摘要同名去 .md>/<nn>-<来源短名>.md`(例:输出是',
  '   `inbox/traces/2026-08-18-143012-source-trace.md`,材料就在',
  '   `inbox/traces/2026-08-18-143012-source-trace/01-karpathy-blog.md`)。',
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
  '5. 摘要写到 `Output:` 指定的路径,结构:',
  '   - frontmatter:`type: Trace Report`、`title`(一行主题)、`generated:',
  '     { by: process:trace-source, at: <ISO 8601 时间> }`、`sources:` 列出全部',
  '     核验过的出处 URL。',
  '   - `## 缘起`:原样引用待溯源引文(引用块),下一行给出源文档链接——',
  '     `Source-Doc:` 是 vault 内相对路径时写 `[<文件名>](<相对路径>)` 形式的 markdown',
  '     链接,vault 外或缺失时原样写路径纯文本。',
  '   - `## 结论`:最可能的原始出处,标可信度(确认/高度疑似/未找到);每条断言旁',
  '     直接标 URL。',
  '   - `## 摘要`:按用户关注点组织的内容提炼——不是来源的复述,而是回答「这段话',
  '     的原始语境是什么、作者真正的主张是什么、与引文的出入在哪」。',
  '   - `## 继续阅读`:逐条列出材料全文的**相对链接**,如',
  '     `[Karpathy 博客正文](2026-08-18-143012-source-trace/01-karpathy-blog.md)`。',
  '6. 找不到出处也要产出摘要:声明未找到,列出已排查的候选与排除理由。',
  '',
  '## 红线',
  '',
  '- 只写 `Output:` 指定的摘要文件与其同名材料子目录(两者都以 `-source-trace`',
  '  命名收尾),绝不修改 vault 里的其他任何文件,绝不动源文档。',
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

/** Host I/O this module needs, injected so it stays unit-testable without the bridge. */
export interface SeedIo {
  exists(path: string): Promise<boolean>
  read(path: string): Promise<string>
  write(path: string, content: string): Promise<void>
}

/** The on-disk task.json's protocol marker; 0 for absent/unreadable/broken —
 *  i.e. a pre-versioning template that predates the marker. */
async function onDiskVersion(io: SeedIo, taskPath: string): Promise<number> {
  if (!(await io.exists(taskPath))) return 0
  try {
    const v = (JSON.parse(await io.read(taskPath)) as { template_version?: unknown }).template_version
    return typeof v === 'number' && Number.isFinite(v) ? v : 0
  } catch {
    return 0
  }
}

/**
 * Seeds the task template into the vault, one file at a time.
 *
 * Two regimes, decided by the on-disk task.json's `template_version`:
 *   - older than {@link TEMPLATE_VERSION} (or absent/broken) → the protocol
 *     changed shape underneath this template, so ALL five files are rewritten
 *     once — an old template would deny the new output paths outright (its
 *     write allowlist named a directory the reports no longer live in).
 *   - current (or newer) → non-destructive: a file that already exists,
 *     including a user-edited prompt, is never overwritten; only missing
 *     files are filled in.
 *
 * Note: this cannot set precheck.sh's executable bit (host.vault.write has no
 * chmod); claude-agent runs prechecks through `sh` so the bit is not needed.
 */
export async function seedTraceTemplate(io: SeedIo): Promise<void> {
  const taskPath = `${BASE}/task.json`
  const migrate = (await onDiskVersion(io, taskPath)) < TEMPLATE_VERSION
  for (const [path, content] of Object.entries(TRACE_TASK_FILES)) {
    if (!migrate && (await io.exists(path))) continue
    await io.write(path, content)
  }
}
