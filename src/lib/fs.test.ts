import { describe, it, expect, vi } from 'vitest'
import { classifyPath, isSupportedPath, isPermissionError, looksBinary, modeKeyFor } from './fs'

describe('classifyPath', () => {
  it('markdown extensions', () => {
    expect(classifyPath('foo.md')).toEqual({ kind: 'markdown' })
    expect(classifyPath('foo.markdown')).toEqual({ kind: 'markdown' })
    expect(classifyPath('foo.mdown')).toEqual({ kind: 'markdown' })
    expect(classifyPath('foo.mkd')).toEqual({ kind: 'markdown' })
  })

  it('mdx is its own kind, never markdown', () => {
    // 'markdown' 在本代码库里等价于"可被 ProseMirror 全文序列化回写"。
    // mdx 是别人的构建源码,回写会毁掉 JSX/import —— 必须是独立 kind。
    expect(classifyPath('foo.mdx')).toEqual({ kind: 'mdx' })
    expect(classifyPath('FOO.MDX')).toEqual({ kind: 'mdx' })
  })

  it('html extensions', () => {
    expect(classifyPath('foo.html')).toEqual({ kind: 'html' })
    expect(classifyPath('foo.htm')).toEqual({ kind: 'html' })
  })

  it('code extensions with language', () => {
    expect(classifyPath('foo.py')).toEqual({ kind: 'code', language: 'python' })
    expect(classifyPath('foo.json')).toEqual({ kind: 'code', language: 'json' })
    expect(classifyPath('foo.ts')).toEqual({ kind: 'code', language: 'typescript' })
    expect(classifyPath('foo.rs')).toEqual({ kind: 'code', language: 'rust' })
    expect(classifyPath('foo.yml')).toEqual({ kind: 'code', language: 'yaml' })
    expect(classifyPath('foo.sh')).toEqual({ kind: 'code', language: 'bash' })
  })

  it('plain-text extensions with empty language', () => {
    expect(classifyPath('foo.txt')).toEqual({ kind: 'code', language: '' })
    expect(classifyPath('foo.log')).toEqual({ kind: 'code', language: '' })
  })

  it('subtitle extensions are plain-text code', () => {
    expect(classifyPath('movie.srt')).toEqual({ kind: 'code', language: '' })
    expect(classifyPath('movie.vtt')).toEqual({ kind: 'code', language: '' })
    expect(classifyPath('movie.ass')).toEqual({ kind: 'code', language: '' })
    expect(classifyPath('movie.ssa')).toEqual({ kind: 'code', language: '' })
  })

  it('diff/patch extensions highlight as diff', () => {
    expect(classifyPath('a.diff')).toEqual({ kind: 'code', language: 'diff' })
    expect(classifyPath('a.patch')).toEqual({ kind: 'code', language: 'diff' })
  })

  it('spreadsheet extensions', () => {
    expect(classifyPath('foo.csv')).toEqual({ kind: 'spreadsheet' })
  })

  it('tsv is plain-text code (tab-delimited parsing not yet implemented)', () => {
    expect(classifyPath('foo.tsv')).toEqual({ kind: 'code', language: '' })
  })

  it('special filenames (no extension)', () => {
    expect(classifyPath('/path/to/Dockerfile')).toEqual({ kind: 'code', language: 'dockerfile' })
    expect(classifyPath('/repo/Makefile')).toEqual({ kind: 'code', language: 'makefile' })
    expect(classifyPath('Gemfile')).toEqual({ kind: 'code', language: 'ruby' })
  })

  it('case insensitive', () => {
    expect(classifyPath('FOO.PY')).toEqual({ kind: 'code', language: 'python' })
    expect(classifyPath('README.MD')).toEqual({ kind: 'markdown' })
    expect(classifyPath('DOCKERFILE')).toEqual({ kind: 'code', language: 'dockerfile' })
  })

  it('image extensions', () => {
    expect(classifyPath('/tmp/foo.png')).toEqual({ kind: 'image' })
    expect(classifyPath('/tmp/foo.jpg')).toEqual({ kind: 'image' })
    expect(classifyPath('/tmp/foo.jpeg')).toEqual({ kind: 'image' })
    expect(classifyPath('/tmp/foo.gif')).toEqual({ kind: 'image' })
    expect(classifyPath('/tmp/foo.webp')).toEqual({ kind: 'image' })
    expect(classifyPath('/tmp/foo.svg')).toEqual({ kind: 'image' })
    expect(classifyPath('/tmp/foo.bmp')).toEqual({ kind: 'image' })
    expect(classifyPath('/tmp/foo.heic')).toEqual({ kind: 'image' })
    expect(classifyPath('/tmp/foo.heif')).toEqual({ kind: 'image' })
    expect(classifyPath('/tmp/foo.avif')).toEqual({ kind: 'image' })
  })

  it('image extensions case-insensitive', () => {
    expect(classifyPath('/tmp/foo.HEIC')).toEqual({ kind: 'image' })
    expect(classifyPath('/tmp/photo.PNG')).toEqual({ kind: 'image' })
    expect(classifyPath('/tmp/img.SVG')).toEqual({ kind: 'image' })
  })

  it('unknown extensions return null', () => {
    expect(classifyPath('foo.exe')).toBe(null)
    expect(classifyPath('noextension')).toBe(null)
  })
})

