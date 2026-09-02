import { describe, expect, it } from 'vitest'
import {
  TOPIC_DESIGN_PROVIDER_IDS,
  topicDesignAvailability,
  topicDesignProviders,
  topicDesignReadScope,
} from './topic-agent'
import type { AgentOption } from './agent-picker/types'

const agent = (id: string, ok = true): AgentOption => ({
  id,
  name: id,
  harness: { harness: id, ok },
})

describe('topicDesignAvailability', () => {
  it('offers only the supported topic-design agents', () => {
    expect(
      topicDesignProviders([
        agent('notemd.codex-agent'),
        agent('unknown-agent'),
        agent('notemd.claude-agent'),
        agent('notemd.deepseek-agent'),
      ]).map(({ id }) => id),
    ).toEqual(['notemd.codex-agent', 'notemd.claude-agent', 'notemd.deepseek-agent'])
    expect(TOPIC_DESIGN_PROVIDER_IDS).toHaveLength(3)
  })

  it.each(TOPIC_DESIGN_PROVIDER_IDS)('accepts a usable supported provider: %s', (id) => {
    expect(topicDesignAvailability([agent(id)], id)).toMatchObject({
      available: true,
      provider: { id },
    })
  })

  it('requires an explicit installed selection', () => {
    const agents = [agent('notemd.codex-agent')]
    expect(topicDesignAvailability(agents, undefined)).toEqual({
      available: false,
      reason: 'missing',
    })
    expect(topicDesignAvailability(agents, 'notemd.deepseek-agent')).toEqual({
      available: false,
      reason: 'missing',
    })
    expect(topicDesignAvailability([], 'notemd.codex-agent')).toEqual({
      available: false,
      reason: 'missing',
    })
  })

  it('reports an installed but unusable selected harness as unavailable', () => {
    expect(
      topicDesignAvailability([agent('notemd.codex-agent', false)], 'notemd.codex-agent'),
    ).toEqual({
      available: false,
      reason: 'unavailable',
    })
    expect(
      topicDesignAvailability(
        [{ id: 'notemd.deepseek-agent', name: 'DeepSeek Agent' }],
        'notemd.deepseek-agent',
      ),
    ).toEqual({ available: false, reason: 'unavailable' })
  })

  it('labels the provider read boundary honestly', () => {
    expect(topicDesignReadScope('notemd.claude-agent')).toBe('inventory')
    expect(topicDesignReadScope('notemd.codex-agent')).toBe('vault')
    expect(topicDesignReadScope('notemd.deepseek-agent')).toBe('vault')
  })
})
