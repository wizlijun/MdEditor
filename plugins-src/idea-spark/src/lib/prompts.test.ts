import { describe, expect, it } from 'vitest'
import { promptEntries, promptPathFor, TASKS_DIR } from './prompts'
import { TASK_ID, TASK_FILES } from './task-template'
import { TRACE_TASK_ID, TRACE_TASK_FILES } from './trace-template'

describe('promptPathFor', () => {
  it('指向 task 模板目录下的 CLAUDE.md', () => {
    expect(promptPathFor('idea-proof')).toBe('.notemd/agent-tasks/idea-proof/CLAUDE.md')
    expect(promptPathFor('x')).toBe(`${TASKS_DIR}/x/CLAUDE.md`)
  })

  // 提示词入口打开的必须正好是播种写下的那个文件——两边各自拼路径,拼歪了
  // 用户会打开一个空白的新文件,还以为提示词丢了。
  it('与两个内置模板实际播种的路径逐字一致', () => {
    expect(Object.keys(TASK_FILES)).toContain(promptPathFor(TASK_ID))
    expect(Object.keys(TRACE_TASK_FILES)).toContain(promptPathFor(TRACE_TASK_ID))
  })
})

describe('promptEntries', () => {
  it('主任务排第一,指令随后并带 / 前缀', () => {
    const out = promptEntries('idea-proof', '论证想法', [
      { taskId: 'trace-source', display: '溯源' },
    ])
    expect(out).toEqual([
      { taskId: 'idea-proof', label: '论证想法' },
      { taskId: 'trace-source', label: '/溯源' },
    ])
  })

  it('没有指令时也至少给出主任务一行', () => {
    expect(promptEntries('idea-proof', '论证想法', [])).toEqual([
      { taskId: 'idea-proof', label: '论证想法' },
    ])
  })

  it('同一个 taskId 只列一行(主任务将来也带 directive 时)', () => {
    const out = promptEntries('idea-proof', '论证想法', [
      { taskId: 'idea-proof', display: '论证' },
      { taskId: 'trace-source', display: '溯源' },
      { taskId: 'trace-source', display: 'trace' },
    ])
    expect(out.map((e) => e.taskId)).toEqual(['idea-proof', 'trace-source'])
    expect(out[0].label).toBe('论证想法')
  })
})
