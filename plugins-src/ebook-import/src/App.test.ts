import { mount, tick, unmount } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.svelte'
import type { NotemdBridge } from './lib/bridge'

describe('topic design Agent picker', () => {
  let app: ReturnType<typeof mount> | undefined

  afterEach(async () => {
    if (app) await unmount(app)
    app = undefined
    document.body.innerHTML = ''
    localStorage.clear()
  })

  it('lets topic design choose a supported Agent independently from AI reading', async () => {
    localStorage.setItem('notemd.agent.provider.ebook-import', 'notemd.claude-agent')
    const requests: Array<{ method: string; params: unknown }> = []
    const request = vi.fn(async (method: string, params?: unknown) => {
      requests.push({ method, params })
      if (method === 'host.agent.providers') {
        return {
          default: 'notemd.codex-agent',
          providers: [
            { id: 'notemd.claude-agent', name: 'Claude Agent', harness: { harness: 'Claude Code', ok: true } },
            { id: 'notemd.codex-agent', name: 'Codex Agent', harness: { harness: 'Codex', ok: true } },
            { id: 'notemd.deepseek-agent', name: 'DeepSeek Agent', harness: { harness: 'DeepSeek Harness', ok: true } },
          ],
        }
      }
      if (method === 'plugin.detect_env') return { ready: true, settings: {} }
      if (method === 'plugin.library_list') {
        return {
          books: [{ rel: 'ebooks/2026-09/Example', name: 'Example', month: '2026-09', summaries: [] }],
        }
      }
      if (method === 'plugin.topic_state') {
        return {
          revision: 'r1',
          catalog: {
            schema_version: 1,
            topics: [
              {
                id: 'general',
                label: 'General',
                description: 'General books',
                vocabulary: ['book'],
                index_file: 'general.index.md',
              },
            ],
          },
          counts: { general: 1 },
          unclassified_books: [],
          unknown_topic_books: [],
        }
      }
      if (method === 'plugin.topic_agent_start') return { job_id: 7 }
      return {}
    })
    window.notemd = {
      pluginId: 'notemd.ebook-import',
      locale: 'zh',
      theme: 'light',
      request,
      onMessage: () => {},
    } satisfies NotemdBridge

    app = mount(App, { target: document.body })
    await vi.waitFor(() => {
      expect(document.querySelector('.topic-actions button[aria-haspopup="menu"]')).not.toBeNull()
    })

    const topicPicker = document.querySelector<HTMLButtonElement>(
      '.topic-actions button[aria-haspopup="menu"]',
    )
    expect(topicPicker?.textContent).toContain('Codex')
    expect(document.querySelector('.topic-agent-status')?.textContent).toContain(
      '可能读取整个 Vault',
    )
    topicPicker?.click()
    await tick()

    const deepSeek = [...document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')]
      .find((button) => button.textContent?.includes('DeepSeek'))
    expect(deepSeek).toBeDefined()
    deepSeek?.click()
    await tick()

    expect(localStorage.getItem('notemd.agent.provider.ebook-topic-design')).toBe(
      'notemd.deepseek-agent',
    )
    expect(localStorage.getItem('notemd.agent.provider.ebook-import')).toBe(
      'notemd.claude-agent',
    )

    const designButton = [...document.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent?.trim() === 'AI 根据书库设计主题',
    )
    expect(designButton?.disabled).toBe(false)
    designButton?.click()
    await vi.waitFor(() => {
      expect(requests).toContainEqual({
        method: 'plugin.topic_agent_start',
        params: { harness: 'notemd.deepseek-agent' },
      })
    })
  })
})
