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

  it('stages AI batch classification edits and applies them once after confirmation', async () => {
    const requests: Array<{ method: string; params: any }> = []
    let push: ((payload: unknown) => void) | undefined
    const request = vi.fn(async (method: string, params?: unknown) => {
      requests.push({ method, params })
      if (method === 'host.agent.providers') {
        return {
          default: 'notemd.codex-agent',
          providers: [
            { id: 'notemd.codex-agent', name: 'Codex Agent', harness: { harness: 'Codex', ok: true } },
          ],
        }
      }
      if (method === 'plugin.detect_env') return { ready: true, settings: {} }
      if (method === 'plugin.library_list') {
        return {
          books: [
            { rel: 'ebooks/2026-09/A', name: 'Book A', month: '2026-09', summaries: [] },
            { rel: 'ebooks/2026-09/B', name: 'Book B', month: '2026-09', summaries: [] },
          ],
        }
      }
      if (method === 'plugin.topic_state') {
        return {
          revision: 'sha256:catalog',
          catalog: {
            schema_version: 1,
            topics: [
              {
                id: 'engineering',
                label: 'Engineering',
                description: 'Build systems',
                index_file: 'engineering.index.md',
                vocabulary: [
                  { term: 'architecture', description: 'system structure' },
                  { term: 'reliability', description: 'correct service' },
                ],
              },
              {
                id: 'business',
                label: 'Business',
                description: 'Build companies',
                index_file: 'business.index.md',
                vocabulary: [
                  { term: 'strategy', description: 'competitive choices' },
                  { term: 'market', description: 'buyers and sellers' },
                ],
              },
            ],
          },
          counts: {},
          unclassified_books: ['2026-09/A', '2026-09/B'],
          unknown_topic_books: [],
        }
      }
      if (method === 'plugin.topic_classification_start') return { job_id: 9, book_count: 2 }
      if (method === 'plugin.topic_classification_apply') return { ok: true }
      return {}
    })
    window.notemd = {
      pluginId: 'notemd.ebook-import',
      locale: 'zh',
      theme: 'light',
      request,
      onMessage: (callback) => { push = callback },
    } satisfies NotemdBridge

    app = mount(App, { target: document.body })
    const classify = await vi.waitFor(() => {
      const button = [...document.querySelectorAll<HTMLButtonElement>('button')].find(
        (candidate) => candidate.textContent?.includes('AI 批量分类 2 本'),
      )
      expect(button).toBeDefined()
      return button!
    })
    classify.click()
    await vi.waitFor(() => {
      expect(requests).toContainEqual({
        method: 'plugin.topic_classification_start',
        params: { harness: 'notemd.codex-agent' },
      })
    })

    push?.({
      type: 'topic_classification',
      job_id: 9,
      event: 'done',
      proposal: {
        schema_version: 1,
        inventory_sha256: 'inventory',
        catalog_revision: 'sha256:catalog',
        assignments: [
          { book: '2026-09/A', topic_id: 'engineering' },
          { book: '2026-09/B', topic_id: 'business' },
        ],
      },
    })
    await tick()

    const selects = [...document.querySelectorAll<HTMLSelectElement>('[role="dialog"] select')]
    expect(selects).toHaveLength(2)
    selects[0].value = 'business'
    selects[0].dispatchEvent(new Event('change', { bubbles: true }))
    await tick()
    expect(requests.filter(({ method }) => method === 'plugin.topic_classification_apply')).toHaveLength(0)
    expect(requests.filter(({ method }) => method === 'plugin.topic_assign')).toHaveLength(0)

    const confirm = [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find(
      (button) => button.textContent?.includes('确认并应用 2 本'),
    )
    expect(confirm).toBeDefined()
    confirm?.click()
    await vi.waitFor(() => {
      const applies = requests.filter(({ method }) => method === 'plugin.topic_classification_apply')
      expect(applies).toHaveLength(1)
      expect(applies[0].params.proposal.assignments).toEqual([
        { book: '2026-09/A', topic_id: 'business' },
        { book: '2026-09/B', topic_id: 'business' },
      ])
    })
    await vi.waitFor(() => expect(document.querySelector('[role="dialog"]')).toBeNull())
  })
})
