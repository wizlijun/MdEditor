import { describe, expect, it } from 'vitest'
import {
  buildTaskDocument,
  DEFAULT_TASK_DIR,
  findTaskByDedupeKey,
  isTaskFileName,
  isTaskPath,
  parseTaskSource,
  TaskSourceError,
  taskIdentityHint,
  taskSlug,
  timestampTaskFileName,
  type TaskSourceErrorCode,
} from './task-source'

const TASK_ID = '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8'

function minimalDocument(overrides = ''): string {
  return `---
type: Task
title: 提交 TestFlight 构建
created: 2026-09-01T03:20:00Z
task:
  version: 1
  id: ${TASK_ID}
${overrides}---
确认签名环境变量。`
}

function expectTaskError(fn: () => unknown, code: TaskSourceErrorCode): void {
  try {
    fn()
    throw new Error(`expected ${code}`)
  } catch (error) {
    expect(error).toBeInstanceOf(TaskSourceError)
    expect((error as TaskSourceError).code).toBe(code)
  }
}

describe('Task source contract', () => {
  it('only accepts direct inbox/tasks files with the strict -task.md suffix', () => {
    expect(DEFAULT_TASK_DIR).toBe('inbox/tasks')
    expect(isTaskFileName('2026-09-01-1120-submit-task.md')).toBe(true)
    expect(isTaskFileName('submit.task.md')).toBe(false)
    expect(isTaskFileName('submit-task.markdown')).toBe(false)
    expect(isTaskPath('inbox/tasks/2026-09-01-1120-submit-task.md')).toBe(true)
    expect(isTaskPath('inbox/tasks/nested/submit-task.md')).toBe(false)
    expect(isTaskPath('other/submit-task.md')).toBe(false)
  })

  it('reads the required Task fields and preserves the body verbatim', () => {
    const body = '第一行\r\n\r\n- [ ] 子项\r\n---\r\n最后一行'
    const source = parseTaskSource(
      'inbox/tasks/submit-task.md',
      `---\r\ntype: Task\r\ntitle: "  提交 TestFlight 构建  "\r\ncreated: 2026-09-01T03:20:00Z\r\ntask:\r\n  version: 1\r\n  id: ${TASK_ID}\r\n---\r\n${body}`,
    )

    expect(source).toMatchObject({
      path: 'inbox/tasks/submit-task.md',
      title: '提交 TestFlight 构建',
      created: '2026-09-01T03:20:00Z',
      task: { version: 1, id: TASK_ID },
    })
    expect(source.body).toBe(body)
  })

  it('reads optional Task, generated, and source metadata', () => {
    const source = parseTaskSource(
      'inbox/tasks/generated-task.md',
      `---
type: Task
title: 提交 TestFlight 构建
description: 今天还没有上传验证。
created: 2026-09-01T03:20:00Z
task:
  version: 1
  id: ${TASK_ID}
  due: "2026-09-02"
  done_when: 构建出现在 TestFlight
  dedupe_key: daily-summary/v1:2026-09-01:testflight-upload
generated:
  by: daily-summary-agent/1
  at: 2026-09-01T03:20:00Z
sources:
  - id: daily-note
    resource: /dailynote/2026/2026-09-01.note.md
    title: 2026-09-01 日记
---
上传前检查 .env。`,
    )

    expect(source.description).toBe('今天还没有上传验证。')
    expect(source.task).toEqual({
      version: 1,
      id: TASK_ID,
      due: '2026-09-02',
      done_when: '构建出现在 TestFlight',
      dedupe_key: 'daily-summary/v1:2026-09-01:testflight-upload',
    })
    expect(source.generated).toEqual({ by: 'daily-summary-agent/1', at: '2026-09-01T03:20:00Z' })
    expect(source.sources).toEqual([{
      id: 'daily-note',
      resource: '/dailynote/2026/2026-09-01.note.md',
      title: '2026-09-01 日记',
    }])
  })

  it('ignores unknown fields while retaining the complete parsed frontmatter', () => {
    const source = parseTaskSource(
      'inbox/tasks/future-task.md',
      `---
type: Task
title: Future-compatible task
created: 2026-09-01T03:20:00Z
future_top: keep-me
task:
  version: 1
  id: ${TASK_ID}
  dedupe_key: future-agent/v1:future-task
  future_nested:
    enabled: true
generated:
  by: daily-summary-agent/1
  at: 2026-09-01T03:20:00Z
  model: future-model
sources:
  - resource: /daily.md
    future_source: keep-me-too
---
body`,
    )

    expect(source.frontmatter.future_top).toBe('keep-me')
    expect((source.frontmatter.task as Record<string, unknown>).future_nested).toEqual({ enabled: true })
    expect((source.frontmatter.generated as Record<string, unknown>).model).toBe('future-model')
    expect((source.frontmatter.sources as Array<Record<string, unknown>>)[0]?.future_source).toBe('keep-me-too')
  })

  it('builds a valid document, quotes YAML 1.1 dates, and preserves its body', () => {
    const body = '上传前确认 `.env`。\n\n- [ ] 安装验证'
    const document = buildTaskDocument({
      title: '  提交 TestFlight 构建  ',
      description: '  今天还没有上传验证。  ',
      created: '2026-09-01T03:20:00Z',
      task: {
        version: 1,
        id: TASK_ID,
        due: '2026-09-02',
        done_when: '构建出现在 TestFlight',
      },
      body,
    })

    expect(document).toContain('due: "2026-09-02"')
    const parsed = parseTaskSource('inbox/tasks/built-task.md', document)
    expect(parsed.title).toBe('提交 TestFlight 构建')
    expect(parsed.description).toBe('今天还没有上传验证。')
    expect(parsed.body).toBe(body)
  })

  it('rejects malformed frontmatter and every invalid required field explicitly', () => {
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', '# no frontmatter'),
      'missing-frontmatter',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', '---\ntype: [\n---\nbody'),
      'invalid-frontmatter',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a.md', minimalDocument()),
      'invalid-path',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument().replace('type: Task', 'type: Idea')),
      'invalid-type',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument().replace('title: 提交 TestFlight 构建', 'title: "  "')),
      'invalid-title',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument().replace('2026-09-01T03:20:00Z', '2026-09-01')),
      'invalid-created',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument().replace('version: 1', 'version: 2')),
      'unsupported-version',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument().replace(TASK_ID, 'not-a-uuid')),
      'invalid-task-id',
    )
  })

  it('rejects invalid optional known fields rather than silently dropping them', () => {
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument('  due: "2026-02-30"\n')),
      'invalid-due',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument('  due: 2026-09-02\n')),
      'invalid-due',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument('  done_when: []\n')),
      'invalid-done-when',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument('  dedupe_key: "  "\n')),
      'invalid-dedupe-key',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument('generated:\n  by: missing-version\n  at: 2026-09-01T03:20:00Z\n')),
      'invalid-generated',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument('sources:\n  - id: missing-resource\n')),
      'invalid-sources',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument('generated:\n  by: daily-summary-agent/1\n  at: 2026-09-01T03:20:00Z\nsources:\n  - resource: /daily.md\n')),
      'invalid-dedupe-key',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument('  dedupe_key: daily-summary/v1:2026-09-01:a\ngenerated:\n  by: daily-summary-agent/1\n  at: 2026-09-01T03:20:00Z\n')),
      'invalid-sources',
    )
    expectTaskError(
      () => parseTaskSource('inbox/tasks/a-task.md', minimalDocument('  dedupe_key: daily-summary/v1:2026-09-01:a\ngenerated:\n  by: daily-summary-agent/1\n  at: 2026-09-01T03:20:00Z\nsources: []\n')),
      'invalid-sources',
    )
  })

  it('extracts only a safe identity hint from an otherwise unsupported Task', () => {
    const future = minimalDocument().replace('version: 1', 'version: 2')
    expect(taskIdentityHint(future)).toEqual({
      id: TASK_ID,
      title: '提交 TestFlight 构建',
    })
    expect(taskIdentityHint('not yaml')).toEqual({})
  })

  it('normalizes a Unicode title into a traversal-safe, bounded slug', () => {
    expect(taskSlug('  ＡＢＣ / 测试\\\u0000 <draft>  ')).toBe('abc-测试-draft')
    expect(taskSlug('../../\\\u0000')).toBe('task')
    expect([...taskSlug('任'.repeat(80))]).toHaveLength(48)
  })

  it('uses local time and increments only the collision suffix', () => {
    const at = new Date(2026, 8, 1, 9, 5)
    expect(timestampTaskFileName(at, '提交 TestFlight', new Set())).toBe(
      '2026-09-01-0905-提交-testflight-task.md',
    )
    expect(timestampTaskFileName(at, '提交 TestFlight', new Set([
      '2026-09-01-0905-提交-testflight-task.md',
      '2026-09-01-0905-提交-testflight-2-task.md',
    ]))).toBe('2026-09-01-0905-提交-testflight-3-task.md')
  })

  it('finds automatic tasks by exact dedupe key only', () => {
    const automatic = parseTaskSource(
      'inbox/tasks/generated-task.md',
      minimalDocument('  dedupe_key: daily-summary/v1:2026-09-01:upload\n'),
    )
    const manual = parseTaskSource('inbox/tasks/manual-task.md', minimalDocument().replace(TASK_ID, '57691a0e-32ed-48bf-829f-b1377d53db93'))

    expect(findTaskByDedupeKey([manual, automatic], 'daily-summary/v1:2026-09-01:upload')).toBe(automatic)
    expect(findTaskByDedupeKey([automatic], '2026-09-01:upload')).toBeUndefined()
  })
})
