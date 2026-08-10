import { describe, it, expect, vi, beforeEach } from 'vitest'

const written = new Map<string, string>()
const files = new Map<string, string>()

vi.mock('@tauri-apps/plugin-fs', () => ({
  exists: vi.fn(async (p: string) => files.has(p)),
  readTextFile: vi.fn(async (p: string) => files.get(p) ?? ''),
  writeTextFile: vi.fn(async (p: string, t: string) => { written.set(p, t); files.set(p, t) }),
}))

const sotvaultStore = { vaultRoot: null as string | null, records: [] as any[] }
const syncSourceToVaultAsHome = vi.fn(async (_src: string) => ({ vault_path: '/vault/sync/guide.mdx' }))
vi.mock('../sotvault.svelte', () => ({
  sotvaultStore,
  syncSourceToVaultAsHome: (p: string) => syncSourceToVaultAsHome(p),
  refreshSotvault: vi.fn(async () => {}),
}))

const { excerptToNote } = await import('./excerpt-command')

beforeEach(() => {
  written.clear(); files.clear()
  sotvaultStore.vaultRoot = null; sotvaultStore.records = []
  syncSourceToVaultAsHome.mockClear()
})

describe('excerptToNote', () => {
  it('writes the excerpt beside a vault-internal document', async () => {
    sotvaultStore.vaultRoot = '/vault'
    const res = await excerptToNote('/vault/docs/guide.mdx', '值得记下来的一句')
    expect(res).toEqual({ ok: true, notePath: '/vault/docs/guide.mdx.note.md' })
    expect(written.get('/vault/docs/guide.mdx.note.md')).toContain('值得记下来的一句')
  })

  it('mirrors an unsynced outside document into the vault first', async () => {
    // Same vault-homing rule as writing a note against any outside file: the
    // note must not be stranded next to a path that moves between machines.
    sotvaultStore.vaultRoot = '/vault'
    const res = await excerptToNote('/proj/docs/guide.mdx', '外部文档的一句')
    expect(syncSourceToVaultAsHome).toHaveBeenCalledWith('/proj/docs/guide.mdx')
    expect(res).toEqual({ ok: true, notePath: '/vault/sync/guide.mdx.note.md' })
    expect(written.get('/vault/sync/guide.mdx.note.md')).toContain('外部文档的一句')
  })

  it('appends to an existing note instead of replacing it', async () => {
    sotvaultStore.vaultRoot = '/vault'
    files.set('/vault/docs/guide.mdx.note.md', '- 早先的一条\n')
    await excerptToNote('/vault/docs/guide.mdx', '新的一条')
    const out = written.get('/vault/docs/guide.mdx.note.md')!
    expect(out).toContain('早先的一条')
    expect(out).toContain('新的一条')
  })

  it('goes through the live tree when the sidecar panel owns that file', async () => {
    // The panel holds the note's text in memory; writing underneath it would be
    // clobbered by its next save. Mutate the tree it owns instead.
    const { outline } = await import('./store.svelte')
    const { parseOutline } = await import('./markdown')
    const { childrenOf } = await import('./model')
    sotvaultStore.vaultRoot = '/vault'
    outline.docPath = '/vault/docs/guide.mdx.note.md'
    outline.tree = parseOutline('- 面板里已有的一条\n')
    try {
      const res = await excerptToNote('/vault/docs/guide.mdx', '面板挂着时摘录的一句')
      expect(res.ok).toBe(true)
      expect(childrenOf(outline.tree, null).map(n => n.content))
        .toEqual(['面板里已有的一条', '面板挂着时摘录的一句'])
      expect(written.size).toBe(0)   // the panel's save path owns the write
    } finally {
      outline.docPath = null
    }
  })

  it('refuses a blank selection without touching disk', async () => {
    sotvaultStore.vaultRoot = '/vault'
    const res = await excerptToNote('/vault/docs/guide.mdx', '  \n ')
    expect(res).toEqual({ ok: false, reason: 'empty' })
    expect(written.size).toBe(0)
  })

  it('asks for a vault instead of writing next to the source', async () => {
    sotvaultStore.vaultRoot = null
    const res = await excerptToNote('/proj/docs/guide.mdx', '一句话')
    expect(res).toEqual({ ok: false, reason: 'configure-vault' })
    expect(written.size).toBe(0)
  })
})
