import { describe, expect, it } from 'vitest'
import appSource from './App.svelte?raw'
import historySource from './components/HistoryList.svelte?raw'
import taskSource from './components/TaskList.svelte?raw'

describe('agent workspace layout contract', () => {
  it('keeps all variable sidebar content in one scrollport and settings outside it', () => {
    expect(appSource).toMatch(/<div class="sidebar-scroll">[\s\S]*?<\/div>\s*<button\s+type="button"\s+class="settings-entry"/)
    expect(appSource).toMatch(/\.sidebar-scroll\s*{[^}]*flex:\s*1;[^}]*min-height:\s*0;[^}]*overflow-y:\s*auto;/)
    expect(appSource).toMatch(/section\s*{[^}]*min-height:\s*0;[^}]*overflow:\s*hidden;/)
    expect(appSource).not.toMatch(/\.runs\s*{[^}]*overflow-y:/)
    expect(appSource.indexOf("tr('history.title')")).toBeLessThan(appSource.indexOf("tr('tasks.title')"))
  })

  it('uses the same neutral surface tokens as the plugin market', () => {
    for (const token of ['--window-background', '--window-surface', '--card-surface', '--window-border', '--standard-accent']) {
      expect(appSource).toContain(token)
    }
  })

  it('splits history rows into readable primary and metadata lines', () => {
    expect(historySource).toContain('class="row-top"')
    expect(historySource).toContain('class="row-meta"')
    expect(historySource).toContain("aria-current={run.run_id === selectedId ? 'page' : undefined}")
    expect(historySource).toContain('runs.slice(0, visibleCount)')
    expect(historySource).toContain('<span class="task">{run.task}</span>')
    expect(historySource).not.toContain('scopeKey')
    expect(appSource).not.toContain('allTasks')
    expect(appSource).not.toContain('class="scope"')
    expect(appSource).toContain("request('history.list', {})")
  })

  it('groups tasks into accessible disclosures that start collapsed', () => {
    expect(taskSource).toContain('let expanded = $state(new Set<string>())')
    expect(taskSource).toContain('aria-expanded={expanded.has(group.id)}')
    expect(taskSource).toContain('aria-controls={panelId}')
  })
})
