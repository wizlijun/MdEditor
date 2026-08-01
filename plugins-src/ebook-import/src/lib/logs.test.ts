import { describe, it, expect, beforeEach } from 'vitest'
import { describeLog } from './logs'
import { setLocale } from './strings'

describe('describeLog', () => {
  beforeEach(() => setLocale('zh'))

  it('localizes the pipeline lines and shows only the file name', () => {
    expect(describeLog('OCR: /Users/x/Books/deep dive.pdf')).toBe('OCR:deep dive.pdf')
    expect(describeLog('converting /Users/x/Books/a book.epub to htmlz')).toBe(
      '正在用 Calibre 转换 a book.epub…',
    )
  })

  it('localizes WeChat OCR page failures, keeping the reason', () => {
    expect(describeLog('page 12 failed: server said no')).toBe('第 12 页失败:server said no')
    expect(describeLog('failed pages: [3, 7]')).toBe('OCR 失败的页码:3, 7')
  })

  it('localizes the Baidu phases and its status word', () => {
    expect(describeLog('baidu ocr: requesting access token')).toBe('百度 OCR:正在获取访问令牌…')
    expect(describeLog('baidu ocr: submitting document')).toBe('百度 OCR:正在上传文档…')
    expect(describeLog('baidu ocr: downloading markdown')).toBe('百度 OCR:正在下载识别结果…')
    expect(describeLog('baidu ocr: running')).toBe('百度 OCR:识别中')
    expect(describeLog('baidu ocr: success')).toBe('百度 OCR:已完成')
  })

  it('passes an unrecognized line through verbatim — a log must not lose detail', () => {
    expect(describeLog('some future backend line')).toBe('some future backend line')
    expect(describeLog('')).toBe('')
  })

  it('falls back to English when the locale has no catalog', () => {
    setLocale('en')
    expect(describeLog('converting /x/y.epub to htmlz')).toBe('Converting y.epub with Calibre…')
    expect(describeLog('baidu ocr: pending')).toBe('Baidu OCR: queued')
  })
})
