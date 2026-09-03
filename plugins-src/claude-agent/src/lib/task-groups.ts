import type { Task } from './events'

export interface TaskGroup {
  id: string
  tasks: Task[]
  running: boolean
}

const LEGACY_SOURCE = new Map<string, string>([
  ['answer-note-question', 'notemd.core'],
  ['selfcheck', 'agent-tools'],
  ['ai-read-ebook', 'notemd.ebook-import'],
  ['classify-unclassified-ebooks-v1', 'notemd.ebook-import'],
  ['organize-ebook-topics', 'notemd.ebook-import'],
  ['organize-ebook-topics-v2', 'notemd.ebook-import'],
  ['organize-ebook-topics-v3', 'notemd.ebook-import'],
  ['organize-ebook-topics-v4', 'notemd.ebook-import'],
  ['idea-proof', 'notemd.idea-spark'],
  ['memory-inference', 'notemd.memory'],
  ['trace-source', 'notemd.trace-source'],
])

const ORDER = [
  'notemd.core',
  'notemd.ebook-import',
  'notemd.idea-spark',
  'notemd.memory',
  'notemd.trace-source',
  'agent-tools',
  'custom',
]

export function taskGroupId(task: Task): string {
  return task.source_plugin?.trim() || LEGACY_SOURCE.get(task.id) || 'custom'
}

export function groupTasks(tasks: Task[]): TaskGroup[] {
  const groups = new Map<string, Task[]>()
  for (const task of tasks) {
    const id = taskGroupId(task)
    groups.set(id, [...(groups.get(id) ?? []), task])
  }
  return [...groups]
    .map(([id, grouped]) => ({ id, tasks: grouped, running: grouped.some((task) => task.running) }))
    .sort((a, b) => {
      const ai = ORDER.indexOf(a.id)
      const bi = ORDER.indexOf(b.id)
      return (ai < 0 ? ORDER.length : ai) - (bi < 0 ? ORDER.length : bi) || a.id.localeCompare(b.id)
    })
}
