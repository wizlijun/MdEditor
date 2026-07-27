import { describe, it, expect } from 'vitest'
import { mdHasQuestionAnnotation } from './question-capture'

describe('mdHasQuestionAnnotation', () => {
  it('detects a question inside wrapped annotation', () => {
    expect(mdHasQuestionAnnotation('x {==原文==}{>>为什么?<<} y')).toBe(true)
  })
  it('detects a question inside point annotation (full-width)', () => {
    expect(mdHasQuestionAnnotation('末尾{>>这个对吗？<<}')).toBe(true)
  })
  it('ignores plain annotations and bare question marks', () => {
    expect(mdHasQuestionAnnotation('x {>>备注<<} 正文里的问号?')).toBe(false)
    expect(mdHasQuestionAnnotation('没有批注')).toBe(false)
  })
})
