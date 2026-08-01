// Localization for the backend's log lines, which the UI shows verbatim in a
// row's detail view. Same approach as errors.ts and for the same reason: the
// backend emits its own English strings (they double as CLI output, where
// English is the convention), so the window translates them on the way in
// rather than teaching the backend a second catalog. An unrecognized line is
// shown as-is — a log must never lose information to a missing translation.

import { t } from './strings'

/** Backend paths are absolute; a log line only needs the file name. */
function baseName(p: string): string {
  const s = p.replace(/\/+$/, '')
  const i = s.lastIndexOf('/')
  return i >= 0 ? s.slice(i + 1) : s
}

/**
 * Translates one backend log line. Patterns mirror the exact `format!` strings
 * in pipeline.rs and ocr/{wechat,baidu}.rs; anything else falls through.
 */
export function describeLog(line: string): string {
  let m: RegExpMatchArray | null

  // pipeline.rs
  if ((m = line.match(/^OCR: (.+)$/))) {
    return t('log.ocrStart', { file: baseName(m[1]) })
  }
  if ((m = line.match(/^converting (.+) to htmlz$/))) {
    return t('log.converting', { file: baseName(m[1]) })
  }

  // ocr/wechat.rs
  if ((m = line.match(/^page (\d+) failed: (.+)$/))) {
    return t('log.pageFailed', { page: m[1], reason: m[2] })
  }
  if ((m = line.match(/^failed pages: \[(.*)\]$/))) {
    return t('log.failedPages', { pages: m[1] })
  }

  // ocr/baidu.rs
  if (line === 'baidu ocr: requesting access token') return t('log.baiduToken')
  if (line === 'baidu ocr: submitting document') return t('log.baiduSubmit')
  if (line === 'baidu ocr: downloading markdown') return t('log.baiduDownload')
  if ((m = line.match(/^baidu ocr: (pending|running|success|failed)$/))) {
    return t('log.baiduStatus', { status: t(`log.baiduStatus.${m[1]}` as never) })
  }

  return line
}
