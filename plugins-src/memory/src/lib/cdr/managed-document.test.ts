import { describe, expect, it } from 'vitest'
import {
  CDR_MANAGED_COMMIT_METHOD,
  CDR_MANAGED_INSPECT_METHOD,
  CDR_MANAGED_LOAD_METHOD,
  ManagedDocumentStore,
  canonicalMemoryFrontmatter,
  inspectManagedDocument,
  managedMemoryPath,
} from './managed-document'
import { PersistentDocumentSession, RepositoryWriteBlockedError } from './repository'
import { sha256Hex, uuidIds, type DocumentRevision, type OperationBatch } from './session'

const documentId = '01900000-0000-7000-8000-000000000001'
const vaultPath = 'wikipage/Memory Workspace.note.md'

async function fixture(): Promise<DocumentRevision> {
  const markdown = ['# MEMORY', 'Project background.']
  return {
    documentId,
    revisionId: 'revision/initial',
    blocks: await Promise.all(markdown.map(async (text, index) => ({
      blockId: `block-${index + 1}`,
      blockRevision: await sha256Hex(text),
      markdown: text,
    }))),
  }
}

function replace(snapshot: DocumentRevision, markdown: string): OperationBatch {
  const block = snapshot.blocks[1]
  return {
    requestId: 'request/update',
    baseRevisionId: snapshot.revisionId,
    operations: [{
      kind: 'block.replace',
      operationId: 'operation/update',
      blockId: block.blockId,
      expectedBlockRevision: block.blockRevision,
      markdown,
    }],
  }
}

class FakeManagedHost {
  generation = 0
  aggregate: unknown
  markdown: string | undefined
  commitCalls = 0

  request = async (method: string, params: any): Promise<unknown> => {
    if (method === CDR_MANAGED_INSPECT_METHOD) {
      return this.generation ? { kind: 'located', document_id: documentId } : { kind: 'missing' }
    }
    if (method === CDR_MANAGED_LOAD_METHOD) {
      if (!this.generation) return { kind: 'missing' }
      const committed = await sha256Hex(this.committedMarkdown)
      const current = this.markdown
      return {
        kind: 'loaded',
        generation: this.generation,
        aggregate: structuredClone(this.aggregate),
        representation: current === undefined
          ? { vault_path: vaultPath, committed_sha256: committed, status: 'missing' }
          : {
              vault_path: vaultPath,
              committed_sha256: committed,
              status: await sha256Hex(current) === committed ? 'in-sync' : 'external-drift',
              disk_sha256: await sha256Hex(current),
              markdown: current,
              profile_type: 'Memory',
            },
      }
    }
    if (method !== CDR_MANAGED_COMMIT_METHOD) throw new Error(`unexpected ${method}`)
    this.commitCalls += 1
    const expectedHash = params.representation.expected.kind === 'present'
      ? params.representation.expected.sha256
      : null
    const diskHash = this.markdown === undefined ? null : await sha256Hex(this.markdown)
    const committedHash = this.generation ? await sha256Hex(this.committedMarkdown) : null
    if (params.expected_generation !== this.generation) {
      return {
        kind: 'aggregate-conflict',
        current: {
          generation: this.generation,
          aggregate: structuredClone(this.aggregate),
          representation_sha256: committedHash,
        },
      }
    }
    if (expectedHash !== diskHash) {
      return {
        kind: 'external-drift',
        current: this.generation ? {
          generation: this.generation,
          aggregate: structuredClone(this.aggregate),
          representation_sha256: committedHash,
        } : null,
        representation: {
          vault_path: vaultPath,
          disk: this.markdown === undefined
            ? { status: 'missing' }
            : { status: 'external-drift', disk_sha256: diskHash, markdown: this.markdown },
        },
      }
    }
    this.generation += 1
    this.aggregate = structuredClone(params.aggregate)
    this.committedMarkdown = params.representation.markdown
    this.markdown = this.committedMarkdown
    return {
      kind: 'committed',
      generation: this.generation,
      representation_sha256: await sha256Hex(this.committedMarkdown),
    }
  }

  private committedMarkdown = ''
}

