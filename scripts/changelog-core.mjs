// CHANGELOG 的解析、门禁校验、发版轮转与正文提取。
//
// 背景:GitHub Release 的正文过去是 `gh api releases/generate-notes` 从提交自
// 动生成的 —— 那是提交流水账,不是变更说明。它告诉你合并了哪 41 个 commit,
// 不告诉你「这个版本对我有什么不同」。所以改成人写的增量 CHANGELOG,并由
// `release.sh` 在没写时直接拦住发布(设计见 docs/superpowers/specs/
// 2026-08-17-changelog-gate-design.md)。
//
// 逻辑放在这里而不是 bash 里:多行文本处理是这类脚本最容易出错、最难测的部
// 分,而这条路径上一旦出错的代价是「发版被莫名其妙卡住」或者更糟——「正文
// 取错了段落,发出去的说明是上一个版本的」。

/** 「未发布」区的标题,两种语言各一。区分大小写,与文件里逐字一致。 */
export const UNRELEASED_HEADINGS = { en: '## Unreleased', zh: '## 未发布' }

/** 版本节标题:`## v6.817.1 — 2026-08-17`。破折号是 U+2014,不是连字符。 */
const VERSION_HEADING_RE = /^## v(\d+\.\d+\.\d+) — (\d{4}-\d{2}-\d{2})\s*$/

export class ChangelogError extends Error {}

/**
 * 把一份 CHANGELOG 切成「前言 / 未发布区 / 各版本节」。
 *
 * 只认顶层 `## ` 标题作为切分点,`### ` 类别标题留在各节内部。
 */
export function parse(text, lang) {
  const unreleasedHeading = UNRELEASED_HEADINGS[lang]
  if (!unreleasedHeading) throw new ChangelogError(`unknown lang: ${lang}`)

  const lines = text.split('\n')
  const headingIdx = lines.findIndex((l) => l.trimEnd() === unreleasedHeading)
  if (headingIdx < 0) {
    throw new ChangelogError(`missing "${unreleasedHeading}" heading`)
  }

  // 未发布区 = 它的标题到下一个顶层 `## ` 之间。
  let nextIdx = lines.length
  for (let i = headingIdx + 1; i < lines.length; i++) {
    if (lines[i].startsWith('## ')) {
      nextIdx = i
      break
    }
  }

  const versions = []
  for (let i = nextIdx; i < lines.length; i++) {
    const m = VERSION_HEADING_RE.exec(lines[i])
    if (m) versions.push({ version: m[1], date: m[2], line: i })
  }

  return {
    preamble: lines.slice(0, headingIdx).join('\n'),
    unreleasedBody: lines.slice(headingIdx + 1, nextIdx).join('\n'),
    rest: lines.slice(nextIdx).join('\n'),
    versions,
  }
}

/**
 * 「未发布」区里有没有真内容。
 *
 * 空行和 `<!-- -->` 注释不算内容 —— 否则轮转留下的空区会被自己当成「已写」,
 * 门禁就永远放行了。
 */
export function hasUnreleasedContent(text, lang) {
  const { unreleasedBody } = parse(text, lang)
  return unreleasedBody
    .split('\n')
    .some((l) => l.trim() !== '' && !l.trim().startsWith('<!--'))
}

/**
 * 门禁:两份都要有「未发布」内容,且版本序列逐一对应。
 *
 * 返回问题清单(空数组 = 放行)。**不抛异常**:调用方要一次看全所有问题,
 * 而不是修一个撞一个。
 */
export function checkGate(enText, zhText) {
  const problems = []

  let en, zh
  try {
    en = parse(enText, 'en')
  } catch (e) {
    problems.push(`CHANGELOG.md: ${e.message}`)
  }
  try {
    zh = parse(zhText, 'zh')
  } catch (e) {
    problems.push(`CHANGELOG.zh-CN.md: ${e.message}`)
  }
  if (!en || !zh) return problems

  if (!hasUnreleasedContent(enText, 'en')) {
    problems.push('CHANGELOG.md 的 "## Unreleased" 区是空的 —— 写完这一版的变更再发')
  }
  if (!hasUnreleasedContent(zhText, 'zh')) {
    problems.push('CHANGELOG.zh-CN.md 的「## 未发布」区是空的 —— 写完这一版的变更再发')
  }

  // 漂移校验。双语两份的老问题是漏改一边(本项目已经因这类漂移出过事:
  // SEARCH_SECTION 与 templates/AGENTS.md,靠 drift 钉子测试才逮到)。逐一
  // 比对版本标题序列挡得住「只更新了一边」;挡不住「两边都写了但内容不对
  // 应」——那只能靠人审,是双语方案自带的代价。
  const seq = (v) => v.versions.map((x) => `${x.version}@${x.date}`)
  const a = seq(en)
  const b = seq(zh)
  if (a.length !== b.length || a.some((x, i) => x !== b[i])) {
    problems.push(
      `两份 CHANGELOG 的版本序列不一致(漏改了一边?)\n` +
        `  CHANGELOG.md:       ${a.join(', ') || '(无)'}\n` +
        `  CHANGELOG.zh-CN.md: ${b.join(', ') || '(无)'}`,
    )
  }

  return problems
}

/**
 * 发版轮转:把「未发布」区就地变成版本节,并在顶部补一个新的空「未发布」区。
 *
 * 已有的旧版本节一个字都不动。
 */
export function rotate(text, lang, version, date) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new ChangelogError(`bad version: ${version}`)
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) {
    throw new ChangelogError(`bad date: ${date}`)
  }
  const { preamble, unreleasedBody, rest } = parse(text, lang)
  const heading = UNRELEASED_HEADINGS[lang]

  // 未发布区的首尾空行归轮转后的版本节所有,别把它们攒进新空区里。
  const body = unreleasedBody.replace(/^\n+/, '').replace(/\n+$/, '')
  const parts = [
    preamble,
    heading,
    '',
    `## v${version} — ${date}`,
    '',
    body,
    '',
    rest.replace(/\n+$/, ''),
  ]
  return parts.join('\n').replace(/\n{3,}/g, '\n\n').replace(/\n*$/, '\n')
}

/**
 * 取出某个版本那一节的正文(不含它自己的标题),供 Release 页正文使用。
 *
 * 找不到就抛 —— 静默返回空字符串会让 Release 页只剩安装说明,而那看起来
 * 「像是正常的」,没人会发现正文丢了。
 */
export function sectionFor(text, lang, version) {
  const lines = text.split('\n')
  let start = -1
  for (let i = 0; i < lines.length; i++) {
    const m = VERSION_HEADING_RE.exec(lines[i])
    if (m && m[1] === version) {
      start = i
      break
    }
  }
  if (start < 0) throw new ChangelogError(`no section for v${version}`)

  let end = lines.length
  for (let i = start + 1; i < lines.length; i++) {
    if (lines[i].startsWith('## ')) {
      end = i
      break
    }
  }
  return lines
    .slice(start + 1, end)
    .join('\n')
    .replace(/^\n+/, '')
    .replace(/\n+$/, '')
}
