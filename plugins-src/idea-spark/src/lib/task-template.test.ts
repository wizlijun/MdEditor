// Verbatim template constants for the `idea-proof` claude-agent task, plus
// idempotent seeding into the vault. Content is asserted against the exact
// spec in .superpowers/sdd/2026-08-04-idea-spark-plugin/task-12-brief.md —
// these files are read by claude-agent's headless runner, so any drift is a
// silent behavior change for a task the user never edited.

import { describe, it, expect, vi } from 'vitest'
import { TASK_FILES, seedTaskTemplate, TASK_ID } from './task-template'

describe('idea-proof template', () => {
  it('contains the five files with parseable json and okf-frontmatter protocol', () => {
    const keys = Object.keys(TASK_FILES)
    expect(keys).toHaveLength(5)
    const task = JSON.parse(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/task.json`])
    expect(task.timeout_seconds).toBe(1800)
    expect(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/CLAUDE.md`]).toContain('type: Idea Proof')
    expect(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/CLAUDE.md`]).toContain('绝不修改 idea 原文')
    JSON.parse(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/.claude/settings.json`])
  })

  it('seed is idempotent: existing files are never overwritten', async () => {
    const write = vi.fn(); const exists = vi.fn().mockResolvedValue(true)
    await seedTaskTemplate({ exists, write })
    expect(write).not.toHaveBeenCalled()
    const write2 = vi.fn(); const exists2 = vi.fn().mockResolvedValue(false)
    await seedTaskTemplate({ exists: exists2, write: write2 })
    expect(write2).toHaveBeenCalledTimes(5)
  })

  it('has the five expected vault-relative keys', () => {
    expect(Object.keys(TASK_FILES).sort()).toEqual(
      [
        `.notemd/agent-tasks/${TASK_ID}/task.json`,
        `.notemd/agent-tasks/${TASK_ID}/CLAUDE.md`,
        `.notemd/agent-tasks/${TASK_ID}/precheck.sh`,
        `.notemd/agent-tasks/${TASK_ID}/.claude/settings.json`,
        `.notemd/agent-tasks/${TASK_ID}/.claude/settings.scoped.json`,
      ].sort(),
    )
  })

  it('task.json has verbatim name/description/prompt and precheck wiring', () => {
    const task = JSON.parse(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/task.json`])
    expect(task.name).toBe('Idea proof')
    expect(task.precheck).toBe('precheck.sh')
    expect(task.max_turns).toBe(80)
    expect(task.description).toContain('先找落差,再证伪,再最小验证')
    expect(task.prompt).toContain('<同名>.proof.md')
  })

  it('precheck.sh guards on NOTEMD_NOTE existence and non-emptiness', () => {
    const sh = TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/precheck.sh`]
    expect(sh.startsWith('#!/bin/sh\n')).toBe(true)
    expect(sh).toContain('$NOTEMD_NOTE')
    expect(sh.trim().endsWith('exit 0')).toBe(true)
  })

  it('settings.json and settings.scoped.json parse and allow WebSearch/WebFetch (idea-proof needs prior-art checks)', () => {
    const settings = JSON.parse(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/.claude/settings.json`])
    expect(settings.permissions.allow).toContain('WebSearch')
    expect(settings.permissions.allow).toContain('WebFetch')
    expect(settings.permissions.deny).toEqual(['Bash', 'Task'])

    const scoped = JSON.parse(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/.claude/settings.scoped.json`])
    expect(scoped.permissions.allow).toContain('Read(${NOTE})')
    expect(scoped.permissions.deny).toEqual(['Bash', 'Task'])
  })

  it('never writes a file that already exists, file by file', async () => {
    const seen: string[] = []
    const exists = vi.fn(async (p: string) => {
      seen.push(p)
      return p.endsWith('precheck.sh')
    })
    const write = vi.fn()
    await seedTaskTemplate({ exists, write })
    expect(seen.sort()).toEqual(Object.keys(TASK_FILES).sort())
    expect(write).toHaveBeenCalledTimes(4)
    expect(write).not.toHaveBeenCalledWith(
      expect.stringContaining('precheck.sh'),
      expect.anything(),
    )
  })
})