describe('isPermissionError', () => {
  it('detects tauri-plugin-fs scope rejections', () => {
    expect(isPermissionError(new Error('forbidden path: /Users/bob/.config/x, maybe it is not allowed on the scope'))).toBe(true)
  })

  it('detects OS-level permission denials', () => {
    expect(isPermissionError(new Error('Permission denied (os error 13)'))).toBe(true)
    expect(isPermissionError('Operation not permitted')).toBe(true)
    expect(isPermissionError(new Error('Access is denied. (os error 5)'))).toBe(true)
  })

  it('returns false for unrelated errors', () => {
    expect(isPermissionError(new Error('File not found'))).toBe(false)
    expect(isPermissionError(null)).toBe(false)
    expect(isPermissionError(undefined)).toBe(false)
  })
})

describe('isSupportedPath', () => {
  it('returns true for supported extensions', () => {
    expect(isSupportedPath('foo.md')).toBe(true)
    expect(isSupportedPath('foo.html')).toBe(true)
    expect(isSupportedPath('foo.py')).toBe(true)
    expect(isSupportedPath('Dockerfile')).toBe(true)
  })

  it('returns true for image extensions', () => {
    expect(isSupportedPath('foo.png')).toBe(true)
    expect(isSupportedPath('foo.jpg')).toBe(true)
    expect(isSupportedPath('photo.HEIC')).toBe(true)
    expect(isSupportedPath('image.svg')).toBe(true)
  })

  it('returns false for unsupported', () => {
    expect(isSupportedPath('foo.exe')).toBe(false)
    expect(isSupportedPath('foo')).toBe(false)
  })
})

describe('looksBinary', () => {
  it('plain text returns false', () => {
    expect(looksBinary('hello world')).toBe(false)
    expect(looksBinary('# Title\n\nSome text\n')).toBe(false)
    expect(looksBinary('')).toBe(false)
  })

  it('content with NUL byte returns true', () => {
    expect(looksBinary('hello\x00world')).toBe(true)
  })

  it('mostly non-printable returns true', () => {
    let s = ''
    for (let i = 0; i < 100; i++) s += '\x01'
    s += 'abcde'
    expect(looksBinary(s)).toBe(true)
  })

  it('common control chars (tab, LF, CR) are OK', () => {
    expect(looksBinary('a\tb\nc\rd')).toBe(false)
  })
})

describe('modeKeyFor', () => {
  it('returns lowercased extension for normal files', () => {
    expect(modeKeyFor('/tmp/foo.md')).toBe('md')
    expect(modeKeyFor('/tmp/script.PY')).toBe('py')
    expect(modeKeyFor('relative/path/index.html')).toBe('html')
  })

  it('returns full basename for files without extension', () => {
    expect(modeKeyFor('/repo/Dockerfile')).toBe('dockerfile')
    expect(modeKeyFor('Makefile')).toBe('makefile')
  })

  it('treats dotfiles as full-name keys (not extensions)', () => {
    expect(modeKeyFor('/proj/.env')).toBe('.env')
    expect(modeKeyFor('.gitignore')).toBe('.gitignore')
  })

  it('handles multiple dots — uses the last one', () => {
    expect(modeKeyFor('/tmp/archive.tar.gz')).toBe('gz')
    expect(modeKeyFor('/proj/.env.local')).toBe('local')
  })
})

vi.mock('@tauri-apps/plugin-fs', () => ({
  readTextFile: vi.fn(),
  writeTextFile: vi.fn(),
  stat: vi.fn(async () => ({ mtime: new Date(1_700_000_000_000), size: 42 })),
}))

describe('statFile', () => {
  it('returns mtime in ms and size from plugin-fs.stat', async () => {
    const { statFile } = await import('./fs')
    const info = await statFile('/tmp/foo.md')
    expect(info).not.toBeNull()
    expect(info!.mtime).toBe(1_700_000_000_000)
    expect(info!.size).toBe(42)
  })

  it('returns null when stat throws', async () => {
    const fsPlug = await import('@tauri-apps/plugin-fs')
    ;(fsPlug.stat as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('ENOENT'))
    const { statFile } = await import('./fs')
    expect(await statFile('/tmp/missing.md')).toBe(null)
  })
})
