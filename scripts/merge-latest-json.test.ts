import { describe as vdescribe, it, expect } from 'vitest'
// @ts-expect-error — plain .mjs helper, no type declarations
import { mergePlatform, describe as summarize, MergeError, KNOWN_PLATFORMS } from './merge-latest-json-core.mjs'

const TAG_URL =
  'https://github.com/wizlijun/note.md/releases/download/v6.808.3/note.md_6.808.3_x64-setup.nsis.zip'

function macManifest() {
  return {
    version: '6.808.3',
    notes: 'See https://github.com/wizlijun/note.md/releases/tag/v6.808.3',
    pub_date: '2026-08-09T02:00:00Z',
    platforms: {
      'darwin-aarch64': { signature: 'sig-arm', url: 'https://x/v6.808.3/a.tar.gz' },
      'darwin-x86_64': { signature: 'sig-intel', url: 'https://x/v6.808.3/b.tar.gz' },
    },
  }
}

const winEntry = {
  platform: 'windows-x86_64',
  url: TAG_URL,
  signature: 'win-sig',
  version: '6.808.3',
}

vdescribe('mergePlatform', () => {
  it('adds the windows entry while keeping both darwin entries intact', () => {
    const out = mergePlatform(macManifest(), winEntry)
    expect(summarize(out)).toBe('darwin-aarch64, darwin-x86_64, windows-x86_64')
    expect(out.platforms['windows-x86_64']).toEqual({ signature: 'win-sig', url: TAG_URL })
    // 原有条目逐字节不动
    expect(out.platforms['darwin-aarch64']).toEqual(macManifest().platforms['darwin-aarch64'])
    expect(out.version).toBe('6.808.3')
    expect(out.pub_date).toBe('2026-08-09T02:00:00Z')
  })

  it('does not mutate the input manifest', () => {
    const input = macManifest()
    mergePlatform(input, winEntry)
    expect(Object.keys(input.platforms)).toEqual(['darwin-aarch64', 'darwin-x86_64'])
  })

  it('is idempotent — re-running replaces its own entry, never duplicates', () => {
    const once = mergePlatform(macManifest(), winEntry)
    const twice = mergePlatform(once, { ...winEntry, signature: 'win-sig-v2' })
    expect(Object.keys(twice.platforms).sort()).toEqual([
      'darwin-aarch64',
      'darwin-x86_64',
      'windows-x86_64',
    ])
    expect(twice.platforms['windows-x86_64'].signature).toBe('win-sig-v2')
  })

  it('rejects a version mismatch — the wrong-version installer trap', () => {
    // manifest 是 6.808.3,构建产物却是 6.808.2:签名会验过,用户装到错版本。
    expect(() => mergePlatform(macManifest(), { ...winEntry, version: '6.808.2' })).toThrow(
      MergeError,
    )
  })

  it('rejects an empty signature (the unset-password hang produces exactly this)', () => {
    expect(() => mergePlatform(macManifest(), { ...winEntry, signature: '   ' })).toThrow(
      /TAURI_SIGNING_PRIVATE_KEY_PASSWORD/,
    )
  })

  it('rejects a url pointing at a different tag', () => {
    const wrong = TAG_URL.replace('v6.808.3', 'v6.808.2')
    expect(() => mergePlatform(macManifest(), { ...winEntry, url: wrong })).toThrow(MergeError)
  })

  it('rejects a non-https url', () => {
    expect(() =>
      mergePlatform(macManifest(), { ...winEntry, url: 'http://x/v6.808.3/a.zip' }),
    ).toThrow(MergeError)
  })

  it('rejects an unknown platform key (a typo would silently mean "no update")', () => {
    expect(() => mergePlatform(macManifest(), { ...winEntry, platform: 'win32-x64' })).toThrow(
      MergeError,
    )
  })

  it('rejects a manifest with no platforms object', () => {
    expect(() => mergePlatform({ version: '6.808.3' }, winEntry)).toThrow(MergeError)
  })

  it('knows the exact keys tauri-plugin-updater builds', () => {
    // 对齐 tauri-plugin-updater src/updater.rs 的 updater_os()/updater_arch()
    expect(KNOWN_PLATFORMS).toContain('windows-x86_64')
    expect(KNOWN_PLATFORMS).toContain('windows-aarch64')
    expect(KNOWN_PLATFORMS).toContain('darwin-aarch64')
  })
})
