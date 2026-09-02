import { describe, expect, it } from 'vitest'
import {
  classificationProposalIsValid,
  cloneClassificationProposal,
  updateClassificationAssignment,
  type TopicClassificationProposal,
} from './topic-classification'

const topics = [
  { id: 'engineering', label: 'Engineering', description: 'Build', index_file: 'e.index.md', vocabulary: [] },
  { id: 'business', label: 'Business', description: 'Compete', index_file: 'b.index.md', vocabulary: [] },
]

const proposal: TopicClassificationProposal = {
  schema_version: 1,
  inventory_sha256: 'abc',
  catalog_revision: 'sha256:def',
  assignments: [
    { book: '2026-08/A', topic_id: 'engineering' },
    { book: '2026-08/B', topic_id: 'business' },
  ],
}

describe('topic classification draft', () => {
  it('clones and updates without mutating the Agent proposal', () => {
    const cloned = cloneClassificationProposal(proposal)
    const updated = updateClassificationAssignment(cloned, '2026-08/A', 'business')
    expect(updated.assignments[0].topic_id).toBe('business')
    expect(cloned.assignments[0].topic_id).toBe('engineering')
    expect(proposal.assignments[0].topic_id).toBe('engineering')
  })

  it('requires an exact book set and known topics', () => {
    const books = ['2026-08/A', '2026-08/B']
    expect(classificationProposalIsValid(proposal, books, topics)).toBe(true)
    expect(classificationProposalIsValid({ ...proposal, assignments: proposal.assignments.slice(0, 1) }, books, topics)).toBe(false)
    expect(classificationProposalIsValid({ ...proposal, assignments: [...proposal.assignments, proposal.assignments[0]] }, books, topics)).toBe(false)
    expect(classificationProposalIsValid({ ...proposal, assignments: [{ book: '2026-08/C', topic_id: 'business' }, proposal.assignments[1]] }, books, topics)).toBe(false)
    expect(classificationProposalIsValid({ ...proposal, assignments: [{ ...proposal.assignments[0], topic_id: 'unknown' }, proposal.assignments[1]] }, books, topics)).toBe(false)
  })
})
