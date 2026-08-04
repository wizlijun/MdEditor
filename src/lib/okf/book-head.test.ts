import { describe, it, expect } from 'vitest'
import { parse as parseYaml } from 'yaml'
// ebook-import 后端(Rust)写出的 book.md 头,由 bookconf.rs 的
// book_head_matches_the_shared_golden 钉住字节;这里钉住它的 OKF 合规性。
import golden from '../../../plugins-src/ebook-import/backend/tests/fixtures/book-head.md?raw'
// @ts-expect-error - plain-JS lint core shared with scripts/okf-lint.mjs
import { lintText } from '../../../scripts/okf-lint-core.mjs'

describe('ebook-import 写出的 book.md', () => {
  it('satisfies the OKF hard constraints', () => {
    expect(lintText('book.md', golden)).toEqual([])
  })

  it('keeps a hostile source path parseable and intact (§5.1)', () => {
    const fm = parseYaml(golden.slice(4, golden.indexOf('\n---\n', 3)))
    expect(fm.type).toBe('Book')
    expect(fm.sources[0].resource).toBe('/in/7 "powers".epub')
    expect(fm.title).toBe('7 Powers')
  })
})
