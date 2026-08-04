// Verbatim template constants for the `idea-proof` claude-agent task, plus
// idempotent seeding into the vault. Content is asserted against the exact
// spec in .superpowers/sdd/2026-08-04-idea-spark-plugin/task-12-brief.md —
// these files are read by claude-agent's headless runner, so any drift is a
// silent behavior change for a task the user never edited.

import { describe, it, expect, vi } from 'vitest'
import { TASK_FILES, seedTaskTemplate, TASK_ID } from './task-template'
// `?raw` is Vite's built-in "import file contents as a string" suffix (typed
// by vite/client, already in this package's tsconfig `types`) — no node:fs
// needed, and it works identically under vitest since it shares Vite's
// module graph. Full-text fixture, copied verbatim from the brief's
// CLAUDE.md block, so a drift in any of the six protocol steps or the
// frontmatter field list fails loudly instead of slipping past a couple of
// `toContain` spot-checks.
import CLAUDE_MD_FIXTURE from './__fixtures__/idea-proof-claude.md?raw'

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

  it('CLAUDE.md matches the brief verbatim, full text (not just spot-checked substrings)', () => {
    expect(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/CLAUDE.md`]).toBe(CLAUDE_MD_FIXTURE)
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
    expect(sh).toContain('[ -n "$NOTEMD_NOTE" ] || { echo "缺少 idea 文件参数"; exit 1; }')
    expect(sh).toContain(
      '[ -s "$NOTEMD_NOTE" ] || { echo "idea 文件不存在或为空:$NOTEMD_NOTE"; exit 1; }',
    )
    expect(sh.trim().endsWith('exit 0')).toBe(true)
  })

  it('settings.json declares its full allow/deny lists (a dropped Write/Edit entry would silently brick .proof.md output)', () => {
    const settings = JSON.parse(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/.claude/settings.json`])
    expect(settings.permissions.allow).toEqual([
      'Read(${VAULT}/**)',
      'Write(${VAULT}/**/*.proof.md)',
      'Edit(${VAULT}/**/*.proof.md)',
      'WebSearch',
      'WebFetch',
    ])
    expect(settings.permissions.deny).toEqual(['Bash', 'Task'])
  })

  it('settings.scoped.json declares its full allow/deny lists', () => {
    const scoped = JSON.parse(TASK_FILES[`.notemd/agent-tasks/${TASK_ID}/.claude/settings.scoped.json`])
    expect(scoped.permissions.allow).toEqual([
      'Read(${NOTE})',
      'Read(${VAULT}/**)',
      'Write(${VAULT}/**/*.proof.md)',
      'Edit(${VAULT}/**/*.proof.md)',
      'WebSearch',
      'WebFetch',
    ])
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
