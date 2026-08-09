// src/lib/outline/rename-pair.ts
import { sanitizeFileName } from './slug'
// NOT '../folder-view.svelte': that module imports @tauri-apps/plugin-fs and
// @tauri-apps/plugin-store at the top level, which would break pure vitest
// runs. `paths.ts` is dependency-free and re-exports the same `dirname`.
import { joinPath, dirname as dirOf } from '../paths'

export interface RenameOp { from: string; to: string }
export interface RenamePlan { ops: RenameOp[] }

const NOTE_SUFFIX_RE = /\.notes?\.md$/i

/**
 * 重命名计划(纯,spec §7):
 * - 主文档 xxx.md 改名 → 同目录配对伴生(.note.md/.notes.md,保留各自后缀)联动;
 * - .note.md 自身改名只改自身;
 * - newName 经 sanitizeFileName;同名 no-op、目标冲突(大小写不敏感,排除自身)→ null。
 * siblings 为该目录现有文件名列表(含被改文件)。
 */
export function planRename(oldPath: string, newNameRaw: string, siblings: string[]): RenamePlan | null {
  const dir = dirOf(oldPath)
  const oldName = dir === '/' ? oldPath.slice(1) : oldPath.slice(dir.length + 1)
  const newName = sanitizeFileName(newNameRaw)
  if (newName === oldName) return null

  const lowerSiblings = new Set(siblings.map(s => s.toLowerCase()))
  const conflicts = (name: string, self: string) =>
    name.toLowerCase() !== self.toLowerCase() && lowerSiblings.has(name.toLowerCase())
  if (conflicts(newName, oldName)) return null

  const ops: RenameOp[] = [{ from: oldPath, to: joinPath(dir, newName) }]

  // 主文档改名 → 伴生联动(仅 .md → .md 的改名;伴生自身改名不反向联动)
  const isMain = /\.md$/i.test(oldName) && !NOTE_SUFFIX_RE.test(oldName)
  const newIsMd = /\.md$/i.test(newName) && !NOTE_SUFFIX_RE.test(newName)

  // 硬性规则(spec §7):有伴生配对的主文档不允许改掉 .md 扩展——
  // 否则伴生无法联动而成孤儿;拒绝整个操作(UI 弹冲突提示)。
  const hasCompanion = ['.note.md', '.notes.md'].some(suffix =>
    siblings.some(s => s.toLowerCase() === oldName.replace(/\.md$/i, suffix).toLowerCase()))
  if (isMain && hasCompanion && !newIsMd) return null

  if (isMain && newIsMd) {
    const base = oldName.replace(/\.md$/i, '')
    const newBase = newName.replace(/\.md$/i, '')
    for (const suffix of ['.note.md', '.notes.md']) {
      const compName = siblings.find(s => s.toLowerCase() === (base + suffix).toLowerCase())
      if (compName) {
        const compNew = newBase + suffix
        if (conflicts(compNew, compName)) return null   // 伴生目标冲突 → 整体中止
        ops.push({ from: joinPath(dir, compName), to: joinPath(dir, compNew) })
      }
    }
  }
  return { ops }
}

/**
 * 执行计划:依次 rename;任何一步失败则把已完成的 op 逆序回滚(尽力而为的
 * 原子性,spec §7)。成功返回 null,失败返回错误信息。IO 薄层,手动验证。
 */
export async function executeRename(plan: RenamePlan): Promise<string | null> {
  const { rename } = await import('@tauri-apps/plugin-fs')
  const done: RenameOp[] = []
  for (const op of plan.ops) {
    try {
      await rename(op.from, op.to)
      done.push(op)
    } catch (e) {
      for (const u of done.reverse()) {
        await rename(u.to, u.from).catch(() => {})   // 回滚失败只能尽力
      }
      return String(e)
    }
  }
  return null
}
