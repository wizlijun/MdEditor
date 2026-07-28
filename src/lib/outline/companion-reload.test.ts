import { describe, it, expect } from 'vitest'
import { decideCompanionReload, type CompanionReloadInput } from './companion-reload'

/** 默认:有基线、内存树与基线一致(= 干净)、已激活 */
const input = (o: Partial<CompanionReloadInput> = {}): CompanionReloadInput => ({
  diskHash: 'disk-new',
  lastHash: 'disk-old',
  ourHash: 'canon-old',
  canonicalHash: 'canon-old',
  armed: true,
  dirtyFlag: false,
  ...o,
})

describe('decideCompanionReload', () => {
  it('ignores our own write echo (disk bytes unchanged)', () => {
    expect(decideCompanionReload(input({ diskHash: 'x', lastHash: 'x' }))).toBe('ignore')
  })

  it('ignores the echo even while locally dirty — same bytes are not a conflict', () => {
    expect(decideCompanionReload(input({
      diskHash: 'x', lastHash: 'x', ourHash: 'other', canonicalHash: 'canon-old',
    }))).toBe('ignore')
  })

  it('ignores when our tree already serializes to exactly what is on disk', () => {
    expect(decideCompanionReload(input({ diskHash: 'same', ourHash: 'same' }))).toBe('ignore')
  })

  it('reloads silently when the note moved ahead and we have nothing of our own', () => {
    expect(decideCompanionReload(input())).toBe('reload')
  })

  it('raises a conflict when our tree diverged from the baseline', () => {
    expect(decideCompanionReload(input({ ourHash: 'canon-edited' }))).toBe('conflict')
  })

  it('compares against the CANONICAL baseline, not the raw disk bytes', () => {
    // 手写笔记:磁盘字节与「解析→序列化」结果天然不同。拿 lastHash 比会永远判脏,
    // 于是永远等不到静默重载 —— 这里断言用规范形基线,干净就是干净。
    expect(decideCompanionReload(input({
      lastHash: 'raw-bytes-differ-from-canonical',
      ourHash: 'canon-old',
      canonicalHash: 'canon-old',
    }))).toBe('reload')
  })

  describe('no baseline yet (note created externally after we mounted)', () => {
    const noBaseline = (o: Partial<CompanionReloadInput> = {}) =>
      decideCompanionReload(input({ lastHash: null, ourHash: null, canonicalHash: null, ...o }))

    it('reloads when the tree is pure derivation (never armed)', () => {
      // 主文档随便敲几个字就会经 markSynced 置 dirtyFlag,但没 armed 就没有人写过东西
      expect(noBaseline({ armed: false, dirtyFlag: true })).toBe('reload')
    })

    it('conflicts when the user actually wrote something', () => {
      expect(noBaseline({ armed: true, dirtyFlag: true })).toBe('conflict')
    })

    it('reloads when nothing is pending at all', () => {
      expect(noBaseline({ armed: true, dirtyFlag: false })).toBe('reload')
    })
  })
})
