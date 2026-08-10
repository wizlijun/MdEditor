// src/lib/dialogs.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest'

// message() mock must be hoisted-safe; use vi.hoisted so the factory can reference it.
const { messageMock } = vi.hoisted(() => ({ messageMock: vi.fn() }))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  message: messageMock,
  save: vi.fn(),
  open: vi.fn(),
}))

// Deterministic t(): echo the key, and for the {name} template append the name so
// the test can assert the filename reached the title.
vi.mock('./i18n/store.svelte', () => ({
  t: (key: string, params?: Record<string, string>) =>
    params?.name ? `${key}|${params.name}` : key,
}))

beforeEach(() => { messageMock.mockReset() })

describe('confirmDirtyClose', () => {
  it('shows a single 3-button native dialog with the filename in the title', async () => {
    messageMock.mockResolvedValueOnce('Yes')
    const { confirmDirtyClose } = await import('./dialogs')
    await confirmDirtyClose('README.md')

    expect(messageMock).toHaveBeenCalledTimes(1)
    const [info, opts] = messageMock.mock.calls[0]
    expect(info).toBe('dialog.saveChanges.info')          // informative text
    expect(opts.title).toContain('README.md')             // bold headline carries filename
    expect(opts.kind).toBe('warning')
    expect(opts.buttons).toEqual({
      yes: 'dialog.save',
      no: 'dialog.dontSave',
      cancel: 'common.cancel',
    })
  })

  it('maps the clicked button label to save / discard / cancel', async () => {
    const { confirmDirtyClose } = await import('./dialogs')
    // message() returns the LABEL text of the clicked custom button
    messageMock.mockResolvedValueOnce('dialog.save')
    expect(await confirmDirtyClose('a.md')).toBe('save')
    messageMock.mockResolvedValueOnce('dialog.dontSave')
    expect(await confirmDirtyClose('a.md')).toBe('discard')
    messageMock.mockResolvedValueOnce('common.cancel')
    expect(await confirmDirtyClose('a.md')).toBe('cancel')
    // Esc / window dismiss returns something else → treated as cancel
    messageMock.mockResolvedValueOnce('')
    expect(await confirmDirtyClose('a.md')).toBe('cancel')
  })
})

describe('pickSaveFile filters', () => {
  const filtersFor = async (path: string) => {
    const { save } = await import('@tauri-apps/plugin-dialog')
    ;(save as ReturnType<typeof vi.fn>).mockReset()
    ;(save as ReturnType<typeof vi.fn>).mockResolvedValueOnce(path)
    const { pickSaveFile } = await import('./dialogs')
    await pickSaveFile(path)
    return (save as ReturnType<typeof vi.fn>).mock.calls[0][0].filters as
      { name: string; extensions: string[] }[]
  }

  it('does not offer mdx when saving a markdown file', async () => {
    // Saving a .md as .mdx would silently move it onto the read-only surface.
    const filters = await filtersFor('/d/readme.md')
    expect(filters.flatMap(f => f.extensions)).not.toContain('mdx')
  })

  it('offers mdx for an mdx file', async () => {
    const filters = await filtersFor('/d/guide.mdx')
    expect(filters[0].extensions).toEqual(['mdx'])
  })
})
