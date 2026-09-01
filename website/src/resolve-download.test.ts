import { describe, it, expect } from 'vitest'
import {
  detectTarget,
  isWindowsInstaller,
  macDownloadUrl,
  normalizeArch,
  normalizeOs,
  osFromUserAgent,
  windowsUrlFromManifest,
  windowsUrlFromReleasePages,
  windowsUrlFromReleases,
} from './resolve-download.js'

const MAC_UA =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15'
const WIN_UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36'
const LINUX_UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0'
const IPHONE_UA = 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_4 like Mac OS X) AppleWebKit/605.1.15 Version/17.4 Mobile/15E148'

// A release that has been through both halves of the pipeline: macOS first,
// then the Windows machine merging windows-* into the same latest.json.
const FULL_MANIFEST = {
  version: '6.808.3',
  platforms: {
    'darwin-aarch64': {
      url: 'https://github.com/wizlijun/note.md/releases/download/v6.808.3/note.md-aarch64.app.tar.gz',
    },
    'darwin-x86_64': {
      url: 'https://github.com/wizlijun/note.md/releases/download/v6.808.3/note.md-x86_64.app.tar.gz',
    },
    'windows-x86_64': {
      url: 'https://github.com/wizlijun/note.md/releases/download/v6.808.3/note.md_6.808.3_x64-setup.exe',
    },
  },
}

// The lag window: macOS shipped 6.809.1, the Windows machine hasn't run yet.
const MAC_ONLY_MANIFEST = {
  version: '6.809.1',
  platforms: {
    'darwin-aarch64': {
      url: 'https://github.com/wizlijun/note.md/releases/download/v6.809.1/note.md-aarch64.app.tar.gz',
    },
    'darwin-x86_64': {
      url: 'https://github.com/wizlijun/note.md/releases/download/v6.809.1/note.md-x86_64.app.tar.gz',
    },
  },
}

const dl = (tag: string, name: string) => ({
  name,
  browser_download_url: `https://github.com/wizlijun/note.md/releases/download/${tag}/${name}`,
})

// GitHub returns releases newest-first.
const RELEASES = [
  { tag_name: 'v6.809.1', draft: false, prerelease: false, assets: [dl('v6.809.1', 'note.md-6.809.1-aarch64.dmg')] },
  {
    tag_name: 'v6.808.3',
    draft: false,
    prerelease: false,
    assets: [
      dl('v6.808.3', 'note.md-6.808.3-aarch64.dmg'),
      dl('v6.808.3', 'note.md_6.808.3_x64-setup.exe'),
      dl('v6.808.3', 'note.md_6.808.3_x64-setup.exe.sig'),
    ],
  },
  {
    tag_name: 'v6.808.1',
    draft: false,
    prerelease: false,
    assets: [dl('v6.808.1', 'note.md_6.808.1_x64-setup.exe')],
  },
]

describe('normalizeArch', () => {
  it('accepts the spellings client hints and humans produce', () => {
    expect(normalizeArch('"arm"')).toBe('aarch64')
    expect(normalizeArch('arm64')).toBe('aarch64')
    expect(normalizeArch('AARCH64')).toBe('aarch64')
    expect(normalizeArch('x64')).toBe('x86_64')
    expect(normalizeArch('intel')).toBe('x86_64')
    expect(normalizeArch('"x86"')).toBe('x86_64')
  })

  it('rejects anything else', () => {
    expect(normalizeArch('riscv')).toBeNull()
    expect(normalizeArch('')).toBeNull()
    expect(normalizeArch(null)).toBeNull()
  })
})

describe('normalizeOs', () => {
  it('maps the aliases we link with', () => {
    expect(normalizeOs('win')).toBe('windows')
    expect(normalizeOs('Windows')).toBe('windows')
    expect(normalizeOs('macos')).toBe('mac')
    expect(normalizeOs('darwin')).toBe('mac')
  })

  it('rejects platforms we ship no binary for', () => {
    expect(normalizeOs('linux')).toBeNull()
    expect(normalizeOs('ios')).toBeNull()
  })
})

describe('osFromUserAgent', () => {
  it('splits mac from windows', () => {
    expect(osFromUserAgent(MAC_UA)).toBe('mac')
    expect(osFromUserAgent(WIN_UA)).toBe('windows')
  })

  it('serves nothing to platforms without a build', () => {
    expect(osFromUserAgent(LINUX_UA)).toBeNull()
    expect(osFromUserAgent(IPHONE_UA)).toBeNull()
    expect(osFromUserAgent('')).toBeNull()
  })
})

describe('detectTarget', () => {
  it('defaults mac to Apple Silicon and Windows to x64', () => {
    expect(detectTarget({ ua: MAC_UA })).toEqual({ os: 'mac', arch: 'aarch64' })
    expect(detectTarget({ ua: WIN_UA })).toEqual({ os: 'windows', arch: 'x86_64' })
  })

  it('lets explicit params override the user agent', () => {
    expect(detectTarget({ ua: WIN_UA, osParam: 'mac', archParam: 'x86_64' })).toEqual({
      os: 'mac',
      arch: 'x86_64',
    })
    // Intel-Mac link followed on a Windows box: still a Windows visitor.
    expect(detectTarget({ ua: WIN_UA, archParam: 'x86_64' })).toEqual({ os: 'windows', arch: 'x86_64' })
  })

  it('uses the client hint only when no explicit arch was given', () => {
    expect(detectTarget({ ua: WIN_UA, archHint: '"arm"' })).toEqual({ os: 'windows', arch: 'aarch64' })
    expect(detectTarget({ ua: WIN_UA, archHint: '"arm"', archParam: 'x64' })).toEqual({
      os: 'windows',
      arch: 'x86_64',
    })
  })

  it('returns null when we have nothing to offer', () => {
    expect(detectTarget({ ua: LINUX_UA })).toBeNull()
    expect(detectTarget({ ua: LINUX_UA, archParam: 'x86_64' })).toBeNull()
    expect(detectTarget({})).toBeNull()
  })
})

