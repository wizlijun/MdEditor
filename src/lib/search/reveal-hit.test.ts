import { describe, expect, it, vi } from 'vitest'
import { openAndRevealSearchHit, type SearchHitRevealRequest } from './reveal-hit'

const request: SearchHitRevealRequest = {
  requestId: 'jump-1',
  path: '/vault/mirror.md',
  line: 42,
  anchor: 'Launch risks',
}

describe('openAndRevealSearchHit', () => {
  it('awaits the open and binds reveal to the final redirected path', async () => {
    let opened = false
    const reveal = vi.fn(() => {
      expect(opened).toBe(true)
    })

    const finalPath = await openAndRevealSearchHit(request, {
      openFile: async (path) => {
        expect(path).toBe('/vault/mirror.md')
        await Promise.resolve()
        opened = true
      },
      activePath: () => '/Users/me/source.md',
      reveal,
    })

    expect(finalPath).toBe('/Users/me/source.md')
    expect(reveal).toHaveBeenCalledWith(42, 'Launch risks', '/Users/me/source.md')
  })

  it('falls back to the requested path when no active tab path is available', async () => {
    const reveal = vi.fn()
    const finalPath = await openAndRevealSearchHit(request, {
      openFile: vi.fn(async () => {}),
      activePath: () => null,
      reveal,
    })

    expect(finalPath).toBe(request.path)
    expect(reveal).toHaveBeenCalledWith(42, 'Launch risks', request.path)
  })
})
