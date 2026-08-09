import { describe, it, expect } from 'vitest'
import {
  normalize, pathRoot, isAbsolute, basename, dirname,
  joinPath, relative, isWithinDir, abbreviateHome,
} from './paths'

describe('normalize', () => {
  it('converts backslashes and collapses runs', () => {
    expect(normalize('D:\\vault\\note.md')).toBe('D:/vault/note.md')
    expect(normalize('/a//b///c')).toBe('/a/b/c')
  })
  it('keeps the UNC double slash', () => {
    expect(normalize('\\\\server\\share\\f.md')).toBe('//server/share/f.md')
  })
})

describe('pathRoot', () => {
  it('handles posix, drive and UNC roots', () => {
    expect(pathRoot('/a/b')).toBe('/')
    expect(pathRoot('D:\\a\\b')).toBe('D:/')
    expect(pathRoot('//server/share/a')).toBe('//server/share/')
    expect(pathRoot('a/b')).toBe('')
  })
})

describe('isAbsolute', () => {
  it('accepts both platforms', () => {
    expect(isAbsolute('/a')).toBe(true)
    expect(isAbsolute('C:\\a')).toBe(true)
    expect(isAbsolute('C:/a')).toBe(true)
    expect(isAbsolute('\\\\server\\share')).toBe(true)
    expect(isAbsolute('a/b')).toBe(false)
    expect(isAbsolute('./a')).toBe(false)
  })
})

describe('basename', () => {
  it('is the last segment on both separators', () => {
    expect(basename('/vault/sub/note.md')).toBe('note.md')
    expect(basename('D:\\vault\\sub\\note.md')).toBe('note.md')
  })
  it('ignores trailing separators', () => {
    expect(basename('/vault/sub/')).toBe('sub')
    expect(basename('D:\\vault\\sub\\')).toBe('sub')
  })
})

describe('dirname', () => {
  it('walks up one level', () => {
    expect(dirname('/vault/sub/note.md')).toBe('/vault/sub')
    expect(dirname('D:\\vault\\sub\\note.md')).toBe('D:/vault/sub')
  })
  it('stops at the root instead of walking off the drive', () => {
    expect(dirname('/note.md')).toBe('/')
    expect(dirname('D:\\note.md')).toBe('D:/')
    expect(dirname('D:\\')).toBe('D:/')
  })
})

describe('joinPath', () => {
  it('joins with a single separator', () => {
    expect(joinPath('/vault', 'a.md')).toBe('/vault/a.md')
    expect(joinPath('/vault/', 'a.md')).toBe('/vault/a.md')
    expect(joinPath('D:\\vault', 'a.md')).toBe('D:/vault/a.md')
  })
  it('keeps the separator after a bare drive', () => {
    expect(joinPath('D:\\', 'a.md')).toBe('D:/a.md')
  })
})

describe('relative', () => {
  it('strips the root prefix', () => {
    expect(relative('/vault', '/vault/sub/a.md')).toBe('sub/a.md')
    expect(relative('D:\\vault', 'D:\\vault\\sub\\a.md')).toBe('sub/a.md')
  })
  it('returns null outside the root', () => {
    expect(relative('/vault', '/other/a.md')).toBeNull()
    expect(relative('D:\\vault', 'E:\\vault\\a.md')).toBeNull()
  })
  it('does not treat a sibling with a shared prefix as inside', () => {
    expect(relative('/vault', '/vault-backup/a.md')).toBeNull()
    expect(relative('D:\\vault', 'D:\\vault-backup\\a.md')).toBeNull()
  })
})

describe('isWithinDir', () => {
  it('is false for the directory itself', () => {
    expect(isWithinDir('/vault', '/vault')).toBe(false)
    expect(isWithinDir('D:\\vault', 'D:\\vault\\')).toBe(false)
  })
  it('is true at any depth', () => {
    expect(isWithinDir('/vault/a/b.md', '/vault')).toBe(true)
    expect(isWithinDir('D:\\vault\\a\\b.md', 'D:\\vault')).toBe(true)
  })
})

describe('abbreviateHome', () => {
  it('abbreviates on both platforms', () => {
    expect(abbreviateHome('/Users/me/vault/a.md', '/Users/me')).toBe('~/vault/a.md')
    expect(abbreviateHome('C:\\Users\\me\\vault\\a.md', 'C:\\Users\\me')).toBe('~/vault/a.md')
  })
  it('leaves paths outside home alone', () => {
    expect(abbreviateHome('D:/vault/a.md', 'C:/Users/me')).toBe('D:/vault/a.md')
  })
  it('passes through when home is unknown', () => {
    expect(abbreviateHome('/x/y', null)).toBe('/x/y')
  })
})