describe('macDownloadUrl', () => {
  it('composes the dmg name from version + arch, tag taken from the updater url', () => {
    expect(macDownloadUrl(FULL_MANIFEST, 'aarch64')).toBe(
      'https://github.com/wizlijun/note.md/releases/download/v6.808.3/note.md-6.808.3-aarch64.dmg',
    )
    expect(macDownloadUrl(FULL_MANIFEST, 'x86_64')).toBe(
      'https://github.com/wizlijun/note.md/releases/download/v6.808.3/note.md-6.808.3-x86_64.dmg',
    )
  })

  it('falls back to v<version> when the platform entry is missing', () => {
    expect(macDownloadUrl({ version: '1.2.3', platforms: {} }, 'aarch64')).toBe(
      'https://github.com/wizlijun/note.md/releases/download/v1.2.3/note.md-1.2.3-aarch64.dmg',
    )
  })

  it('gives up without a version', () => {
    expect(macDownloadUrl({}, 'aarch64')).toBeNull()
    expect(macDownloadUrl(null, 'aarch64')).toBeNull()
  })
})

describe('windowsUrlFromManifest', () => {
  it('uses the updater url verbatim — Tauri 2 signs the installer itself', () => {
    expect(windowsUrlFromManifest(FULL_MANIFEST, 'x86_64')).toBe(
      'https://github.com/wizlijun/note.md/releases/download/v6.808.3/note.md_6.808.3_x64-setup.exe',
    )
  })

  it('returns null during the lag window and for arches we do not build', () => {
    expect(windowsUrlFromManifest(MAC_ONLY_MANIFEST, 'x86_64')).toBeNull()
    expect(windowsUrlFromManifest(FULL_MANIFEST, 'aarch64')).toBeNull()
  })

  it('ignores a non-exe url', () => {
    const bogus = { platforms: { 'windows-x86_64': { url: 'https://example.com/note.md.zip' } } }
    expect(windowsUrlFromManifest(bogus, 'x86_64')).toBeNull()
  })
})

describe('isWindowsInstaller', () => {
  it('matches the Tauri NSIS bundle name, not its signature', () => {
    expect(isWindowsInstaller('note.md_6.808.3_x64-setup.exe', 'x86_64')).toBe(true)
    expect(isWindowsInstaller('note.md_6.808.3_x64-setup.exe.sig', 'x86_64')).toBe(false)
    expect(isWindowsInstaller('note.md_6.808.3_arm64-setup.exe', 'aarch64')).toBe(true)
    expect(isWindowsInstaller('note.md_6.808.3_arm64-setup.exe', 'x86_64')).toBe(false)
    expect(isWindowsInstaller('note.md-6.808.3-aarch64.dmg', 'x86_64')).toBe(false)
    expect(isWindowsInstaller(undefined, 'x86_64')).toBe(false)
  })
})

describe('windowsUrlFromReleases', () => {
  it('skips releases that have no Windows package yet', () => {
    expect(windowsUrlFromReleases(RELEASES, 'x86_64')).toBe(
      'https://github.com/wizlijun/note.md/releases/download/v6.808.3/note.md_6.808.3_x64-setup.exe',
    )
  })

  it('ignores drafts and prereleases', () => {
    const noisy = [
      { draft: true, assets: [dl('v9.9.9', 'note.md_9.9.9_x64-setup.exe')] },
      { prerelease: true, assets: [dl('v9.9.8', 'note.md_9.9.8_x64-setup.exe')] },
      ...RELEASES,
    ]
    expect(windowsUrlFromReleases(noisy, 'x86_64')).toBe(
      'https://github.com/wizlijun/note.md/releases/download/v6.808.3/note.md_6.808.3_x64-setup.exe',
    )
  })

  it('returns null when no release ships that arch', () => {
    expect(windowsUrlFromReleases(RELEASES, 'aarch64')).toBeNull()
    expect(windowsUrlFromReleases([], 'x86_64')).toBeNull()
    expect(windowsUrlFromReleases(null, 'x86_64')).toBeNull()
  })
})

describe('windowsUrlFromReleasePages', () => {
  it('keeps paging when the newest full page has no Windows installer', async () => {
    const firstPage = Array.from({ length: 100 }, (_, i) => ({
      tag_name: `v7.${100 - i}.0`,
      draft: false,
      prerelease: false,
      assets: [dl(`v7.${100 - i}.0`, `note.md-7.${100 - i}.0-aarch64.dmg`)],
    }))
    const secondPage = [
      {
        tag_name: 'v6.811.1',
        draft: false,
        prerelease: false,
        assets: [dl('v6.811.1', 'note.md_6.811.1_x64-setup.exe')],
      },
    ]
    const requested: number[] = []

    const url = await windowsUrlFromReleasePages((page: number) => {
      requested.push(page)
      return Promise.resolve(page === 1 ? firstPage : secondPage)
    }, 'x86_64', 100)

    expect(url).toBe(
      'https://github.com/wizlijun/note.md/releases/download/v6.811.1/note.md_6.811.1_x64-setup.exe',
    )
    expect(requested).toEqual([1, 2])
  })

  it('stops after the final short page when no installer exists', async () => {
    const requested: number[] = []
    const url = await windowsUrlFromReleasePages((page: number) => {
      requested.push(page)
      return Promise.resolve(page === 1 ? Array.from({ length: 2 }, () => ({ assets: [] })) : [])
    }, 'x86_64', 100)

    expect(url).toBeNull()
    expect(requested).toEqual([1])
  })
})
