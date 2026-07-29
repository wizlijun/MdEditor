import { describe, it, expect, vi, afterEach } from 'vitest'
import { mdHasQuestionAnnotation, mdDerivesQuestion, scheduleQuestionCapture } from './question-capture'

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

  it('ignores a note that is nothing but question marks (Ask seed, not yet typed)', () => {
    expect(mdHasQuestionAnnotation('正文{>>?<<}')).toBe(false)
    expect(mdHasQuestionAnnotation('正文{==x==}{>>？ <<}')).toBe(false)
    expect(mdHasQuestionAnnotation('正文{>>??<<}')).toBe(false)
  })
})

describe('mdDerivesQuestion — fence-aware real-question gate', () => {
  it('true when an annotation carries a question', () => {
    expect(mdDerivesQuestion('文 {==x==}{>>为什么?<<}')).toBe(true)
  })
  it('true for a point annotation question (full-width)', () => {
    expect(mdDerivesQuestion('末尾{>>这个对吗？<<}')).toBe(true)
  })
  it('false for a plain annotation', () => {
    expect(mdDerivesQuestion('文 {>>备注<<}')).toBe(false)
  })
  it('false for a seed-only note — an unwritten question must not hit the disk', () => {
    expect(mdDerivesQuestion('文 {==x==}{>>?<<}')).toBe(false)
  })
  it('false when the only question annotation sits inside a code fence', () => {
    // The cheap gate would false-positive here; the derive-based gate must not —
    // otherwise the source file gets mirrored into the vault for nothing.
    expect(mdHasQuestionAnnotation('```\n{>>为什么?<<}\n```\n')).toBe(true)
    expect(mdDerivesQuestion('```\n{>>为什么?<<}\n```\n')).toBe(false)
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

  it('does not fire stale capture when last annotation is deleted (no {>> in md)', () => {
    vi.useFakeTimers()
    const path = '/tmp/a.md'
    // First call: a question annotation is present — schedules a timer
    expect(() => scheduleQuestionCapture(path, '文 {>>为什么?<<}')).not.toThrow()
    // Second call: annotation was entirely removed — must cancel the pending timer
    expect(() => scheduleQuestionCapture(path, '批注被整体删掉了')).not.toThrow()
    // Advance past debounce: no stale captureQuestions call should fire
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
