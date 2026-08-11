import { invoke } from '@tauri-apps/api/core'

export interface SearchHit {
  path: string
  absPath: string
  line: number
  lineEnd: number
  text: string
  breadcrumb: string
  level: 'file' | 'section' | 'line'
  score: number
  docDate: string | null
  sourceRef: string
  agentBy: string | null
  humanVerified: boolean
}

export interface SearchResponse {
  route: string
  tookMs: number
  total: number
  hits: SearchHit[]
}

export interface SearchStats {
  files: number
  blocks: number
  dbBytes: number
  builtAt: string | null
  tokenizerId: string
}

export const searchApi = {
  query: (query: string, limit = 50) =>
    invoke<SearchResponse>('notemd_search', { query, limit }),
  stats: () => invoke<SearchStats>('notemd_search_stats'),
  rebuild: () => invoke<SearchStats>('notemd_search_rebuild'),
}
