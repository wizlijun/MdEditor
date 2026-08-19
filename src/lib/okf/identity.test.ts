import { describe, it, expect, beforeEach, vi } from 'vitest'
import { humanActor, humanActorNow, warmHumanActor, resetHumanActor } from './identity'

vi.mock('@tauri-apps/api/core', () => ({ invoke: async () => 'bruce' }))
vi.mock('../sotvault.svelte', () => ({ sotvaultStore: { vaultRoot: '/v' } }))

describe('humanActorNow', () => {
  beforeEach(() => resetHumanActor())

  it('冷启动时返回 null——宁可漏签,也不阻塞热路径', () => {
    expect(humanActorNow()).toBeNull()
  })

  it('预热后同步就能拿到完整 actor 串', async () => {
    warmHumanActor()
    await vi.waitFor(() => expect(humanActorNow()).toBe('human:bruce'))
  })

  it('await 过 humanActor 之后同步取值器也就热了', async () => {
    expect(await humanActor()).toBe('human:bruce')
    expect(humanActorNow()).toBe('human:bruce')
  })
})
