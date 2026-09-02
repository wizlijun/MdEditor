import { describe, expect, it, vi } from 'vitest'
import { DocumentWriteQueue } from './document-write'

describe('DocumentWriteQueue', () => {
  it('严格按编辑顺序写入,后一次不会越过较慢的前一次', async () => {
    let releaseFirst!: () => void
    const first = new Promise<void>((resolve) => (releaseFirst = resolve))
    const writes: string[] = []
    const write = vi.fn(async (_path: string, content: string) => {
      if (content === 'first') await first
      writes.push(content)
    })
    const queue = new DocumentWriteQueue(write)

    const a = queue.enqueue('report.md', 'first')
    const b = queue.enqueue('report.md', 'second')
    await vi.waitFor(() => expect(write).toHaveBeenCalledTimes(1))
    releaseFirst()
    await Promise.all([a, b, queue.drain()])

    expect(writes).toEqual(['first', 'second'])
  })

  it('一次失败不会阻断后续重试', async () => {
    const write = vi
      .fn<(path: string, content: string) => Promise<unknown>>()
      .mockRejectedValueOnce(new Error('disk full'))
      .mockResolvedValueOnce({ ok: true })
    const queue = new DocumentWriteQueue(write)

    await expect(queue.enqueue('report.md', 'first')).rejects.toThrow('disk full')
    await expect(queue.enqueue('report.md', 'retry')).resolves.toBeUndefined()
    expect(write).toHaveBeenCalledTimes(2)
  })
})
