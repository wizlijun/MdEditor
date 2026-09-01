import type { AgentOption } from './agent-picker/types'

export const TOPIC_DESIGN_PROVIDER_ID = 'notemd.claude-agent'

export type TopicDesignUnavailableReason = 'missing' | 'unavailable'

export type TopicDesignAvailability =
  | { available: true; provider: AgentOption }
  | { available: false; reason: TopicDesignUnavailableReason }

/**
 * Topic design consumes untrusted metadata. Only Claude's task-scoped file
 * allowlist is currently verifiable; the general AI-read picker remains
 * intentionally broader and does not use this gate.
 */
export function topicDesignAvailability(agents: AgentOption[]): TopicDesignAvailability {
  const provider = agents.find((agent) => agent.id === TOPIC_DESIGN_PROVIDER_ID)
  if (!provider) return { available: false, reason: 'missing' }
  if (!provider.harness?.ok) return { available: false, reason: 'unavailable' }
  return { available: true, provider }
}
