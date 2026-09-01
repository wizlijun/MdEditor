import { describe, expect, it } from 'vitest'
import {
  createTopic,
  moveTopic,
  removeTopic,
  stageTopicRemoval,
  topicCount,
  updateTopic,
  validateTopics,
  type TopicDefinition,
} from './topics'

const topic = (id: string, label = id): TopicDefinition => ({
  id,
  label,
  description: `${label} books`,
  index_file: `${label}.index.md`,
  vocabulary: [
    { term: `${label} one`, description: 'First related concept' },
    { term: `${label} two`, description: 'Second related concept' },
  ],
})

describe('validateTopics', () => {
  it('accepts one to five complete, unique topics', () => {
    const result = validateTopics([topic('business', 'Business'), topic('software', 'Software')])
    expect(result.valid).toBe(true)
    expect(result.errors).toEqual([])
  })

  it('rejects an empty taxonomy and more than five topics', () => {
    expect(validateTopics([]).errors).toContainEqual({ path: 'topics', code: 'too_few' })
    expect(validateTopics(Array.from({ length: 6 }, (_, i) => topic(`topic-${i}`))).errors).toContainEqual(
      { path: 'topics', code: 'too_many' },
    )
  })

  it('validates stable ids and safe generated index names', () => {
    const bad = topic('Bad Id')
    bad.index_file = '../index.md'
    const result = validateTopics([bad])
    expect(result.errors).toContainEqual({ path: 'topics.0.id', code: 'invalid_id' })
    expect(result.errors).toContainEqual({ path: 'topics.0.index_file', code: 'invalid_index_file' })
  })

  it('does not silently trim ids or generated file names the backend will reject', () => {
    const bad = topic(' business', 'Business')
    bad.index_file = 'Business.index.md '
    const result = validateTopics([bad])
    expect(result.errors).toContainEqual({ path: 'topics.0.id', code: 'invalid_id' })
    expect(result.errors).toContainEqual({ path: 'topics.0.index_file', code: 'invalid_index_file' })
  })

  it('detects duplicate ids, labels, index files and vocabulary terms', () => {
    const first = topic('business', 'Business')
    first.vocabulary[1].term = first.vocabulary[0].term
    const second = topic('business', 'Business')
    const result = validateTopics([first, second])
    expect(result.errors.map((e) => e.code)).toEqual(
      expect.arrayContaining([
        'duplicate_id',
        'duplicate_label',
        'duplicate_index_file',
        'duplicate_term',
      ]),
    )
  })

  it('requires descriptions and at least two fully described vocabulary entries', () => {
    const bad = topic('business', 'Business')
    bad.description = '  '
    bad.vocabulary = [{ term: 'Strategy', description: '' }]
    const result = validateTopics([bad])
    expect(result.errors).toContainEqual({ path: 'topics.0.description', code: 'required' })
    expect(result.errors).toContainEqual({ path: 'topics.0.vocabulary', code: 'too_few' })
    expect(result.errors).toContainEqual({
      path: 'topics.0.vocabulary.0.description',
      code: 'required',
    })
  })
})

describe('topic reducers', () => {
  it('creates a minimal editable topic without mutating the existing list', () => {
    const before = [topic('business', 'Business')]
    const after = createTopic(before)
    expect(after).not.toBe(before)
    expect(after[0]).toBe(before[0])
    expect(after[1]).toMatchObject({ id: 'topic-2', label: '', index_file: '' })
    expect(after[1].vocabulary).toHaveLength(2)
  })

  it('does not create a sixth topic', () => {
    const five = Array.from({ length: 5 }, (_, i) => topic(`topic-${i + 1}`))
    expect(createTopic(five)).toBe(five)
  })

  it('updates fields while preserving an existing topic id', () => {
    const before = [topic('business', 'Business')]
    const after = updateTopic(before, 'business', {
      id: 'renamed-id',
      label: '商业战略',
      description: '企业竞争优势',
      index_file: '商业战略.index.md',
      vocabulary: before[0].vocabulary,
    })
    expect(after[0]).toMatchObject({ id: 'business', label: '商业战略' })
    expect(before[0].label).toBe('Business')
  })

  it('moves and removes topics immutably', () => {
    const before = [topic('a'), topic('b'), topic('c')]
    expect(moveTopic(before, 'b', -1).map((t) => t.id)).toEqual(['b', 'a', 'c'])
    expect(moveTopic(before, 'b', 1).map((t) => t.id)).toEqual(['a', 'c', 'b'])
    expect(moveTopic(before, 'a', -1)).toBe(before)
    expect(removeTopic(before, 'b').map((t) => t.id)).toEqual(['a', 'c'])
  })

  it('reads missing topic counts as zero', () => {
    expect(topicCount({ business: 3 }, 'business')).toBe(3)
    expect(topicCount({}, 'software')).toBe(0)
  })

  it('stages deletion and migration without mutating the persisted inputs', () => {
    const before = [topic('business'), topic('software')]
    const migrations = { business: 'software' }
    const staged = stageTopicRemoval(before, ['business', 'software'], { business: 2 }, migrations, 'business')
    expect(staged.removed).toBe(true)
    expect(staged.topics.map((entry) => entry.id)).toEqual(['software'])
    expect(staged.migrations).toEqual({ business: 'software' })
    expect(before.map((entry) => entry.id)).toEqual(['business', 'software'])
    expect(migrations).toEqual({ business: 'software' })
  })

  it('does not stage an in-use topic deletion without a migration', () => {
    const before = [topic('business'), topic('software')]
    const staged = stageTopicRemoval(before, ['business', 'software'], { business: 1 }, {}, 'business')
    expect(staged).toEqual({ topics: before, migrations: {}, removed: false })
  })

  it('retargets earlier staged migrations when their destination is deleted', () => {
    const before = [topic('software'), topic('history')]
    const staged = stageTopicRemoval(
      before,
      ['business', 'software', 'history'],
      { business: 2, software: 1 },
      { business: 'software', software: 'history' },
      'software',
    )
    expect(staged.topics.map((entry) => entry.id)).toEqual(['history'])
    expect(staged.migrations).toEqual({ business: 'history', software: 'history' })
  })
})
