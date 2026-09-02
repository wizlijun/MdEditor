import { describe, expect, it } from 'vitest'
import appSource from './App.svelte?raw'
import historySource from './components/HistoryList.svelte?raw'

describe('agent workspace layout contract', () => {
  it('keeps all variable sidebar content in one scrollport and settings outside it', () => {
    expect(appSource).toMatch(/<div class="sidebar-scroll">[\s\S]*?<\/div>\s*<button\s+type="button"\s+class="settings-entry"/)
    expect(appSource).toMatch(/\.sidebar-scroll\s*{[^}]*flex:\s*1;[^}]*min-height:\s*0;[^}]*overflow-y:\s*auto;/)
    expect(appSource).toMatch(/section\s*{[^}]*min-height:\s*0;[^}]*overflow:\s*hidden;/)
    expect(appSource).not.toMatch(/\.runs\s*{[^}]*overflow-y:/)
  })

  it('uses the same neutral surface tokens as the plugin market', () => {
    for (const token of ['--window-background', '--window-surface', '--card-surface', '--window-border', '--standard-accent']) {
      expect(appSource).toContain(token)
    }
  })

  it('splits history rows into readable primary and metadata lines', () => {
    expect(historySource).toContain('class="row-top"')
    expect(historySource).toContain('class="row-meta"')
    expect(historySource).toContain('aria-pressed={run.run_id === selectedId}')
  })
})
