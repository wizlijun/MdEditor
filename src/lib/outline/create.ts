// src/lib/outline/create.ts
import { touchFrontmatter } from './frontmatter'
import { pageNameOf } from './backlinks'
import { CONCEPT_TYPE, conceptFileText, isReservedConceptName, type ConceptMeta } from '../okf/concept'

/** 新大纲文件的完整文本:front-matter + 单个空节点(空大纲)。
 *  type 缺省由 touchFrontmatter 填 Outline Note(OKF §4.1)。
 *  `author` 在场时写 §5.2 的 `generated` —— 只有这条创建路径传,保存路径不传。 */
export function newOutlineFileText(
  title: string, now?: string, type?: string, author?: ConceptMeta['generated'],
): string {
  const fm = touchFrontmatter(null, { title, now, type, generated: author })
  return `---\n${fm}\n---\n- \n`
}

/** 新普通页(解析 wikilink 时建的 `.md`)的完整文本。
 *  [[index]] / [[log]] 会落到保留文件名上:这类文件 **MUST NOT** 是概念文档
 *  (§8/§9),所以只写正文,不盖 frontmatter —— 文件名保持用户看到的样子,
 *  署名也一并不写(没有 frontmatter 可挂)。 */
export function newPageFileText(title: string, author?: ConceptMeta['generated']): string {
  const body = `# ${title}\n`
  if (isReservedConceptName(`${title}.md`)) return body
  return conceptFileText({ type: CONCEPT_TYPE.note, title, generated: author }, body)
}

/** 确保 .note.md 存在(不存在则以空大纲创建)。title 缺省取文件名;
 *  wikipage 建页传原始标题(spec §5:文件名 slug 化、fm title 存原文)。
 *  新建的文件签人写署名 —— 手记/日记/wikipage 都是你的一个动作直接产生的。 */
export async function ensureOutlineFile(path: string, title?: string, type?: string): Promise<string> {
  const { exists, writeTextFile } = await import('@tauri-apps/plugin-fs')
  if (!(await exists(path).catch(() => false))) {
    const { humanActor } = await import('../okf/identity')
    const by = await humanActor().catch(() => null)
    const author = by ? { by, at: new Date().toISOString() } : undefined
    await writeTextFile(path, newOutlineFileText(title ?? pageNameOf(path), undefined, type, author))
  }
  return path
}
