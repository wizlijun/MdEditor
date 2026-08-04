// src/lib/outline/create.ts
import { touchFrontmatter } from './frontmatter'
import { pageNameOf } from './backlinks'
import { CONCEPT_TYPE, conceptFileText, isReservedConceptName } from '../okf/concept'

/** 新大纲文件的完整文本:front-matter + 单个空节点(空大纲)。
 *  type 缺省由 touchFrontmatter 填 Outline Note(OKF §4.1)。 */
export function newOutlineFileText(title: string, now?: string, type?: string): string {
  const fm = touchFrontmatter(null, { title, now, type })
  return `---\n${fm}\n---\n- \n`
}

/** 新普通页(vault 外解析 wikilink 时建的 `.md`)的完整文本。
 *  [[index]] / [[log]] 会落到保留文件名上:这类文件 **MUST NOT** 是概念文档
 *  (§8/§9),所以只写正文,不盖 frontmatter —— 文件名保持用户看到的样子。 */
export function newPageFileText(title: string): string {
  const body = `# ${title}\n`
  if (isReservedConceptName(`${title}.md`)) return body
  return conceptFileText({ type: CONCEPT_TYPE.note, title }, body)
}

/** 确保 .note.md 存在(不存在则以空大纲创建)。title 缺省取文件名;
 *  wikipage 建页传原始标题(spec §5:文件名 slug 化、fm title 存原文)。 */
export async function ensureOutlineFile(path: string, title?: string, type?: string): Promise<string> {
  const { exists, writeTextFile } = await import('@tauri-apps/plugin-fs')
  if (!(await exists(path).catch(() => false))) {
    await writeTextFile(path, newOutlineFileText(title ?? pageNameOf(path), undefined, type))
  }
  return path
}
