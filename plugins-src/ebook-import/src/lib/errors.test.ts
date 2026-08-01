import { describe, it, expect } from 'vitest'
import { errorKey, describeError } from './errors'
import { setLocale, t } from './strings'

// Exact wording the Rust backend emits (plugins-src/ebook-import/backend/src),
// grepped from plugin.rs / pipeline.rs / calibre.rs / settings.rs / ocr/*.rs.
// Pinning these here means a backend wording change that breaks the mapping
// fails this test instead of silently falling back to raw English.
describe('errorKey', () => {
  it('maps "no vault configured"', () => {
    expect(errorKey('no vault configured')).toBe('err.noVault')
  })

  it('maps "calibre not found"', () => {
    expect(errorKey('calibre not found')).toBe('err.calibreMissing')
  })

  it('maps ebook-convert timeout', () => {
    expect(errorKey('ebook-convert timed out after 300s')).toBe('err.calibreTimeout')
  })

  it('maps ebook-convert exit-status and launch failures', () => {
    expect(errorKey('ebook-convert exited with status 1: some stderr excerpt')).toBe('err.calibreFailed')
    expect(errorKey('failed to launch ebook-convert: No such file or directory (os error 2)')).toBe(
      'err.calibreFailed',
    )
  })

  it('maps a bad ebooks_root', () => {
    expect(errorKey('ebooks_root must be a vault-relative path')).toBe('err.badRoot')
  })

  it('maps a missing derivable title', () => {
    expect(errorKey('could not derive a directory name for this book')).toBe('err.noTitle')
  })

  it('maps OCR-only-PDF', () => {
    expect(errorKey('OCR only supports PDF input, got .epub')).toBe('err.ocrOnlyPdf')
  })

  it('maps empty OCR output', () => {
    expect(errorKey('OCR produced no content for any of 3 page(s)')).toBe('err.ocrEmpty')
  })

  it('maps an unreachable OCR service', () => {
    expect(errorKey('ocr service unreachable: connection refused')).toBe('err.ocrUnreachable')
  })

  it('maps Baidu OCR failures', () => {
    expect(errorKey('Baidu OCR API error 17: Open api daily request limit reached')).toBe('err.baiduFailed')
    expect(errorKey('baidu ocr: task abc123 failed')).toBe('err.baiduFailed')
    expect(errorKey('baidu ocr: task reported success without markdown_url')).toBe('err.baiduFailed')
  })

  it('maps an unsupported input extension', () => {
    expect(errorKey("unsupported file extension '.txt' (expected one of: epub, pdf, docx)")).toBe(
      'err.unsupportedType',
    )
  })

  it('returns null for a string it does not recognize', () => {
    expect(errorKey('create work dir /tmp/x: permission denied')).toBeNull()
    expect(errorKey('')).toBeNull()
  })
})

describe('describeError', () => {
  it('returns the localized text for a matched string', () => {
    setLocale('en')
    expect(describeError('calibre not found').text).toBe(t('err.calibreMissing'))
    setLocale('zh')
    expect(describeError('calibre not found').text).toBe(t('err.calibreMissing'))
    setLocale('en')
  })

  it('keeps the raw message as detail when the match carries extra info', () => {
    const raw = 'ebook-convert timed out after 300s'
    const d = describeError(raw)
    expect(d.text).toBe(t('err.calibreTimeout'))
    expect(d.detail).toBe(raw)
  })

  it('omits detail when the matched string carries no extra info', () => {
    const d = describeError('no vault configured')
    expect(d.text).toBe(t('err.noVault'))
    expect(d.detail).toBeUndefined()
  })

  it('returns the raw message verbatim, with no detail, when unmatched', () => {
    const raw = 'create work dir /tmp/x: permission denied'
    expect(describeError(raw)).toEqual({ text: raw })
  })
})
