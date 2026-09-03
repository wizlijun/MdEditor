import { describe, expect, it } from 'vitest'
import type { Task } from './events'
import { groupTasks, taskGroupId } from './task-groups'
const task = (id: string, source_plugin?: string, running = false): Task => ({ id, name: id, description: '', running, source_plugin })
describe('task plugin groups', () => {
  it('uses explicit ownership, legacy ids, and custom fallback', () => {
    expect(taskGroupId(task('idea-proof'))).toBe('notemd.idea-spark')
    expect(taskGroupId(task('x', 'third.party'))).toBe('third.party')
    expect(taskGroupId(task('mine'))).toBe('custom')
  })
  it('orders groups and aggregates running state', () => {
    const groups = groupTasks([task('mine'), task('trace-source', undefined, true), task('ai-read-ebook')])
    expect(groups.map((group) => group.id)).toEqual(['notemd.ebook-import', 'notemd.trace-source', 'custom'])
    expect(groups[1].running).toBe(true)
  })
})
