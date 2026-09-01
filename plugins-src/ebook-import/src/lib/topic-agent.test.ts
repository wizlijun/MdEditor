import { describe, expect, it } from 'vitest'
import { TOPIC_DESIGN_PROVIDER_ID, topicDesignAvailability } from './topic-agent'
import type { AgentOption } from './agent-picker/types'

const agent = (id: string, ok = true): AgentOption => ({
  id,
  name: id,
  harness: { harness: id, ok },
})

describe('topicDesignAvailability', () => {
  it('accepts only a verified, usable Claude Agent', () => {
    const result = topicDesignAvailability([
      agent('notemd.codex-agent'),
      agent(TOPIC_DESIGN_PROVIDER_ID),
      agent('notemd.deepseek-agent'),
    ])
    expect(result).toMatchObject({ available: true, provider: { id: TOPIC_DESIGN_PROVIDER_ID } })
  })

  it('does not fall back to Codex, DeepSeek, or an unknown default', () => {
    expect(
      topicDesignAvailability([agent('notemd.codex-agent'), agent('notemd.deepseek-agent')]),
    ).toEqual({ available: false, reason: 'missing' })
    expect(topicDesignAvailability([])).toEqual({ available: false, reason: 'missing' })
  })

  it('reports an installed but unverifiable Claude harness as unavailable', () => {
    expect(topicDesignAvailability([agent(TOPIC_DESIGN_PROVIDER_ID, false)])).toEqual({
      available: false,
      reason: 'unavailable',
    })
    expect(
      topicDesignAvailability([{ id: TOPIC_DESIGN_PROVIDER_ID, name: 'Claude Agent' }]),
    ).toEqual({ available: false, reason: 'unavailable' })
  })
})
