import { describe, expect, it } from 'vitest'
import {
  appendWorkflowEntry,
  isNearLogBottom,
  WORKFLOW_LOG_LIMIT,
  type WorkflowEntry,
} from './workflow-log'

describe('smart-search workflow log', () => {
  it('keeps phase history and removes repeated provider snapshots', () => {
    let entries: WorkflowEntry[] = []
    entries = appendWorkflowEntry(entries, { id: 1, stage: 'plan', level: 'active', message: '理解问题' })
    entries = appendWorkflowEntry(entries, {
      id: 2, stage: 'plan', level: 'active', message: 'building plan', runId: 'p1', steps: 1,
    })
    const beforeDuplicate = entries
    const same = appendWorkflowEntry(entries, {
      id: 3, stage: 'plan', level: 'active', message: 'building plan', runId: 'p1', steps: 1,
    })
    entries = appendWorkflowEntry(same, { id: 4, stage: 'search', level: 'success', message: '找到 3 条' })

    expect(same).toBe(beforeDuplicate)
    expect(entries.map((entry) => entry.message)).toEqual(['理解问题', 'building plan', '找到 3 条'])
  })

  it('bounds runaway logs and keeps the newest terminal entry', () => {
    let entries: WorkflowEntry[] = []
    for (let id = 1; id <= WORKFLOW_LOG_LIMIT + 5; id += 1) {
      entries = appendWorkflowEntry(entries, {
        id, stage: 'summary', level: id === WORKFLOW_LOG_LIMIT + 5 ? 'success' : 'active',
        message: `step ${id}`,
      })
    }
    expect(entries).toHaveLength(WORKFLOW_LOG_LIMIT)
    expect(entries.at(-1)).toMatchObject({ message: `step ${WORKFLOW_LOG_LIMIT + 5}`, level: 'success' })
  })

  it('only follows the tail while the user is near the bottom', () => {
    expect(isNearLogBottom({ scrollTop: 176, clientHeight: 100, scrollHeight: 300 } as HTMLElement)).toBe(true)
    expect(isNearLogBottom({ scrollTop: 120, clientHeight: 100, scrollHeight: 300 } as HTMLElement)).toBe(false)
  })
})
