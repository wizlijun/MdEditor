import { describe, expect, it } from 'vitest'
import { TRACE_TASK_FILES, TRACE_TASK_ID } from './trace-template'

const at = (suffix: string) => {
  const hit = Object.entries(TRACE_TASK_FILES).find(([p]) => p.endsWith(suffix))
  expect(hit, suffix).toBeTruthy()
  return hit![1]
}

describe('trace-source 模板', () => {
  it('五个文件齐全且都在模板目录下', () => {
    const paths = Object.keys(TRACE_TASK_FILES)
    expect(paths).toHaveLength(5)
    for (const p of paths) expect(p.startsWith(`.notemd/agent-tasks/${TRACE_TASK_ID}/`)).toBe(true)
  })

  it('task.json 可解析,directive/okf_type/超时符合 spec', () => {
    const t = JSON.parse(at('/task.json'))
    expect(t.directive).toEqual(['溯源', 'trace'])
    expect(t.okf_type).toBe('Trace Report')
    expect(t.timeout_seconds).toBe(2700)
    expect(t.precheck).toBe('precheck.sh')
  })

  it('settings 放开 WebSearch/WebFetch/yt-dlp,写权限只有 traces/', () => {
    for (const f of ['/.claude/settings.json', '/.claude/settings.scoped.json']) {
      const s = JSON.parse(at(f))
      expect(s.permissions.allow).toContain('WebSearch')
      expect(s.permissions.allow).toContain('Bash(yt-dlp:*)')
      const writes = s.permissions.allow.filter(
        (a: string) => a.startsWith('Write(') || a.startsWith('Edit('),
      )
      expect(writes.length).toBeGreaterThan(0)
      for (const w of writes) expect(w).toContain('/traces/')
      expect(s.permissions.deny).toContain('Task')
    }
  })

  it('CLAUDE.md 含协议要件:输出行、材料目录、降级、缘起、红线', () => {
    const md = at('/CLAUDE.md')
    for (const must of ['输出:', 'traces/', 'yt-dlp', '未取到字幕', '缘起', 'Trace Material', '绝不'])
      expect(md, must).toContain(must)
    expect(md).toContain('${VAULT}') // 数组拼行没被 JS 求值
  })
})
