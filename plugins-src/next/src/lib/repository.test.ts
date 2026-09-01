import { describe, expect, it } from 'vitest'
import YAML from 'yaml'
import { appendEvent, createIdeaSource, createTaskSource, itemSearchText, loadWorkspace, updateItemPlanningMetadata, type VaultPort } from './repository'
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

  async rename(from: string, to: string) {
    const content = this.files.get(from)
    if (content === undefined) throw new Error(`missing: ${from}`)
    if (this.files.has(to)) throw new Error(`exists: ${to}`)
    this.files.delete(from)
    this.files.set(to, content)
    return { ok: true as const }
  }

  async remove(path: string) {
    this.files.delete(path)
    return { ok: true as const }
  }

  async open(path: string) {
    this.opened.push(path)
    return { ok: true }
  }
}

const idea = (created = '2026-08-29T01:00:00Z') => `---\ntype: Idea\ncreated: ${created}\n---\n# A useful idea\n`
const ideaWithoutCreated = '---\ntype: Idea\n---\n# A useful idea\n'
const TASK_ID = '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8'
const TASK_ID_2 = '93d3d6e2-dcc0-4ec0-9869-37cf130a0964'
const task = (id = TASK_ID, options: { title?: string; dedupeKey?: string; project?: string } = {}) => `---
type: Task
title: ${options.title ?? '提交 TestFlight 构建'}
created: 2026-09-01T03:20:00Z
task:
  version: 1
  id: ${id}
${options.project ? `  project: ${options.project}\n` : ''}
${options.dedupeKey ? `  dedupe_key: ${options.dedupeKey}\n` : ''}---
确认签名环境变量。`
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
  it('preserves the configured WIP limit across a verified append and reload', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    let workspace = await loadWorkspace(vault, { wipLimit: 1 })

    workspace = await appendEvent(workspace, commit(), {}, vault)

    expect(workspace.projection).toMatchObject({
      wipCount: 1,
      wipLimit: 1,
      wipAtLimit: true,
      wipExceeded: false,
    })
  })

  it('discovers valid Task files in Inbox without upgrading a v1 ledger', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/tasks/submit-task.md', task(TASK_ID, { project: 'NoteMD' }))

    const workspace = await loadWorkspace(vault)

    expect(workspace.taskDir).toBe('inbox/tasks')
    expect(workspace.taskSources).toHaveLength(1)
    expect(workspace.capture).toEqual([
      expect.objectContaining({
        kind: 'task',
        item_id: TASK_ID,
        path: 'inbox/tasks/submit-task.md',
        title: '提交 TestFlight 构建',
        task: { version: 1, id: TASK_ID, project: 'NoteMD' },
        state: 'capture',
      }),
    ])
    expect(itemSearchText(workspace.capture[0])).toContain('notemd')
    expect(workspace.ledger.version).toBe(1)
  })

  it('keeps duplicate Task ids and dedupe keys out of normal lanes and exposes repair cards', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/tasks/id-a-task.md', task(TASK_ID, { title: 'ID A' }))
    vault.files.set('inbox/tasks/id-b-task.md', task(TASK_ID, { title: 'ID B' }))
    vault.files.set('inbox/tasks/key-a-task.md', task(TASK_ID_2, { title: 'Key A', dedupeKey: 'daily/v1:same' }))
    vault.files.set('inbox/tasks/key-b-task.md', task('4d36be4c-4f41-4050-8f34-d636b51aa341', { title: 'Key B', dedupeKey: 'daily/v1:same' }))

    const workspace = await loadWorkspace(vault)

    expect(workspace.capture).toEqual([])
    expect(workspace.unsupported).toHaveLength(4)
    expect(workspace.unsupported.every((item) => item.kind === 'task' && item.repairReason)).toBe(true)
    expect(workspace.scanErrors).toEqual(expect.arrayContaining([
      expect.stringContaining(`duplicate task.id ${TASK_ID}`),
      expect.stringContaining('duplicate task.dedupe_key daily/v1:same'),
    ]))
  })

  it('quarantines a placed Task when its stable id later becomes ambiguous', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/tasks/original-task.md', task())
    let workspace = await loadWorkspace(vault)
    workspace = await appendEvent(workspace, placeEvent(workspace.capture[0], {
      route: 'commit',
      commitment: '提交构建',
      next_action: '运行发布脚本',
      close_condition: '构建可安装',
    }, {
      now: () => '2026-09-01T03:30:00Z',
      id: () => 'task-event',
    }), {}, vault)
    expect(workspace.wip).toHaveLength(1)

    vault.files.set('inbox/tasks/duplicate-task.md', task(TASK_ID, { title: '重复身份' }))
    workspace = await loadWorkspace(vault)

    expect(workspace.wip).toEqual([])
    expect(workspace.unsupported).toHaveLength(2)
    expect(workspace.unsupported.every((item) => item.item_id === TASK_ID)).toBe(true)
  })

  it('keeps an unreadable Task file visible in the repair area', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/tasks/future-task.md', task().replace('version: 1', 'version: 2'))

    const workspace = await loadWorkspace(vault)

    expect(workspace.capture).toEqual([])
    expect(workspace.unsupported).toEqual([
      expect.objectContaining({
        kind: 'task',
        title: '提交 TestFlight 构建',
        path: 'inbox/tasks/future-task.md',
        repairReason: expect.stringContaining('not supported'),
      }),
    ])
    expect(workspace.scanErrors[0]).toContain('future-task.md')
  })

  it('quarantines a placed Task when its source moves to an unsupported schema', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/tasks/future-task.md', task())
    let workspace = await loadWorkspace(vault)
    workspace = await appendEvent(workspace, placeEvent(workspace.capture[0], {
      route: 'commit',
      commitment: '提交构建',
      next_action: '运行发布脚本',
      close_condition: '构建可安装',
    }, {
      now: () => '2026-09-01T03:30:00Z',
      id: () => 'task-event',
    }), {}, vault)
    vault.files.set('inbox/tasks/future-task.md', task().replace('version: 1', 'version: 2'))

    workspace = await loadWorkspace(vault)

    expect(workspace.wip).toEqual([])
    expect(workspace.unsupported).toEqual([
      expect.objectContaining({ item_id: TASK_ID, path: 'inbox/tasks/future-task.md' }),
    ])
  })

  it('matches a placed Task by stable id after its source file is renamed', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/tasks/original-task.md', task())
    let workspace = await loadWorkspace(vault)
    workspace = await appendEvent(workspace, placeEvent(workspace.capture[0], {
      route: 'commit',
      commitment: '提交构建',
      next_action: '运行发布脚本',
      close_condition: '构建可安装',
    }, {
      now: () => '2026-09-01T03:30:00Z',
      id: () => 'task-event',
    }), {}, vault)
    await vault.rename('inbox/tasks/original-task.md', 'inbox/tasks/renamed-task.md')

    workspace = await loadWorkspace(vault)

    expect(workspace.wip).toHaveLength(1)
    expect(workspace.wip[0]).toMatchObject({
      kind: 'task',
      item_id: TASK_ID,
      path: 'inbox/tasks/renamed-task.md',
      orphan: false,
    })
  })

  it('creates Task source through a verified temporary file and no-clobber rename', async () => {
    const vault = new MemoryVault()
    const created = await createTaskSource({
      title: '提交 TestFlight 构建',
      body: '确认签名环境变量。',
      project: 'NoteMD',
      priority: 'P1',
      due: '2026-09-08',
      contexts: ['@电脑'],
      done_when: '构建可安装',
    }, {
      now: () => new Date(2026, 8, 1, 11, 20),
      id: () => TASK_ID,
    }, vault)

    expect(created.path).toBe('inbox/tasks/2026-09-01-1120-提交-testflight-构建-task.md')
    expect(created.source.task.id).toBe(TASK_ID)
    expect(created.source.task.project).toBe('NoteMD')
    expect(created.source.task).toMatchObject({ priority: 'P1', due: '2026-09-08', contexts: ['@电脑'] })
    expect(vault.files.get(created.path)).toBe(created.content)
    expect([...vault.files.keys()].some((path) => path.endsWith('.tmp'))).toBe(false)
    expect(vault.writes).toEqual([expect.stringMatching(/^inbox\/tasks\/\.next-task-.*\.tmp$/)])
  })

  it('treats no-clobber publication as success without an ambiguous final read', async () => {
    const vault = new MemoryVault()
    const originalRead = vault.read.bind(vault)
    vault.read = async (path) => path.endsWith('-task.md')
      ? Promise.reject(new Error('transient read failure'))
      : originalRead(path)

    await expect(createTaskSource({ title: 'Published' }, {
      now: () => new Date(2026, 8, 1, 11, 20),
      id: () => TASK_ID,
    }, vault)).resolves.toMatchObject({
      path: 'inbox/tasks/2026-09-01-1120-published-task.md',
    })
  })

  it('retries a raced final Task filename and cleans the temporary file on failure', async () => {
    const vault = new MemoryVault()
    const originalRename = vault.rename.bind(vault)
    let raced = false
    vault.rename = async (from, to) => {
      if (!raced) {
        raced = true
        vault.files.set(to, 'racer')
        throw new Error('exists')
      }
      return originalRename(from, to)
    }
    const created = await createTaskSource({ title: 'Race' }, {
      now: () => new Date(2026, 8, 1, 11, 20),
      id: () => TASK_ID,
    }, vault)
    expect(created.path).toBe('inbox/tasks/2026-09-01-1120-race-2-task.md')
    expect(vault.files.get('inbox/tasks/2026-09-01-1120-race-task.md')).toBe('racer')

    const broken = new MemoryVault()
    const originalRead = broken.read.bind(broken)
    broken.read = async (path) => path.endsWith('.tmp') ? { content: 'corrupt' } : originalRead(path)
    await expect(createTaskSource({ title: 'Broken' }, {
      now: () => new Date(2026, 8, 1, 11, 20),
      id: () => TASK_ID,
    }, broken)).rejects.toThrow('did not match')
    expect([...broken.files.keys()].some((path) => path.endsWith('.tmp'))).toBe(false)
  })

  it('upgrades to ledger v2 only when appending a Task lifecycle event', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/tasks/submit-task.md', task())
    const loaded = await loadWorkspace(vault)
    const after = await appendEvent(loaded, placeEvent(loaded.capture[0], {
      route: 'commit',
      commitment: '提交构建',
      next_action: '运行发布脚本',
      close_condition: '构建可安装',
    }, {
      now: () => '2026-09-01T03:30:00Z',
      id: () => 'task-event',
    }), {}, vault)

    expect(after.ledger).toMatchObject({ version: 2, task_dirs: ['inbox/tasks'] })
    expect(after.wip[0]).toMatchObject({ kind: 'task', item_id: TASK_ID })
    expect(after.ledger.events[0]).toMatchObject({ item_kind: 'task', item_id: TASK_ID })
  })
  it('creates a new Idea in Idea Spark current directory without appending a lifecycle event', async () => {
    const vault = new MemoryVault()
    vault.files.set('.notemd/idea-spark.json', JSON.stringify({ ideaDir: 'capture/sparks' }))
    const created = await createIdeaSource('# 新念头', {
      now: creationTime,
    }, vault)

    expect(created.path).toBe('capture/sparks/2026-08-30-0905-idea.md')
    expect(vault.files.get(created.path)).toBe(
      '---\ntype: Idea\ncreated: 2026-08-30T01:05:00.000Z\nnext:\n  priority: P2\n---\n# 新念头',
    )
    expect(vault.files.has(NEXT_PATH)).toBe(false)

    const workspace = await loadWorkspace(vault)
    expect(workspace.ideaDir).toBe('capture/sparks')
    expect(workspace.capture).toEqual([
      expect.objectContaining({ path: created.path, title: '新念头', body: '# 新念头', state: 'capture' }),
    ])
    expect(workspace.ledger.events).toEqual([])
  })

  it('keeps Idea planning metadata visible after lifecycle placement and searchable', async () => {
    const vault = new MemoryVault()
    const created = await createIdeaSource('# 有期限的行动', {
      now: creationTime,
      metadata: { priority: 'P0', due: '2026-09-08', contexts: ['@电脑', '@电话'] },
    }, vault)
    let workspace = await loadWorkspace(vault)
    expect(workspace.capture[0]).toMatchObject({
      path: created.path, priority: 'P0', due: '2026-09-08', contexts: ['@电脑', '@电话'],
    })
    const ids = ['metadata-event', 'metadata-item']
    workspace = await appendEvent(workspace, placeEvent(workspace.capture[0], {
      route: 'commit', commitment: '交付', next_action: '开始', close_condition: '完成',
    }, {
      now: () => '2026-09-01T03:30:00Z',
      id: () => ids.shift()!,
    }), {}, vault)
    expect(workspace.wip[0]).toMatchObject({
      priority: 'P0', due: '2026-09-08', contexts: ['@电脑', '@电话'],
    })
    expect(itemSearchText(workspace.wip[0])).toContain('@电话')
  })

  it('updates Idea metadata in its source without appending a lifecycle event', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', `---\ntype: Idea\ncreated: 2026-08-29T01:00:00Z\ncustom: keep\n---\n# A useful idea\n`)
    const workspace = await loadWorkspace(vault)
    vault.writes = []

    await updateItemPlanningMetadata(workspace.capture[0], {
      priority: 'P0', due: '2026-09-08', contexts: ['@电脑'],
    }, vault)

    expect(vault.writes).toEqual(['inbox/ideas/a-idea.md'])
    expect(vault.files.has(NEXT_PATH)).toBe(true)
    const reloaded = await loadWorkspace(vault)
    expect(reloaded.capture[0]).toMatchObject({
      priority: 'P0', due: '2026-09-08', contexts: ['@电脑'],
    })
    expect(YAML.parse(vault.files.get('inbox/ideas/a-idea.md')!.split('---')[1]!).custom).toBe('keep')
  })

  it('updates Task metadata at task.* and clears optional values', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/tasks/submit-task.md', task().replace(
      `${TASK_ID}\n`,
      `${TASK_ID}\n  priority: P1\n  due: "2026-09-08"\n  contexts: ["@电脑"]\n`,
    ))
    const workspace = await loadWorkspace(vault)
    vault.writes = []

    await updateItemPlanningMetadata(workspace.capture[0], { priority: 'P3', contexts: [] }, vault)

    expect(vault.writes).toEqual(['inbox/tasks/submit-task.md'])
    const reloaded = await loadWorkspace(vault)
    expect(reloaded.capture[0]).toMatchObject({ priority: 'P3', contexts: [] })
    expect(reloaded.capture[0].due).toBeUndefined()
    expect(reloaded.capture[0].task).toEqual({ version: 1, id: TASK_ID, priority: 'P3' })
  })

  it('refuses metadata updates for missing, repaired, or externally replaced sources', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea())
    const workspace = await loadWorkspace(vault)
    vault.writes = []

    await expect(updateItemPlanningMetadata({ ...workspace.capture[0], orphan: true }, {
      priority: 'P2', contexts: [],
    }, vault)).rejects.toThrow('missing')
    vault.files.set('inbox/ideas/a-idea.md', idea('2026-08-30T01:00:00Z'))
    await expect(updateItemPlanningMetadata(workspace.capture[0], {
      priority: 'P2', contexts: [],
    }, vault)).rejects.toThrow('changed')
    expect(vault.writes).toEqual([])
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
    expect(after.wip[0]).toMatchObject({ body: '# A useful idea\n' })
    expect(after.wip[0].projection).toMatchObject({ state: 'wip', next_action: 'Run one test' })
  })

  it('offers distinct active project markers for reuse, ordered by recent placement', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', idea('2026-08-29T01:00:00Z'))
    vault.files.set('inbox/ideas/b-idea.md', idea('2026-08-29T01:01:00Z'))
    let workspace = await loadWorkspace(vault)
    const [newer, older] = workspace.capture
    const ids = ['e1', 'i1', 'e2', 'i2']
    const eventFactory = { now: () => ids[0] === 'e1' ? '2026-08-29T02:00:00Z' : '2026-08-29T03:00:00Z', id: () => ids.shift()! }
    workspace = await appendEvent(workspace, placeEvent(older, {
      route: 'park', wake_trigger: 'later', projects: ['Writing', 'Shared'],
    }, eventFactory), {}, vault)
    workspace = await appendEvent(workspace, placeEvent(newer, {
      route: 'park', wake_trigger: 'later', project: 'Next',
    }, eventFactory), {}, vault)

    expect(workspace.projectOptions).toEqual(['Next', 'Writing', 'Shared'])
    expect(itemSearchText(workspace.dormant[0])).toContain('next')
  })

  it('derives a non-persistent Inbox project suggestion from confirmed local examples', async () => {
    const vault = new MemoryVault()
    vault.files.set('inbox/ideas/a-idea.md', '---\ntype: Idea\ncreated: 2026-08-29T01:00:00Z\n---\n# 念头泳道\n\n处理念头、泳道和关闭出口。')
    vault.files.set('inbox/ideas/b-idea.md', '---\ntype: Idea\ncreated: 2026-08-29T01:01:00Z\n---\n# 新的念头安放\n\n继续改进泳道与关闭出口。')
    let workspace = await loadWorkspace(vault)
    expect(workspace.capture.every((item) => item.suggestedProject === undefined)).toBe(true)
    const historical = workspace.capture.find((item) => item.path?.endsWith('a-idea.md'))!
    const ids = ['e-project', 'i-project']
    workspace = await appendEvent(workspace, placeEvent(historical, {
      route: 'park',
      wake_trigger: 'later',
      projects: ['Next'],
    }, {
      now: () => '2026-08-29T02:00:00Z',
      id: () => ids.shift()!,
    }), {}, vault)

    expect(workspace.projectOptions).toEqual(['Next'])
    expect(workspace.capture).toHaveLength(1)
    expect(workspace.capture[0].suggestedProject).toMatchObject({ project: 'Next', reason: 'content' })
    expect(workspace.ledger.events).toHaveLength(1)
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
    expect(workspace.wip[0].body).toBeUndefined()
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
      body: '# A useful idea\n',
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
      body: '# A useful idea\n',
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
