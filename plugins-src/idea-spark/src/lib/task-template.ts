// The `idea-proof` claude-agent task template, seeded verbatim into the vault
// on first delegation. See plugins-src/claude-agent/backend/templates/ for
// the analogous built-in templates (answer-note-question, selfcheck) — this
// plugin's task lives in the *vault* instead of compiled into the claude-agent
// binary, since claude-agent only auto-discovers `.notemd/agent-tasks/*`; it
// has no notion of "idea-spark's task" at compile time.
//
// Every file body below is built as an array of lines joined with '\n' rather
// than a single template literal. Two traps rule out backticks here:
//   1. CLAUDE.md's own markdown contains ``` code fences, which would
//      terminate a `...` template literal early (the ENTRY_MAP landing-page
//      bug this project has hit before).
//   2. Several files contain the literal text `${VAULT}` / `${NOTE}` —
//      claude-agent's own placeholder syntax, substituted by claude-agent at
//      run time, NOT by this file. Inside a real template literal `${VAULT}`
//      would be evaluated as a JS expression (ReferenceError: VAULT is not
//      defined) instead of staying as literal text.
// Array-of-lines sidesteps both: no backtick ever opens, and no `${` is ever
// parsed as interpolation.

/** The task id — also the vault-relative directory name under agent-tasks/. */
export const TASK_ID = 'idea-proof'

const BASE = `.notemd/agent-tasks/${TASK_ID}`

const TASK_JSON = [
  '{',
  '  "name": "Idea proof",',
  '  "description": "把一个模糊的 idea 论证成可下一步的文档:先找落差,再证伪,再最小验证。",',
  '  "prompt": "读取给定的 idea 文件,严格按 CLAUDE.md 的协议输出论证文档。产物只有一个:与 idea 同目录的 <同名>.proof.md。绝不修改 idea 原文。输出语言跟随 idea 原文语言。",',
  '  "max_turns": 80,',
  '  "timeout_seconds": 1800,',
  '  "precheck": "precheck.sh"',
  '}',
  '',
].join('\n')

const PRECHECK_SH = [
  '#!/bin/sh',
  '# idea 文件必须存在且非空,否则这次运行不值得花 token。',
  '[ -n "$NOTEMD_NOTE" ] || { echo "缺少 idea 文件参数"; exit 1; }',
  '[ -s "$NOTEMD_NOTE" ] || { echo "idea 文件不存在或为空:$NOTEMD_NOTE"; exit 1; }',
  'exit 0',
  '',
].join('\n')

const SETTINGS_JSON = [
  '{',
  '  "permissions": {',
  '    "allow": [',
  '      "Read(${VAULT}/**)",',
  '      "Write(${VAULT}/**/*.proof.md)",',
  '      "Edit(${VAULT}/**/*.proof.md)",',
  '      "WebSearch",',
  '      "WebFetch"',
  '    ],',
  '    "deny": [ "Bash", "Task" ]',
  '  }',
  '}',
  '',
].join('\n')

const SETTINGS_SCOPED_JSON = [
  '{',
  '  "permissions": {',
  '    "allow": [',
  '      "Read(${NOTE})",',
  '      "Read(${VAULT}/**)",',
  '      "Write(${VAULT}/**/*.proof.md)",',
  '      "Edit(${VAULT}/**/*.proof.md)",',
  '      "WebSearch",',
  '      "WebFetch"',
  '    ],',
  '    "deny": [ "Bash", "Task" ]',
  '  }',
  '}',
  '',
].join('\n')

