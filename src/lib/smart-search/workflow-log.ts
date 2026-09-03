export type WorkflowStage = 'plan' | 'search' | 'tune' | 'freeze' | 'memory' | 'answer' | 'document'
export type WorkflowLevel = 'active' | 'success' | 'warning' | 'error'

export interface WorkflowEntry {
  id: number
  stage: WorkflowStage
  level: WorkflowLevel
  message: string
  runId?: string
  steps?: number
}

export const WORKFLOW_LOG_LIMIT = 200

export function appendWorkflowEntry(
  entries: WorkflowEntry[],
  entry: WorkflowEntry,
): WorkflowEntry[] {
  const duplicate = entry.runId && entry.steps !== undefined
    ? entries.some((existing) => (
        existing.runId === entry.runId
        && existing.steps === entry.steps
        && existing.message === entry.message
      ))
    : false
  if (duplicate) return entries
  const next = [...entries, entry]
  return next.length > WORKFLOW_LOG_LIMIT ? next.slice(-WORKFLOW_LOG_LIMIT) : next
}

export function isNearLogBottom(
  metrics: Pick<HTMLElement, 'scrollTop' | 'clientHeight' | 'scrollHeight'>,
  threshold = 24,
): boolean {
  return metrics.scrollHeight - metrics.scrollTop - metrics.clientHeight <= threshold
}
