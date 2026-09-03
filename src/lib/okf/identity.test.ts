import { describe, it, expect, beforeEach, vi } from 'vitest'
import { humanActor, humanActorNow, warmHumanActor, resetHumanActor } from './identity'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  sotvaultStore: { vaultRoot: '/v' as string | null },
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('../sotvault.svelte', () => ({ sotvaultStore: mocks.sotvaultStore }))

describe('humanActorNow', () => {
  beforeEach(() => {
    resetHumanActor()
    mocks.invoke.mockReset()
    mocks.invoke.mockResolvedValue('bruce')
    mocks.sotvaultStore.vaultRoot = '/v'
  })

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

  it('并发预热共用同一个身份请求', async () => {
    expect(await Promise.all([humanActor(), humanActor()])).toEqual(['human:bruce', 'human:bruce'])
    expect(mocks.invoke).toHaveBeenCalledTimes(1)
  })

  it('vault 切换后旧请求晚到也不能覆盖新身份', async () => {
    let resolveOld!: (value: string) => void
    let resolveNew!: (value: string) => void
    mocks.invoke
      .mockImplementationOnce(() => new Promise<string>((resolve) => { resolveOld = resolve }))
      .mockImplementationOnce(() => new Promise<string>((resolve) => { resolveNew = resolve }))

    const oldRequest = humanActor()
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledTimes(1))

    mocks.sotvaultStore.vaultRoot = '/next'
    resetHumanActor()
    const newRequest = humanActor()
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledTimes(2))

    resolveOld('old-user')
    resolveNew('new-user')
    await expect(oldRequest).resolves.toBe('human:new-user')
    await expect(newRequest).resolves.toBe('human:new-user')
    expect(humanActorNow()).toBe('human:new-user')
    expect(mocks.invoke.mock.calls[1]?.[1]).toEqual({ vaultPath: '/next' })
  })
})
