import { describe, expect, it } from 'vitest'
import YAML from 'yaml'
import { appendEvent, createIdeaSource, loadWorkspace, type VaultPort } from './repository'
import { NEXT_PATH } from './ledger'
import { placeEvent, relinkEvent, reopenEvent } from './events'
import type { CommitEvent, NextEvent, SettleEvent } from './model'

class MemoryVault implements VaultPort {
  files = new Map<string, string>()
  writes: string[] = []
  opened: string[] = []

  async exists(path: string) {
    const prefix = `${path}/`
    return { exists: this.files.has(path) || [...this.files.keys()].some((key) => key.startsWith(prefix)) }
  }

  async list(path: string) {
    const prefix = `${path}/`
    const names = [...this.files.keys()]
      .filter((key) => key.startsWith(prefix) && !key.slice(prefix.length).includes('/'))
      .map((key) => ({ name: key.slice(prefix.length), is_dir: false }))
      .sort((a, b) => a.name.localeCompare(b.name))
    return { entries: names }
  }

  async read(path: string) {
    const content = this.files.get(path)
    if (content === undefined) throw new Error(`missing: ${path}`)
    return { content }
  }

  async write(path: string, content: string) {
    this.files.set(path, content)
    this.writes.push(path)
    return { ok: true as const }
  }

  async open(path: string) {
    this.opened.push(path)
    return { ok: true }
  }
}

const idea = (created = '2026-08-29T01:00:00Z') => `---\ntype: Idea\ncreated: ${created}\n---\n# A useful idea\n`
const ideaWithoutCreated = '---\ntype: Idea\n---\n# A useful idea\n'
const creationTime = () => ({
  getFullYear: () => 2026,
  getMonth: () => 7,
  getDate: () => 30,
  getHours: () => 9,
  getMinutes: () => 5,
  toISOString: () => '2026-08-30T01:05:00.000Z',
}) as unknown as Date

const commit = (eventId = 'e1'): CommitEvent => ({
  at: '2026-08-29T02:00:00Z',
  event_id: eventId,
  idea_id: 'i1',
  action: 'commit',
  source: { path: 'inbox/ideas/a-idea.md', created: '2026-08-29T01:00:00Z' },
  commitment: 'Test the idea',
  next_action: 'Run one test',
  close_condition: 'Evidence exists',
})