describe('managed-document', () => {
  it('derives the one fixed Memory-first path and inspects without creating it', async () => {
    expect(managedMemoryPath('wiki')).toBe('wiki/Memory Workspace.note.md')
    expect(() => managedMemoryPath('../wiki')).toThrow('invalid wiki_dir')
    await expect(inspectManagedDocument(vaultPath, async () => ({ kind: 'missing' })))
      .resolves.toEqual({ kind: 'missing' })
    await expect(inspectManagedDocument(vaultPath, async () => ({ kind: 'located', document_id: documentId })))
      .resolves.toEqual({ kind: 'located', documentId })
  })

  it('commits Markdown, session history, and a derived fingerprint index as one aggregate generation', async () => {
    const host = new FakeManagedHost()
    const initial = await fixture()
    const store = new ManagedDocumentStore(vaultPath, documentId, canonicalMemoryFrontmatter(documentId), host.request)
    const session = await PersistentDocumentSession.open(initial, uuidIds(), store)

    expect(session.openKind).toBe('created')
    expect(host.markdown).toBe(`${canonicalMemoryFrontmatter(documentId)}# MEMORY\n\nProject background.\n`)
    const created = host.aggregate as any
    expect(created.profile).toEqual({ id: 'notemd.memory.self', version: 1 })
    expect(created.session.head).toEqual(initial)
    expect(created.derivedBlockIndex.active).toHaveLength(2)
    expect(created.derivedBlockIndex.active[0]).toMatchObject({
      blockId: 'block-1', blockRevision: initial.blocks[0].blockRevision,
      fingerprint: { hash: expect.stringMatching(/^[0-9a-f]{12}$/), length: 8 },
    })

    await session.submit(replace(session.snapshot(), 'Updated background.'), 'human:local')
    expect(host.generation).toBe(2)
    expect(session.snapshot().blocks[1].blockRevision).toBe(await sha256Hex('Updated background.'))
    expect((host.aggregate as any).session.revisionHistory).toEqual([initial])
    expect(host.markdown).toContain('Updated background.')
    expect(store.managedStatus).toEqual({ status: 'in-sync', readOnlyReason: null })

    const reopened = await PersistentDocumentSession.open(initial, uuidIds(), new ManagedDocumentStore(
      vaultPath, documentId, null, host.request,
    ))
    expect(reopened.snapshot()).toEqual(session.snapshot())
    expect(reopened.revisionHistory()).toEqual([initial])
  })

  it('preserves external bytes, rejects the write, and latches the session read-only', async () => {
    const host = new FakeManagedHost()
    const initial = await fixture()
    const store = new ManagedDocumentStore(vaultPath, documentId, canonicalMemoryFrontmatter(documentId), host.request)
    const session = await PersistentDocumentSession.open(initial, uuidIds(), store)
    const committed = host.markdown
    host.markdown = `${committed}external edit\n`

    await expect(session.submit(replace(session.snapshot(), 'Must not overwrite.'), 'human:local'))
      .rejects.toBeInstanceOf(RepositoryWriteBlockedError)
    expect(host.markdown).toContain('external edit')
    expect(host.markdown).not.toContain('Must not overwrite')
    expect(store.managedStatus.status).toBe('external-drift')
    const calls = host.commitCalls
    await expect(session.assess('block-2', 'agent:verifier', 'verified'))
      .rejects.toBeInstanceOf(RepositoryWriteBlockedError)
    expect(host.commitCalls).toBe(calls)
  })

  it('rejects a stored aggregate whose exact block hash or fingerprint was tampered', async () => {
    const host = new FakeManagedHost()
    const initial = await fixture()
    await PersistentDocumentSession.open(
      initial,
      uuidIds(),
      new ManagedDocumentStore(vaultPath, documentId, canonicalMemoryFrontmatter(documentId), host.request),
    )

    const hashTampered = structuredClone(host.aggregate) as any
    hashTampered.session.head.blocks[0].markdown = '# Different bytes'
    host.aggregate = hashTampered
    await expect(new ManagedDocumentStore(vaultPath, documentId, null, host.request).load(documentId))
      .rejects.toThrow('invalid content hash')

    host.aggregate = structuredClone(hashTampered)
    ;(host.aggregate as any).session.head.blocks[0].markdown = initial.blocks[0].markdown
    ;(host.aggregate as any).derivedBlockIndex.active[0].fingerprint.length += 1
    await expect(new ManagedDocumentStore(vaultPath, documentId, null, host.request).load(documentId))
      .rejects.toThrow('fingerprint does not match')
  })

  it('opens a deleted representation as the last committed state in read-only mode', async () => {
    const host = new FakeManagedHost()
    const initial = await fixture()
    await PersistentDocumentSession.open(
      initial,
      uuidIds(),
      new ManagedDocumentStore(vaultPath, documentId, canonicalMemoryFrontmatter(documentId), host.request),
    )
    host.markdown = undefined
    const store = new ManagedDocumentStore(vaultPath, documentId, null, host.request)
    const reopened = await PersistentDocumentSession.open(initial, uuidIds(), store)

    expect(reopened.snapshot()).toEqual(initial)
    expect(store.managedStatus).toEqual({
      status: 'missing',
      readOnlyReason: '受控 Markdown 已被外部删除；当前显示最后提交版本。',
    })
  })
})
