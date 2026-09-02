import type { AgentOption } from './agent-picker/types'

export const TOPIC_DESIGN_PROVIDER_IDS = [
  'notemd.claude-agent',
  'notemd.codex-agent',
  'notemd.deepseek-agent',
] as const

export type TopicDesignReadScope = 'inventory' | 'vault'

export type TopicDesignUnavailableReason = 'missing' | 'unavailable'

export type TopicDesignAvailability =
  | { available: true; provider: AgentOption }
  | { available: false; reason: TopicDesignUnavailableReason }

/**
 * Topic design consumes untrusted metadata, so it has an explicit provider
 * allowlist rather than inheriting an arbitrary host default. Claude can
 * enforce the inventory-only allowlist; Codex and DeepSeek still run under the
 * read-only task policy, but their wider Vault read boundary is shown in UI.
 */
export function topicDesignProviders(agents: AgentOption[]): AgentOption[] {
  return agents.filter((agent) =>
    TOPIC_DESIGN_PROVIDER_IDS.includes(agent.id as (typeof TOPIC_DESIGN_PROVIDER_IDS)[number]),
  )
}

export function topicDesignAvailability(
  agents: AgentOption[],
  selected: string | undefined,
): TopicDesignAvailability {
  const provider = agents.find((agent) => agent.id === selected)
  if (!provider) return { available: false, reason: 'missing' }
  if (!provider.harness?.ok) return { available: false, reason: 'unavailable' }
  return { available: true, provider }
}

export function topicDesignReadScope(providerId: string): TopicDesignReadScope {
  return providerId === 'notemd.claude-agent' ? 'inventory' : 'vault'
}
