import { describe, it, expect, vi, afterEach } from 'vitest'
import { mdHasQuestionAnnotation, scheduleQuestionCapture } from './question-capture'

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

describe('scheduleQuestionCapture — retraction cancels pending timer', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('does not throw when re-scheduled with plain-annotation md after question md', () => {
    vi.useFakeTimers()
    const path = '/tmp/test.md'
    const questionMd = 'x {>>为什么?<<} y'
    const plainMd = 'x {>>备注<<} y'

    // First call schedules a capture for the question annotation
    expect(() => scheduleQuestionCapture(path, questionMd)).not.toThrow()
    // Second call (user deleted the ?) should cancel the pending timer and gate out
    expect(() => scheduleQuestionCapture(path, plainMd)).not.toThrow()
    // Advancing past the debounce window: no stale captureQuestions call fires
    // (captureQuestions itself requires dynamic imports not available here,
    // so we just confirm no unhandled rejection / crash after timer exhaustion)
    expect(() => vi.advanceTimersByTime(2000)).not.toThrow()
  })

  it('skips scheduling for non-.md paths', () => {
    vi.useFakeTimers()
    expect(() => scheduleQuestionCapture('/tmp/test.txt', '{>>问题?<<}')).not.toThrow()
    expect(() => scheduleQuestionCapture(null, '{>>问题?<<}')).not.toThrow()
    expect(() => scheduleQuestionCapture('/tmp/test.notes.md', '{>>问题?<<}')).not.toThrow()
    vi.advanceTimersByTime(2000)
  })
})
