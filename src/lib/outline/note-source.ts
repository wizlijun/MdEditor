// 伴生笔记的来源(OKF §5.1)。
//
// 镜像文件(vault/sync/*.md)是 vault 外某个文件的快照,镜像本身保持逐字快照
// 不塞元数据(信念 4:批注属于 vault,不属于路径),所以「这份笔记是就哪个源文件
// 写的」落在**伴生 .note.md** 的 frontmatter 里。
import type { ConceptMeta } from '../okf/concept'

const NOTE_SUFFIX = /\.notes?\.md$/i

/** `X.note.md` / `X.notes.md` → `X.md`;不是伴生名则返回 null。 */
export function mainPathOfNote(notePath: string): string | null {
  return NOTE_SUFFIX.test(notePath) ? notePath.replace(NOTE_SUFFIX, '.md') : null
}

/**
 * 该伴生笔记应写入的 `sources`:文档是本机记录过的镜像时,取它的源路径;
 * 否则 undefined(不写这个键)。
 */
export function sourcesForNote(
  notePath: string,
  resolveMirrorSource: (mainPath: string) => string | null,
): ConceptMeta['sources'] | undefined {
  const main = mainPathOfNote(notePath)
  if (!main) return undefined
  const resource = resolveMirrorSource(main)
  return resource ? [{ resource }] : undefined
}
