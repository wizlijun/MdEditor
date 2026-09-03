export type MigrationMode = 'incremental' | 'full'

export interface MeetingSummary {
  conversation_id: string
  title?: string | null
  started_at: string
  ended_at?: string | null
  duration_ms?: number | null
  speaker_count?: number | null
  source?: string | null
  source_system?: string | null
  transcript_path?: string | null
  transcript_relative_path?: string | null
  target_relative_path?: string | null
  path?: string | null
}

export interface HemoryUser {
  id: string
  label: string
}

export interface HemoryDetection {
  source?: string
  users: Array<string | HemoryUser>
  selected_user?: string | null
  needs_timezone?: boolean
  timezone?: string | null
  warnings?: string[]
}

export type MigrationAction =
  | 'create'
  | 'update'
  | 'skip'
  | 'conflict'
  | 'blocked'
  | 'excluded'
  | 'source_missing'

export interface MigrationOutputHashes {
  transcript?: string
  summary?: string
  meta?: string
}

export interface MigrationItem {
  conversation_id: string
  source_relative_path: string
  source_schema?: string
  selected_transcript?: string | null
  source_fingerprint?: string | null
  target_relative_path?: string | null
  action: MigrationAction
  reason?: string | null
  output_hashes?: MigrationOutputHashes | null
  warnings?: string[]
}

export interface MigrationReport {
  schema_version: number
  mode: MigrationMode
  dry_run: boolean
  source_user?: string | null
  scanned: number
  eligible: number
  create: number
  update: number
  skip: number
  conflict: number
  blocked: number
  excluded_audio: number
  committed: number
  source_missing: number
  warnings: string[]
  errors: string[]
  items: MigrationItem[]
}

export interface MigrationProgressPush {
  type: 'hemory-migration'
  job_id: number
  event: 'progress' | 'done' | 'failed' | 'cancelled'
  committed?: number
  total?: number
  item?: MigrationItem
  report?: MigrationReport
  error?: string
}
