// @vitest-environment happy-dom
//
// Renders the real Board component against adversarial store data. The board's
// three columns are fed from vault files that agents (and humans) write — the
// plugin must never let bad data kill the render. Regression: two archive
// files both contained id 2026-07-31-03 (two *different* decisions were given
// the same id on creation); the flattened archive list then blew up Svelte's
// keyed-each with `each_key_duplicate`, the render aborted mid-swap, and the
// window sat on "Loading…" forever (2026-08-17).
import { describe, it, expect, afterEach } from 'vitest'
import { mount, unmount, flushSync } from 'svelte'
import { state } from '../lib/store.svelte'
import { setLocale } from '../lib/strings'
import Board from './Board.svelte'
import type { OpenDecision, ArchivedDecision } from '../lib/model'
import type { CandidateFile } from '../lib/candidate'

const open = (id: string): OpenDecision => ({
  id,
  title: `t-${id}`,
  prediction: `p-${id}`,
  confidence: 0.75,
  'check-date': '2099-01-01',
  created: '2026-07-31',
  origin: 'agent',
  strikes: 0,
})

const archived = (id: string): ArchivedDecision => ({
  id,
  created: '2026-07-31',
  status: 'closed',
  prediction: `p-${id}`,
  confidence: 0.75,
  outcome: 'hit',
  'still-endorse': true,
  origin: 'agent',
})

const candidateFile = (fileDate: string, ids: string[]): CandidateFile => ({
  date: fileDate,
  fileDate,
  new_candidates: ids.map((id) => ({ id, title: `c-${id}`, prediction: `p-${id}`, confidence: 0.5, prediction_source: 'quoted' })),
  closures: [],
  edit_decisions: [],
})

let app: ReturnType<typeof mount> | null = null

function mountBoard(): HTMLElement {
  const target = document.createElement('div')
  document.body.appendChild(target)
  app = mount(Board, { target })
  flushSync()
  return target
}

afterEach(() => {
  if (app) {
    unmount(app)
    app = null
  }
  document.body.innerHTML = ''
  state.open = []
  state.candidates = []
  state.archived = []
  state.score = null
  state.loading = false
})

describe('Board renders without crashing on duplicate ids', () => {
  it('duplicate ids across archive files (the 2026-08-17 loading-forever bug)', () => {
    setLocale('zh')
    state.open = [open('2026-08-01-01')]
    state.candidates = []
    state.archived = [archived('2026-07-31-03'), archived('2026-07-31-04'), archived('2026-07-31-03')]
    const target = mountBoard()
    // All three archived entries render, including both bearers of the dup id.
    expect(target.textContent!.split('2026-07-31-03').length - 1).toBeGreaterThanOrEqual(2)
    expect(target.textContent).toContain('2026-07-31-04')
  })

  it('duplicate candidate ids across diary files', () => {
    setLocale('zh')
    state.open = []
    state.archived = []
    state.candidates = [candidateFile('2026-08-15', ['x-01']), candidateFile('2026-08-16', ['x-01'])]
    const target = mountBoard()
    expect(target.textContent).toContain('c-x-01')
  })

  it('duplicate open ids still render', () => {
    setLocale('zh')
    state.open = [open('dup-1'), open('dup-1')]
    state.candidates = []
    state.archived = []
    const target = mountBoard()
    expect(target.textContent).toContain('t-dup-1')
  })
})
