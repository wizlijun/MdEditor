import { describe, it, expect } from 'vitest'
import { fileIcon, isImage, isText } from './vault-list'

describe('vault-list helpers', () => {
  it('returns correct icon by extension', () => {
    expect(fileIcon('md')).toBe('📝')
    expect(fileIcon('markdown')).toBe('📝')
    expect(fileIcon('html')).toBe('🌐')
    expect(fileIcon('htm')).toBe('🌐')
    expect(fileIcon('txt')).toBe('📄')
    expect(fileIcon('log')).toBe('📄')
    expect(fileIcon('png')).toBe('🖼️')
    expect(fileIcon('jpg')).toBe('🖼️')
    expect(fileIcon('webp')).toBe('🖼️')
    expect(fileIcon('canvas')).toBe('🗺️')
    expect(fileIcon('unknown')).toBe('📄')
  })

  it('isImage detects image extensions', () => {
    expect(isImage('png')).toBe(true)
    expect(isImage('jpg')).toBe(true)
    expect(isImage('md')).toBe(false)
  })

  it('isText detects text extensions', () => {
    expect(isText('md')).toBe(true)
    expect(isText('txt')).toBe(true)
    expect(isText('html')).toBe(true)
    expect(isText('canvas')).toBe(true)
    expect(isText('png')).toBe(false)
  })

  it('treats mdx as a readable text document', () => {
    expect(isText('mdx')).toBe(true)
    expect(fileIcon('mdx')).toBe('📝')
  })
})
