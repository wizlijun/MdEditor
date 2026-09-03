import {
  searchApi,
  type SearchHit,
  type SearchOptions,
  type SmartSearchQuery,
  type SmartSearchResponse,
} from '../search/api'
import { isCancelled } from '../search/store.svelte'

export interface SmartSearchRunOptions extends SearchOptions {}

/**
 * Per-webview search state. The main sidebar keeps its own singleton; this
 * instance belongs only to the global search window, so one surface never
 * overwrites or cancels the other's visible state.
 */
export class SmartSearchStore {
  query = $state('')
  hits = $state<SearchHit[]>([])
  route = $state<string | null>(null)
  tookMs = $state(0)
  total = $state(0)
  loading = $state(false)
  error = $state<string | null>(null)
  truncated = $state(false)
  deepAvailable = $state(false)
  extractedTerms = $state<string[]>([])
  subqueries = $state<SmartSearchQuery[]>([])
  private sequence = 0

  constructor(
    private readonly search: typeof searchApi.smart = searchApi.smart,
  ) {}

  async run(query: string, options: SmartSearchRunOptions = {}): Promise<SmartSearchResponse | null> {
    this.query = query
    if (!query.trim()) {
      this.clear()
      return null
    }
    const mine = ++this.sequence
    this.loading = true
    this.error = null
    try {
      const response = await this.search(query, options)
      if (mine !== this.sequence) return null
      this.hits = response.hits
      this.route = response.route
      this.tookMs = response.tookMs
      this.total = response.total
      this.truncated = response.truncated === true
      this.deepAvailable = response.deepAvailable === true
      this.extractedTerms = response.extractedTerms ?? []
      this.subqueries = response.subqueries ?? []
      return response
    } catch (error) {
      if (mine !== this.sequence || isCancelled(error)) return null
      this.error = error instanceof Error ? error.message : String(error)
      this.hits = []
      this.deepAvailable = false
      this.truncated = false
      return null
    } finally {
      if (mine === this.sequence) this.loading = false
    }
  }

  /** Replace the live preview with the host-validated authoritative result. */
  apply(query: string, response: SmartSearchResponse): void {
    this.sequence++
    this.query = query
    this.hits = response.hits
    this.route = response.route
    this.tookMs = response.tookMs
    this.total = response.total
    this.loading = false
    this.error = null
    this.truncated = response.truncated === true
    this.deepAvailable = response.deepAvailable === true
    this.extractedTerms = response.extractedTerms ?? []
    this.subqueries = response.subqueries ?? []
  }

  clear(): void {
    this.sequence++
    this.query = ''
    this.hits = []
    this.route = null
    this.tookMs = 0
    this.total = 0
    this.loading = false
    this.error = null
    this.truncated = false
    this.deepAvailable = false
    this.extractedTerms = []
    this.subqueries = []
  }
}
