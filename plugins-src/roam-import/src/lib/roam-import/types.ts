// src/lib/roam-import/types.ts
/** Roam JSON 导出的 block(递归)。未列出的键(text-align、emojis 等)忽略。 */
export interface RoamBlock {
  uid?: string
  string?: string
  heading?: number
  children?: RoamBlock[]
  'create-time'?: number
  'edit-time'?: number
}

/** Roam JSON 导出的页面。顶层就是 RoamPage[]。 */
export interface RoamPage {
  title: string
  uid?: string
  children?: RoamBlock[]
  'create-time'?: number
  'edit-time'?: number
}

export interface RoamGraph {
  pages: RoamPage[]
}

/** 增量清单,存 vault/.notemd/roam-import.json */
export interface ImportManifest {
  graphName: string
  importedAt: string
  pages: Record<string, { file: string; editTime: number; contentHash: string }>
}

/** 页面清单键:有 uid 用 uid,否则退回标题 */
export function pageKey(p: RoamPage): string {
  return p.uid ?? `t:${p.title}`
}