describe('Next repository', () => {
  it('creates a new Idea in Idea Spark current directory without appending a lifecycle event', async () => {
    const vault = new MemoryVault()
    vault.files.set('.notemd/idea-spark.json', JSON.stringify({ ideaDir: 'capture/sparks' }))
    const created = await createIdeaSource('# 新念头', {
      now: creationTime,
    }, vault)

    expect(created.path).toBe('capture/sparks/2026-08-30-0905-idea.md')
    expect(vault.files.get(created.path)).toBe(
      '---\ntype: Idea\ncreated: 2026-08-30T01:05:00.000Z\n---\n# 新念头',
    )
    expect(vault.files.has(NEXT_PATH)).toBe(false)

    const workspace = await loadWorkspace(vault)
    expect(workspace.ideaDir).toBe('capture/sparks')
    expect(workspace.capture).toEqual([
      expect.objectContaining({ path: created.path, title: '新念头', state: 'capture' }),
    ])
    expect(workspace.ledger.events).toEqual([])
  })

  it('falls back to the default Idea directory and never overwrites an idea or proof slot', async () => {
    const vault = new MemoryVault()
    vault.files.set('.notemd/idea-spark.json', '{broken')
    vault.files.set('inbox/ideas/2026-08-30-0905-idea.md', idea())
    vault.files.set('inbox/ideas/2026-08-30-0905-2-idea.proof.md', '# Proof')

    const created = await createIdeaSource('另一个念头', {
      now: creationTime,
    }, vault)

    expect(created.path).toBe('inbox/ideas/2026-08-30-0905-3-idea.md')
    expect(vault.files.get('inbox/ideas/2026-08-30-0905-idea.md')).toBe(idea())
    expect(vault.files.get('inbox/ideas/2026-08-30-0905-2-idea.proof.md')).toBe('# Proof')
  })

  it('rejects blank ideas and a write that cannot be read back exactly', async () => {
    const vault = new MemoryVault()
    await expect(createIdeaSource('   ', {}, vault)).rejects.toThrow('blank')
    const originalRead = vault.read.bind(vault)
    vault.read = async (path: string) => {
      if (path.endsWith('-idea.md')) return { content: 'changed elsewhere' }
      return originalRead(path)
    }
    await expect(createIdeaSource('内容', {
      now: creationTime,
    }, vault)).rejects.toThrow('did not match')
  })

  it('discovers historic idea names while proof remains evidence only', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    vault.files.set('inbox/ideas/a-idea.proof.md', '# Proof')
    const workspace = await loadWorkspace(vault)
    expect(workspace.ideaDir).toBe('inbox/ideas')
    expect(workspace.capture).toHaveLength(1)
    expect(workspace.capture[0]).toMatchObject({ proofed: true, state: 'capture' })
    expect(workspace.closed).toHaveLength(0)
  })

  it('writes only the Next event document, then rebuilds WIP from it', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    const before = vault.files.get('inbox/ideas/a-idea.md')
    const loaded = await loadWorkspace(vault)
    const after = await appendEvent(loaded, commit(), {}, vault)
    expect(vault.writes).toEqual([NEXT_PATH, NEXT_PATH])
    expect(new Set(vault.writes)).toEqual(new Set([NEXT_PATH]))
    expect(vault.files.get('inbox/ideas/a-idea.md')).toBe(before)
    expect(after.wip).toHaveLength(1)
    expect(after.wip[0].projection).toMatchObject({ state: 'wip', next_action: 'Run one test' })
  })

  it('keeps idea and proof bytes unchanged across all six lifecycle actions', async () => {
    const vault = new MemoryVault()
    const originalIdea = idea()
    const originalProof = '# Evidence\n\nDo not change me.\n'
    const relinkedIdea = idea()
    const relinkedProof = '# Other evidence\n'
    vault.files.set('inbox/ideas/a-idea.md', originalIdea)
    vault.files.set('inbox/ideas/a-idea.proof.md', originalProof)
    vault.files.set('inbox/ideas/renamed-idea.md', relinkedIdea)
    vault.files.set('inbox/ideas/renamed-idea.proof.md', relinkedProof)
    let workspace = await loadWorkspace(vault)
    const events: NextEvent[] = [
      commit(),
      { at: '2026-08-29T03:00:00Z', event_id: 'e2', idea_id: 'i1', action: 'wait', waiting_for: 'review', review_at: '2026-09-01' },
      { at: '2026-08-29T04:00:00Z', event_id: 'e3', idea_id: 'i1', action: 'park', wake_trigger: '2026-10-01' },
      { at: '2026-08-29T05:00:00Z', event_id: 'e4', idea_id: 'i1', action: 'reopen' },
      { at: '2026-08-29T06:00:00Z', event_id: 'e5', idea_id: 'i1', action: 'settle', exit: { kind: 'done' } },
      { at: '2026-08-29T07:00:00Z', event_id: 'e6', idea_id: 'i1', action: 'relink', source: { path: 'inbox/ideas/renamed-idea.md', created: '2026-08-29T01:00:00Z' } },
    ]

    for (const event of events) {
      workspace = await appendEvent(workspace, event, {}, vault)
      expect(vault.files.get('inbox/ideas/a-idea.md')).toBe(originalIdea)
      expect(vault.files.get('inbox/ideas/a-idea.proof.md')).toBe(originalProof)
      expect(vault.files.get('inbox/ideas/renamed-idea.md')).toBe(relinkedIdea)
      expect(vault.files.get('inbox/ideas/renamed-idea.proof.md')).toBe(relinkedProof)
    }
  })

  it('never overwrites a malformed existing Next document', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    vault.files.set(NEXT_PATH, '---\ntype: Note\n---\n')
    const loaded = await loadWorkspace(vault)
    expect(loaded.readOnlyError).toBeTruthy()
    await expect(appendEvent(loaded, commit(), {}, vault)).rejects.toMatchObject({ code: 'read_only' })
    expect(vault.writes).toEqual([])
    expect(vault.files.get(NEXT_PATH)).toBe('---\ntype: Note\n---\n')
  })

  it('re-reads and revalidates external changes before appending', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    const loaded = await loadWorkspace(vault)
    const external = { ...commit('same-id'), commitment: 'Changed elsewhere' }
    await appendEvent(loaded, external, {}, vault)
    await expect(appendEvent(loaded, commit('same-id'), {}, vault)).rejects.toMatchObject({ code: 'invalid_event' })
    const raw = vault.files.get(NEXT_PATH)!
    const meta = YAML.parse(raw.match(/^---\n([\s\S]*?)\n---/)![1])
    expect(meta.events).toHaveLength(1)
    expect(meta.events[0].commitment).toBe('Changed elsewhere')
  })

  it('keeps a missing source as an orphan instead of closing it', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    let workspace = await loadWorkspace(vault)
    workspace = await appendEvent(workspace, commit(), {}, vault)
    vault.files.delete('inbox/ideas/a-idea.md')
    workspace = await loadWorkspace(vault)
    expect(workspace.wip[0]).toMatchObject({ orphan: true, state: 'wip' })
    expect(workspace.closed).toHaveLength(0)
  })

  it('keeps an orphan recoverable after reopen so it can still be explicitly settled', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    let workspace = await loadWorkspace(vault)
    workspace = await appendEvent(workspace, commit(), {}, vault)
    workspace = await appendEvent(workspace, {
      at: '2026-08-29T03:00:00Z',
      event_id: 'park-event',
      idea_id: 'i1',
      action: 'park',
      source: commit().source,
      wake_trigger: 'later',
    }, {}, vault)
    vault.files.delete('inbox/ideas/a-idea.md')
    workspace = await loadWorkspace(vault)

    const dormant = workspace.dormant[0]
    workspace = await appendEvent(workspace, reopenEvent(dormant, {
      now: () => '2026-08-29T04:00:00Z',
      id: () => 'reopen-event',
    }), {}, vault)
    const reopened = workspace.items.find((item) => item.idea_id === 'i1')!
    expect(reopened).toMatchObject({ state: 'capture', orphan: true })
    expect(workspace.capture).toEqual([])

    workspace = await appendEvent(workspace, placeEvent(reopened, {
      route: 'settle',
      exit: { kind: 'stopped', via: 'drop' },
      reason: 'Source no longer exists',
    }, {
      now: () => '2026-08-29T05:00:00Z',
      id: () => 'settle-event',
    }), {}, vault)
    expect(workspace.closed[0]).toMatchObject({ state: 'closed', orphan: true })
  })

  it('offers same-created rename candidates and relinks only after human selection', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    let workspace = await loadWorkspace(vault)
    workspace = await appendEvent(workspace, commit(), {}, vault)
    vault.files.delete('inbox/ideas/a-idea.md')
    vault.files.set('inbox/ideas/renamed-idea.md', idea())

    workspace = await loadWorkspace(vault)
    const orphan = workspace.wip[0]
    expect(orphan.orphan).toBe(true)
    expect(orphan.relinkMatch).toBe('created')
    expect(orphan.relinkCandidates.map((candidate) => candidate.path)).toEqual([
      'inbox/ideas/renamed-idea.md',
    ])

    const idFactory = { now: () => '2026-08-29T04:00:00Z', id: () => 'relink-event' }
    workspace = await appendEvent(
      workspace,
      relinkEvent(orphan, orphan.relinkCandidates[0], idFactory),
      {},
      vault,
    )
    expect(workspace.wip[0]).toMatchObject({
      orphan: false,
      path: 'inbox/ideas/renamed-idea.md',
    })
  })

  it('labels fallback relink choices as manual when creation time does not match', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    let workspace = await loadWorkspace(vault)
    workspace = await appendEvent(workspace, commit(), {}, vault)
    vault.files.delete('inbox/ideas/a-idea.md')
    vault.files.set('inbox/ideas/other-idea.md', idea('2026-08-30T01:00:00Z'))

    workspace = await loadWorkspace(vault)
    expect(workspace.wip[0]).toMatchObject({
      orphan: true,
      relinkMatch: 'manual',
    })
    expect(workspace.wip[0].relinkCandidates.map((candidate) => candidate.path)).toEqual([
      'inbox/ideas/other-idea.md',
    ])
  })

  it('offers a same-path file with missing creation time for manual relink instead of hiding it', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    let workspace = await loadWorkspace(vault)
    workspace = await appendEvent(workspace, commit(), {}, vault)
    vault.files.set('inbox/ideas/a-idea.md', ideaWithoutCreated)

    workspace = await loadWorkspace(vault)
    expect(workspace.wip[0]).toMatchObject({ orphan: true, relinkMatch: 'manual' })
    expect(workspace.wip[0].relinkCandidates.map((candidate) => candidate.path)).toEqual([
      'inbox/ideas/a-idea.md',
    ])
    expect(workspace.capture).toEqual([])
  })

  it('requires manual relink when a previously unmarked path gains a creation time', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', ideaWithoutCreated)
    let workspace = await loadWorkspace(vault)
    workspace = await appendEvent(workspace, {
      ...commit(),
      source: { path: 'inbox/ideas/a-idea.md' },
    }, {}, vault)
    vault.files.set('inbox/ideas/a-idea.md', idea('2026-08-30T01:00:00Z'))

    workspace = await loadWorkspace(vault)
    expect(workspace.wip[0]).toMatchObject({ orphan: true, relinkMatch: 'manual' })
    expect(workspace.wip[0].relinkCandidates.map((candidate) => candidate.path)).toEqual([
      'inbox/ideas/a-idea.md',
    ])
    expect(workspace.capture).toEqual([])
  })

  it('keeps old source directories when Idea Spark moves its inbox', async () => {
    const vault = new MemoryVault()
    vault.files.set('.notemd/idea-spark.json', JSON.stringify({ ideaDir: 'old/ideas' }))
    vault.files.set('old/ideas/a-idea.md', idea('2026-08-28T01:00:00Z'))
    let workspace = await loadWorkspace(vault)
    const oldCapture = workspace.capture[0]
    const ids = ['old-event', 'old-idea']
    workspace = await appendEvent(workspace, placeEvent(oldCapture, {
      route: 'park',
      wake_trigger: 'when needed',
    }, { now: () => '2026-08-29T01:00:00Z', id: () => ids.shift()! }), {}, vault)

    vault.files.set('.notemd/idea-spark.json', JSON.stringify({ ideaDir: 'new/ideas' }))
    vault.files.set('new/ideas/b-idea.md', idea('2026-08-29T01:00:00Z'))
    workspace = await loadWorkspace(vault)
    expect(workspace.sourceDirs).toEqual(['old/ideas', 'new/ideas'])
    expect(workspace.dormant[0].path).toBe('old/ideas/a-idea.md')
    expect(workspace.capture[0].path).toBe('new/ideas/b-idea.md')
  })

  it('snapshots source directories before the first placement so a later move loses no capture', async () => {
    const vault = new MemoryVault()
    vault.files.set('.notemd/idea-spark.json', JSON.stringify({ ideaDir: 'old/ideas' }))
    vault.files.set('old/ideas/a-idea.md', idea('2026-08-28T01:00:00Z'))

    let workspace = await loadWorkspace(vault)
    expect(workspace.ledger.events).toEqual([])
    expect(workspace.ledger.source_dirs).toEqual(['old/ideas'])
    expect(vault.files.has(NEXT_PATH)).toBe(true)

    vault.files.set('.notemd/idea-spark.json', JSON.stringify({ ideaDir: 'new/ideas' }))
    vault.files.set('new/ideas/b-idea.md', idea('2026-08-29T01:00:00Z'))
    workspace = await loadWorkspace(vault)

    expect(workspace.sourceDirs).toEqual(['old/ideas', 'new/ideas'])
    expect(workspace.capture.map((item) => item.path).sort()).toEqual([
      'new/ideas/b-idea.md',
      'old/ideas/a-idea.md',
    ])
    const persisted = YAML.parse(vault.files.get(NEXT_PATH)!.match(/^---\n([\s\S]*?)\n---/)![1])
    expect(persisted.source_dirs).toEqual(['old/ideas', 'new/ideas'])
    expect(persisted.events).toEqual([])
  })

  it('does not resurrect a historically claimed source as an impossible capture after relink', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    let workspace = await loadWorkspace(vault)
    workspace = await appendEvent(workspace, commit(), {}, vault)
    vault.files.set('inbox/ideas/renamed-idea.md', idea())
    const current = workspace.wip[0]
    workspace = await appendEvent(workspace, relinkEvent(current, {
      path: 'inbox/ideas/renamed-idea.md',
      title: 'A useful idea',
      created: '2026-08-29T01:00:00Z',
      proofed: false,
    }, { now: () => '2026-08-29T04:00:00Z', id: () => 'relink-event' }), {}, vault)

    expect(workspace.wip[0].path).toBe('inbox/ideas/renamed-idea.md')
    expect(workspace.capture.map((item) => item.path)).not.toContain('inbox/ideas/a-idea.md')
  })

  it('supports a valid close without requiring done to have a reason', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    let workspace = await loadWorkspace(vault)
    workspace = await appendEvent(workspace, commit(), {}, vault)
    const settle: SettleEvent = {
      at: '2026-08-29T03:00:00Z',
      event_id: 'e2',
      idea_id: 'i1',
      action: 'settle',
      source: commit().source,
      exit: { kind: 'done' },
    }
    workspace = await appendEvent(workspace, settle, {}, vault)
    expect(workspace.closed[0].projection).toMatchObject({ state: 'closed', exit: { kind: 'done' } })
  })
})
