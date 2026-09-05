import { beforeEach, describe, expect, it } from 'vitest'
import {
  beginTocLocationTracking,
  reportTocLocation,
  tocLocation,
} from './location.svelte'

beforeEach(() => {
  tocLocation.trackedTabId = null
  tocLocation.activeHeadingIndex = null
})

describe('TOC location tracking', () => {
  it('accepts reports only from the tracked editor', () => {
    const stop = beginTocLocationTracking('article-a')
    reportTocLocation('article-b', 3)
    expect(tocLocation.activeHeadingIndex).toBeNull()

    reportTocLocation('article-a', 2)
    expect(tocLocation.activeHeadingIndex).toBe(2)
    stop()
    expect(tocLocation).toMatchObject({ trackedTabId: null, activeHeadingIndex: null })
  })

  it('does not let stale cleanup or reports clear a newer tracking owner', () => {
    const stopA = beginTocLocationTracking('article-a')
    const stopB = beginTocLocationTracking('article-b')

    stopA()
    reportTocLocation('article-a', 1)
    reportTocLocation('article-b', 4)
    expect(tocLocation).toMatchObject({ trackedTabId: 'article-b', activeHeadingIndex: 4 })

    stopB()
    expect(tocLocation.trackedTabId).toBeNull()
  })
})
