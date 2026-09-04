import { describe, it, expect, vi, beforeEach } from 'vitest'

// share/index.ts pulls the whole GUI surface (tabs, toasts, sotvault, i18n);
// mock every side-effectful module and keep theme-loader REAL so the test
// pins the actual settings.theme → themeId resolution.
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }))
vi.mock('../tabs.svelte', () => ({
  activeTab: vi.fn(),
  saveActive: vi.fn(),
}))
vi.mock('../settings.svelte', () => ({
  getPluginScopedKey: vi.fn(),
  settings: { theme: { light: 'paper-light', dark: 'ink-dark', followSystem: false } },
}))
vi.mock('../toast.svelte', () => ({ pushToast: vi.fn() }))
vi.mock('../platform.svelte', () => ({ isIOS: vi.fn(async () => false) }))
vi.mock('../plugins/share-baker', () => ({ bakeShareHtml: vi.fn(async () => '<html>x</html>') }))
vi.mock('./publish', () => ({
  publishHtml: vi.fn(async () => ({ url: 'https://w/s', slug: 's', isUpdate: false })),
  vaultRelativeSrc: vi.fn(() => 'notes/a.md'),
}))
vi.mock('../sotvault.svelte', () => ({
  sotvaultStore: { vaultRoot: '/vault' },
  ensureVaultCopyForShare: vi.fn(),
}))
vi.mock('../sotvault-logic', () => ({ isUnder: vi.fn(() => true) }))
vi.mock('./unpublish', () => ({ unpublish: vi.fn() }))
vi.mock('./copy-link', () => ({ copyShareLink: vi.fn() }))
vi.mock('./upload-image', () => ({ uploadImage: vi.fn() }))
vi.mock('../i18n/store.svelte', () => ({ t: (k: string) => k }))

import { sharePublishCurrent, shareUnpublishCurrent } from './index'
import { bakeShareHtml } from '../plugins/share-baker'
import { activeTab } from '../tabs.svelte'
import { getPluginScopedKey } from '../settings.svelte'
import { pushToast } from '../toast.svelte'

const CONFIGURED: Record<string, unknown> = {
  'share.baseUrl': 'https://w',
  'share.apiKey': 'k',
}

function mockTab() {
  return {
    id: 't1', filePath: '/vault/notes/a.md', title: 'a.md',
    initialContent: '# a', currentContent: '# a', mode: 'source',
    kind: 'markdown', externalState: 'fresh', externalBannerDismissed: false,
    lastKnownMtime: 0, lastKnownHash: '',
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  ;(getPluginScopedKey as any).mockImplementation((k: string) => CONFIGURED[k])
  ;(activeTab as any).mockReturnValue(mockTab())
})

describe('sharePublishCurrent', () => {
  it('bakes with the ACTIVE theme, not the default', async () => {
    // followSystem=false → active theme is the light slot ('paper-light').
    await sharePublishCurrent()
    expect(bakeShareHtml).toHaveBeenCalledTimes(1)
    const [tabArg, themeArg] = (bakeShareHtml as any).mock.calls[0]
    expect(tabArg.filePath).toBe('/vault/notes/a.md')
    expect(themeArg).toBe('paper-light')
    expect(themeArg).not.toBe('default')
    // success toast, no error
    expect((pushToast as any).mock.calls.some((c: any[]) => c[0].level === 'error')).toBe(false)
  })

  it('rejects Canvas instead of sending JSON through the Markdown baker', async () => {
    ;(activeTab as any).mockReturnValue({ ...mockTab(), kind: 'canvas', title: 'board.canvas' })
    await sharePublishCurrent()
    expect(bakeShareHtml).not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenCalledWith(expect.objectContaining({
      level: 'error',
      message: 'share.errPrefix',
    }))
  })
})

describe('shareUnpublishCurrent', () => {
  it('surfaces not_configured when config is missing but a tab is open', async () => {
    ;(getPluginScopedKey as any).mockReturnValue(undefined)
    await shareUnpublishCurrent()
    expect(pushToast).toHaveBeenCalledTimes(1)
    expect((pushToast as any).mock.calls[0][0].level).toBe('error')
  })

  it('stays silent when there is no active tab', async () => {
    ;(activeTab as any).mockReturnValue(undefined)
    ;(getPluginScopedKey as any).mockReturnValue(undefined)
    await shareUnpublishCurrent()
    expect(pushToast).not.toHaveBeenCalled()
  })
})
