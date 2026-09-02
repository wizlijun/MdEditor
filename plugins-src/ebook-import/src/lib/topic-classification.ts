import type { TopicDefinition } from './topics'

export interface TopicAssignment {
  book: string
  topic_id: string
}

export interface TopicClassificationProposal {
  schema_version: number
  inventory_sha256: string
  catalog_revision: string
  assignments: TopicAssignment[]
}

export function cloneClassificationProposal(
  proposal: TopicClassificationProposal,
): TopicClassificationProposal {
  return {
    ...proposal,
    assignments: proposal.assignments.map((assignment) => ({ ...assignment })),
  }
}

export function updateClassificationAssignment(
  proposal: TopicClassificationProposal,
  book: string,
  topicId: string,
): TopicClassificationProposal {
  return {
    ...proposal,
    assignments: proposal.assignments.map((assignment) =>
      assignment.book === book ? { ...assignment, topic_id: topicId } : assignment,
    ),
  }
}

/** Front-end fail-closed guard; the backend repeats this against live files. */
export function classificationProposalIsValid(
  proposal: TopicClassificationProposal,
  expectedBooks: string[],
  topics: TopicDefinition[],
): boolean {
  if (proposal.schema_version !== 1 || !proposal.inventory_sha256 || !proposal.catalog_revision) {
    return false
  }
  const expected = new Set(expectedBooks)
  const topicIds = new Set(topics.map((topic) => topic.id))
  const seen = new Set<string>()
  for (const assignment of proposal.assignments) {
    if (
      !expected.has(assignment.book) ||
      seen.has(assignment.book) ||
      !topicIds.has(assignment.topic_id)
    ) {
      return false
    }
    seen.add(assignment.book)
  }
  return seen.size === expected.size
}
