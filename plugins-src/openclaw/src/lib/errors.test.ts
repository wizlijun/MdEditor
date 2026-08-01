import { describe, it, expect, beforeEach } from 'vitest'
import { errorKey, describeError } from './errors'
import { setLocale } from './strings'

// Exact wording the Rust backend emits (plugins-src/openclaw/backend/src/{lib,pair}.rs).
// Pinning these here means a backend wording change that breaks the mapping
// fails this test instead of silently falling back to raw English.
describe('errorKey', () => {
  it('maps a missing access token', () => {
    expect(
      errorKey('access token not configured — run OpenClaw once to auto-generate'),
    ).toBe('err.noAccessToken')
  })

  it('maps a missing relay URL', () => {
    expect(errorKey('no relay URL')).toBe('err.noRelayUrl')
  })

  it('leaves an unrecognized message unmatched', () => {
    expect(errorKey('not paired — open Devices to pair')).toBe(null)
    expect(errorKey('status 404')).toBe(null)
    expect(errorKey('some future backend string')).toBe(null)
  })
})

describe('describeError', () => {
  beforeEach(() => setLocale('en'))

  it('localizes a known backend message', () => {
    expect(describeError('no relay URL')).toBe('No relay URL configured — set one up in OpenClaw settings.')
  })

  it('turns a pairing HTTP status into a localized sentence', () => {
    expect(describeError('status 404')).toBe('Pairing failed (status 404).')
    setLocale('zh')
    expect(describeError('status 404')).toBe('配对失败(状态 404)。')
    setLocale('en')
  })

  it('passes an unrecognized message through unchanged', () => {
    expect(describeError('connection refused')).toBe('connection refused')
  })
})
