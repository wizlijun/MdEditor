// Pure topic taxonomy state shared by the import UI. Persistence and bridge
// calls deliberately live in App.svelte: this module only validates and
// returns new values, which keeps draft editing deterministic and testable.

export interface TopicVocabulary {
  term: string
  description: string
  [key: string]: unknown
}

export interface TopicDefinition {
  id: string
  label: string
  description: string
  index_file: string
  vocabulary: TopicVocabulary[]
  /** Preserve future backend/YAML fields while the current UI edits a topic. */
  [key: string]: unknown
}

export type TopicCounts = Record<string, number>

export type TopicValidationCode =
  | 'required'
  | 'too_few'
  | 'too_many'
  | 'invalid_id'
  | 'invalid_index_file'
  | 'duplicate_id'
  | 'duplicate_label'
  | 'duplicate_index_file'
  | 'duplicate_term'

export interface TopicValidationError {
  path: string
  code: TopicValidationCode
}

export interface TopicValidationResult {
  valid: boolean
  errors: TopicValidationError[]
}

const TOPIC_ID = /^[a-z0-9]+(?:-[a-z0-9]+)*$/

/** True only for one safe generated index filename in the ebooks root. */
export function isValidIndexFile(value: string): boolean {
  const name = value
  const lower = name.toLowerCase()
  return (
    name.length > '.index.md'.length &&
    name === name.trim() &&
    !/[\u0000-\u001f\u007f]/.test(name) &&
    lower.endsWith('.index.md') &&
    !name.includes('/') &&
    !name.includes('\\') &&
    !name.includes('..') &&
    lower !== 'index.md' &&
    lower !== 'log.md'
  )
}

/** Validate the UI/backend `topics` array using the topics.yml v1 rules. */
export function validateTopics(topics: TopicDefinition[]): TopicValidationResult {
  const errors: TopicValidationError[] = []
  if (topics.length < 1) errors.push({ path: 'topics', code: 'too_few' })
  if (topics.length > 5) errors.push({ path: 'topics', code: 'too_many' })

  const ids = new Set<string>()
  const labels = new Set<string>()
  const indexFiles = new Set<string>()

  topics.forEach((topic, topicIndex) => {
    const base = `topics.${topicIndex}`
    const id = topic.id
    const label = topic.label.trim()
    const indexFile = topic.index_file

    if (!TOPIC_ID.test(id)) errors.push({ path: `${base}.id`, code: 'invalid_id' })
    if (ids.has(id)) errors.push({ path: `${base}.id`, code: 'duplicate_id' })
    ids.add(id)

    if (!label) errors.push({ path: `${base}.label`, code: 'required' })
    if (labels.has(label)) errors.push({ path: `${base}.label`, code: 'duplicate_label' })
    labels.add(label)

    if (!topic.description.trim()) {
      errors.push({ path: `${base}.description`, code: 'required' })
    }

    if (!isValidIndexFile(indexFile)) {
      errors.push({ path: `${base}.index_file`, code: 'invalid_index_file' })
    }
    if (indexFiles.has(indexFile.toLowerCase())) {
      errors.push({ path: `${base}.index_file`, code: 'duplicate_index_file' })
    }
    indexFiles.add(indexFile.toLowerCase())

    if (topic.vocabulary.length < 2) {
      errors.push({ path: `${base}.vocabulary`, code: 'too_few' })
    }
    const terms = new Set<string>()
    topic.vocabulary.forEach((entry, entryIndex) => {
      const entryBase = `${base}.vocabulary.${entryIndex}`
      const term = entry.term.trim()
      if (!term) errors.push({ path: `${entryBase}.term`, code: 'required' })
      if (terms.has(term)) errors.push({ path: `${entryBase}.term`, code: 'duplicate_term' })
      terms.add(term)
      if (!entry.description.trim()) {
        errors.push({ path: `${entryBase}.description`, code: 'required' })
      }
    })
  })

  return { valid: errors.length === 0, errors }
}

/** Deep-enough copy for a manager draft; unknown future fields stay intact. */
export function cloneTopics(topics: TopicDefinition[]): TopicDefinition[] {
  return topics.map((topic) => ({
    ...topic,
    vocabulary: topic.vocabulary.map((entry) => ({ ...entry })),
  }))
}

/** Add one blank editable topic, or return the same list at the five-topic cap. */
export function createTopic(topics: TopicDefinition[]): TopicDefinition[] {
  if (topics.length >= 5) return topics
  const used = new Set(topics.map((topic) => topic.id))
  let suffix = topics.length + 1
  while (used.has(`topic-${suffix}`)) suffix += 1
  return [
    ...topics,
    {
      id: `topic-${suffix}`,
      label: '',
      description: '',
      index_file: '',
      vocabulary: [
        { term: '', description: '' },
        { term: '', description: '' },
      ],
    },
  ]
}

/** Replace an editable topic, but never change an existing stable id. */
export function updateTopic(
  topics: TopicDefinition[],
  id: string,
  next: TopicDefinition,
): TopicDefinition[] {
  const index = topics.findIndex((topic) => topic.id === id)
  if (index < 0) return topics
  const out = [...topics]
  out[index] = { ...next, id, vocabulary: next.vocabulary.map((entry) => ({ ...entry })) }
  return out
}

/** Move a topic one or more positions; an out-of-range move is a no-op. */
export function moveTopic(
  topics: TopicDefinition[],
  id: string,
  offset: number,
): TopicDefinition[] {
  const from = topics.findIndex((topic) => topic.id === id)
  const to = from + offset
  if (from < 0 || to < 0 || to >= topics.length || offset === 0) return topics
  const out = [...topics]
  const [item] = out.splice(from, 1)
  out.splice(to, 0, item)
  return out
}

export function removeTopic(topics: TopicDefinition[], id: string): TopicDefinition[] {
  const index = topics.findIndex((topic) => topic.id === id)
  if (index < 0) return topics
  return [...topics.slice(0, index), ...topics.slice(index + 1)]
}

export function topicCount(counts: TopicCounts, id: string): number {
  const count = counts[id]
  return Number.isFinite(count) && count > 0 ? count : 0
}
