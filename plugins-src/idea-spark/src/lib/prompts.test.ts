import { describe, expect, it } from 'vitest'
import { promptPathFor, TASKS_DIR } from './prompts'
import { TASK_ID, TASK_FILES } from './task-template'

describe('promptPathFor', () => {
  it('指向 task 模板目录下的 CLAUDE.md', () => {
    expect(promptPathFor('idea-proof')).toBe('.notemd/agent-tasks/idea-proof/CLAUDE.md')
    expect(promptPathFor('x')).toBe(`${TASKS_DIR}/x/CLAUDE.md`)
  })

  // 提示词入口打开的必须正好是播种写下的那个文件——两边各自拼路径,拼歪了
  // 用户会打开一个空白的新文件,还以为提示词丢了。
  it('与内置模板实际播种的路径逐字一致', () => {
    expect(Object.keys(TASK_FILES)).toContain(promptPathFor(TASK_ID))
  })
})