const CLAUDE_MD = [
  '# 任务:把 idea 论证成可下一步的文档',
  '',
  '你在 note.md 的 Claude Agent 插件里以 headless 模式运行,vault 根在 `${VAULT}`。',
  '输入是一个 idea 文件(环境变量 NOTEMD_NOTE 指向它):用户刚写下的一个模糊念头,',
  '可能含四个小节:领域/方向、可能迁移的场景、现有条件、期望成果——缺哪个就在文中',
  '标注「未提供」,不要臆造。',
  '',
  '你是一名严谨的研究顾问和论文审稿人。目标不是鼓励用户,而是帮 ta 用最低成本缩小',
  '未知空间。按以下流程输出:',
  '',
  '1. **先找落差,不要急着给方案**',
  '   - 结果落差:理论有效但实验/现实不稳定的地方。',
  '   - 迁移落差:原场景有效,换到新场景后前提失效的地方。',
  '   - 假设落差:大家默认成立、但现实中未必成立的前提。',
  '   每个落差写成一句可检验陈述:在【具体条件】下,【现有做法】因为【明确原因】,',
  '   无法稳定实现【目标】。',
  '2. **判断这个问题是否值得做**:能否证伪(什么结果会证明它不成立)、能否观测',
  '   (关键证据现在拿得到吗)、能否小规模验证(最小实验是什么)、失败是否有信息',
  '   (失败后能否区分:问题不存在/信号不足/基线够强/方法错了)、是否撞题',
  '   (已有工作是否覆盖;搜不到文献时检查是否只是术语不同或负结果藏在附录里)。',
  '3. **给出 3 个候选研究/产品验证点**,每个包括:可证伪陈述、关键证据和证据等级、',
  '   最接近的已有工作/竞品/实践、与已有工作的真实差异、最小验证动作、',
  '   如果结果为负还能学到什么。',
  '4. **先做反方审稿**:对最值得做的候选,先尝试否定它——是否可能只是测量误差、',
  '   样本偏差、指标选错、数据泄漏或基线过弱?有没有更简单的方法已经足够?',
  '   哪些模糊词必须改成可测指标?如果严重撞题或前提不成立,直接说',
  '   「不值得做」或「需要收窄」。',
  '5. **设计逐级验证门**(按顺序,不跳步):G0 现象是否真实存在;G1 所需信号是否',
  '   可观测;G2 机制是否优于简单基线和强基线;G3 接入完整流程后是否产生真实收益;',
  '   G4 是否能在不同条件下复现。每一关说明:验证命题、最小实验、对照组、通过标准、',
  '   否定标准、失败后的下一步。',
  '6. **最后输出**(文档必须以这个结构收束):直接判断(值得做/需要收窄/暂不值得做)、',
  '   最大未知、最先要做的一个验证动作、3 个候选点排序、逐级验证门、最低成立标准、',
  '   结论边界(在什么条件下,最多能声称什么)。',
  '',
  '要求:区分事实、已有结论、你的推断;不编造文献或证据,找不到证据就写',
  '「尚未找到证据」;不为了显得创新而回避简单强基线。',
  '',
  '## 产物(逐条遵守)',
  '',
  '1. **只写一个文件**:与 idea 同目录、同名去掉 `.md` 加 `.proof.md`',
  '   (例:`inbox/ideas/2026-08-04-foo.md` → `inbox/ideas/2026-08-04-foo.proof.md`)。',
  '2. 文件开头是 YAML frontmatter,`type` 必填:',
  '',
  '   ```',
  '   ---',
  '   type: Idea Proof',
  '   title: <一行结论,如「值得做:…」>',
  '   generated:',
  '     by: process:claude-agent',
  '     at: <ISO 8601 时间>',
  '   sources:',
  '     - resource: <idea 文件的 vault 相对路径>',
  '   ---',
  '   ```',
  '',
  '3. **绝不修改 idea 原文**,也不写其他任何文件。重跑即整体覆盖旧的 `.proof.md`。',
  '4. 输出语言跟随 idea 原文语言。',
  '',
].join('\n')

/** Vault-relative path → full file content for the `idea-proof` task template. */
export const TASK_FILES: Record<string, string> = {
  [`${BASE}/task.json`]: TASK_JSON,
  [`${BASE}/CLAUDE.md`]: CLAUDE_MD,
  [`${BASE}/precheck.sh`]: PRECHECK_SH,
  [`${BASE}/.claude/settings.json`]: SETTINGS_JSON,
  [`${BASE}/.claude/settings.scoped.json`]: SETTINGS_SCOPED_JSON,
}

/** Host I/O this module needs, injected so it stays unit-testable without the bridge. */
export interface SeedIo {
  exists(path: string): Promise<boolean>
  write(path: string, content: string): Promise<void>
}

/**
 * Seeds the `idea-proof` task template into the vault, one file at a time.
 * Idempotent and non-destructive: a file that already exists (including one
 * the user has edited) is left untouched — never overwritten.
 *
 * Note: this cannot set precheck.sh's executable bit (host.vault.write has no
 * chmod). See task-12-report.md for the consequence and mitigation.
 */
export async function seedTaskTemplate(
  io: SeedIo,
  files: Record<string, string> = TASK_FILES,
): Promise<void> {
  for (const [path, content] of Object.entries(files)) {
    if (await io.exists(path)) continue
    await io.write(path, content)
  }
}
