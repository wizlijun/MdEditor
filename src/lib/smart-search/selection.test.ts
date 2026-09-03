import { describe, expect, it } from 'vitest'
import { addRemovedKeys, chooseResultKeys, restoreRemovedKeys } from './selection'

const ordered = ['a', 'b', 'c', 'd']

describe('smart-search desktop list selection', () => {
  it('supports a plain single selection and Cmd/Ctrl toggle', () => {
    expect(chooseResultKeys(ordered, ['a'], 'c', 'a', { toggle: false, range: false })).toEqual(['c'])
    expect(chooseResultKeys(ordered, ['a'], 'c', 'a', { toggle: true, range: false })).toEqual(['a', 'c'])
    expect(chooseResultKeys(ordered, ['a', 'c'], 'a', 'c', { toggle: true, range: false })).toEqual(['c'])
  })

  it('supports Shift range selection without discarding an existing selection', () => {
    expect(chooseResultKeys(ordered, ['a'], 'd', 'b', { toggle: false, range: true }))
      .toEqual(['a', 'b', 'c', 'd'])
  })

  it('removes identities idempotently and restores only the last batch', () => {
    const removed = addRemovedKeys(['a'], ['b', 'b', 'c'])
    expect(removed).toEqual(['a', 'b', 'c'])
    expect(restoreRemovedKeys(removed, ['b', 'c'])).toEqual(['a'])
  })
})
