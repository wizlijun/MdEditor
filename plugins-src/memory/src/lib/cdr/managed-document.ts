import { computeFingerprint, type BlockFingerprint } from '../../../../../src/lib/blockchunk/fingerprint'
import { bridge } from '../bridge'
import {
  parseDocumentSessionState,
  sha256Hex,
  type DocumentSessionState,
} from './session'
import {
  RepositoryIOError,
  RepositoryWriteBlockedError,
  type AggregateCommit,
  type AggregateCommitResult,
  type AggregateStore,
  type RepositoryRequest,
  type StoredAggregate,
} from './repository'

export const CDR_MANAGED_INSPECT_METHOD = 'host.cdr.repository.v2.inspect' as const
export const CDR_MANAGED_LOAD_METHOD = 'host.cdr.repository.v2.load' as const
export const CDR_MANAGED_COMMIT_METHOD = 'host.cdr.repository.v2.commit' as const
export const MANAGED_DOCUMENT_SCHEMA = 'notemd.cdr/managed-document/v1' as const
export const DERIVED_BLOCK_INDEX_SCHEMA = 'notemd.cdr/derived-block-index/v1' as const
export const MEMORY_SELF_PROFILE = 'notemd.memory.self' as const
export const MEMORY_WORKSPACE_FILENAME = 'Memory Workspace.note.md' as const

type UnknownRecord = Record<string, unknown>

interface DerivedBlockIndexEntry {
  blockId: string
  blockRevision: string
  fingerprint: BlockFingerprint
}

interface ManagedAggregate {
  schema: typeof MANAGED_DOCUMENT_SCHEMA
  profile: { id: typeof MEMORY_SELF_PROFILE; version: 1 }
  session: DocumentSessionState
  derivedBlockIndex: {
    schema: typeof DERIVED_BLOCK_INDEX_SCHEMA
    active: readonly DerivedBlockIndexEntry[]
  }
}

export type ManagedRepresentationStatus = 'unloaded' | 'in-sync' | 'external-drift' | 'missing'

export type ManagedDocumentInspection =
  | { kind: 'missing' }
  | { kind: 'located'; documentId: string }

export interface ManagedDocumentStatus {
  status: ManagedRepresentationStatus
  readOnlyReason: string | null
}

function io(message: string): never {
  throw new RepositoryIOError(message)
}

function record(value: unknown, path: string): UnknownRecord {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) io(`${path} must be an object`)
  return value as UnknownRecord
}

function exact(value: UnknownRecord, path: string, keys: readonly string[]): void {
  const actual = Object.keys(value).sort()
  const expected = [...keys].sort()
  if (JSON.stringify(actual) !== JSON.stringify(expected)) io(`${path} returned unexpected fields`)
}

function nonEmptyString(value: unknown, path: string): string {
  if (typeof value !== 'string' || !value) io(`${path} must be a non-empty string`)
  return value
}

function generation(value: unknown, path: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 1) io(`${path} is invalid`)
  return value as number
}

function sha256(value: unknown, path: string): string {
  if (typeof value !== 'string' || !/^[0-9a-f]{64}$/.test(value)) io(`${path} must be a lowercase SHA-256`)
  return value
}

function parseFingerprint(value: unknown, path: string): BlockFingerprint {
  const item = record(value, path)
  exact(item, path, ['hash', 'length', 'minhash'])
  if (typeof item.hash !== 'string' || !/^[0-9a-f]{12}$/.test(item.hash)) io(`${path}.hash is invalid`)
  if (!Number.isSafeInteger(item.length) || (item.length as number) < 0) io(`${path}.length is invalid`)
  if (!Array.isArray(item.minhash) || item.minhash.length !== 32
    || item.minhash.some((entry) => !Number.isInteger(entry) || entry < 0 || entry > 0xffffffff)) {
    io(`${path}.minhash is invalid`)
  }
  return { hash: item.hash, length: item.length as number, minhash: [...item.minhash] as number[] }
}

function sameFingerprint(left: BlockFingerprint, right: BlockFingerprint): boolean {
  return left.hash === right.hash
    && left.length === right.length
    && left.minhash.length === right.minhash.length
    && left.minhash.every((value, index) => value === right.minhash[index])
}

async function verifyManagedAggregate(aggregate: ManagedAggregate): Promise<void> {
  const knownBlocks = new Map<string, Set<string>>()
  const revisions = [...aggregate.session.revisionHistory, aggregate.session.head]
  const knownRevisionIds = new Set(revisions.map((revision) => revision.revisionId))
  for (const [revisionIndex, revision] of revisions.entries()) {
    for (const [blockIndex, block] of revision.blocks.entries()) {
      if (await sha256Hex(block.markdown) !== block.blockRevision) {
        io(`aggregate.session revision ${revisionIndex} block ${blockIndex} has an invalid content hash`)
      }
      const blockRevisions = knownBlocks.get(block.blockId) ?? new Set<string>()
      blockRevisions.add(block.blockRevision)
      knownBlocks.set(block.blockId, blockRevisions)
    }
  }
  for (const [receiptIndex, receipt] of aggregate.session.receipts.entries()) {
    if (receipt.outcome.kind !== 'applied') continue
    if (!knownRevisionIds.has(receipt.outcome.change.baseRevisionId)
      || !knownRevisionIds.has(receipt.outcome.change.revisionId)) {
      io(`aggregate.session.receipts[${receiptIndex}] references an unknown document revision`)
    }
    for (const operation of receipt.outcome.change.operations) {
      const revision = receipt.outcome.change.blockRevisions[operation.blockId]
      if (await sha256Hex(operation.markdown) !== revision
        || !knownBlocks.get(operation.blockId)?.has(revision)) {
        io(`aggregate.session.receipts[${receiptIndex}] does not match its applied block revision`)
      }
    }
  }
  for (const [proposalIndex, proposal] of aggregate.session.proposals.entries()) {
    if (!knownRevisionIds.has(proposal.batch.baseRevisionId)) {
      io(`aggregate.session.proposals[${proposalIndex}] references an unknown document revision`)
    }
    for (const operation of proposal.batch.operations) {
      if (!knownBlocks.get(operation.blockId)?.has(operation.expectedBlockRevision)) {
        io(`aggregate.session.proposals[${proposalIndex}] references an unknown block revision`)
      }
    }
  }
  for (const [assessmentIndex, assessment] of aggregate.session.assessments.entries()) {
    if (!knownBlocks.get(assessment.blockId)?.has(assessment.blockRevision)) {
      io(`aggregate.session.assessments[${assessmentIndex}] references an unknown block revision`)
    }
  }

  const entries = new Map(aggregate.derivedBlockIndex.active.map((entry) => [entry.blockId, entry]))
  for (const block of aggregate.session.head.blocks) {
    const entry = entries.get(block.blockId)!
    if (!sameFingerprint(entry.fingerprint, await computeFingerprint(block.markdown))) {
      io(`aggregate.derivedBlockIndex.active fingerprint does not match head block ${block.blockId}`)
    }
  }
}

async function parseManagedAggregate(value: unknown, documentId: string): Promise<ManagedAggregate> {
  const item = record(value, 'aggregate')
  exact(item, 'aggregate', ['schema', 'profile', 'session', 'derivedBlockIndex'])
  if (item.schema !== MANAGED_DOCUMENT_SCHEMA) io(`aggregate.schema must be ${MANAGED_DOCUMENT_SCHEMA}`)

  const profile = record(item.profile, 'aggregate.profile')
  exact(profile, 'aggregate.profile', ['id', 'version'])
  if (profile.id !== MEMORY_SELF_PROFILE || profile.version !== 1) io('aggregate.profile is unsupported')

  const session = parseDocumentSessionState(item.session)
  if (session.head.documentId !== documentId) io('aggregate.session belongs to another document')

  const derivedBlockIndex = record(item.derivedBlockIndex, 'aggregate.derivedBlockIndex')
  exact(derivedBlockIndex, 'aggregate.derivedBlockIndex', ['schema', 'active'])
  if (derivedBlockIndex.schema !== DERIVED_BLOCK_INDEX_SCHEMA) {
    io(`aggregate.derivedBlockIndex.schema must be ${DERIVED_BLOCK_INDEX_SCHEMA}`)
  }
  if (!Array.isArray(derivedBlockIndex.active)) io('aggregate.derivedBlockIndex.active must be an array')
  const active = derivedBlockIndex.active.map((value, index): DerivedBlockIndexEntry => {
    const identity = record(value, `aggregate.derivedBlockIndex.active[${index}]`)
    exact(identity, `aggregate.derivedBlockIndex.active[${index}]`, ['blockId', 'blockRevision', 'fingerprint'])
    return {
      blockId: nonEmptyString(identity.blockId, `aggregate.derivedBlockIndex.active[${index}].blockId`),
      blockRevision: sha256(identity.blockRevision, `aggregate.derivedBlockIndex.active[${index}].blockRevision`),
      fingerprint: parseFingerprint(identity.fingerprint, `aggregate.derivedBlockIndex.active[${index}].fingerprint`),
    }
  })
  const identities = new Map(active.map((identity) => [identity.blockId, identity]))
  if (identities.size !== active.length || active.length !== session.head.blocks.length) {
    io('aggregate.derivedBlockIndex.active must contain exactly one entry per head block')
  }
  for (const block of session.head.blocks) {
    const identity = identities.get(block.blockId)
    if (!identity || identity.blockRevision !== block.blockRevision) {
      io(`aggregate.derivedBlockIndex.active does not match head block ${block.blockId}`)
    }
  }

  const aggregate: ManagedAggregate = {
    schema: MANAGED_DOCUMENT_SCHEMA,
    profile: { id: MEMORY_SELF_PROFILE, version: 1 },
    session,
    derivedBlockIndex: { schema: DERIVED_BLOCK_INDEX_SCHEMA, active },
  }
  await verifyManagedAggregate(aggregate)
  return aggregate
}

async function buildManagedAggregate(session: DocumentSessionState): Promise<ManagedAggregate> {
  const aggregate: ManagedAggregate = {
    schema: MANAGED_DOCUMENT_SCHEMA,
    profile: { id: MEMORY_SELF_PROFILE, version: 1 },
    session,
    derivedBlockIndex: {
      schema: DERIVED_BLOCK_INDEX_SCHEMA,
      active: await Promise.all(session.head.blocks.map(async (block) => ({
        blockId: block.blockId,
        blockRevision: block.blockRevision,
        fingerprint: await computeFingerprint(block.markdown),
      }))),
    },
  }
  await verifyManagedAggregate(aggregate)
  return aggregate
}

function frontmatterPrefix(markdown: string): string {
  let offset = 0
  let first = true
  for (const line of markdown.match(/.*(?:\r\n|\n|$)/g) ?? []) {
    if (!line) continue
    offset += line.length
    const normalized = line.replace(/\r?\n$/, '')
    if (first) {
      if (normalized !== '---') io('managed Markdown requires leading frontmatter')
      first = false
      continue
    }
    if (normalized === '---') return markdown.slice(0, offset)
  }
  return io('managed Markdown frontmatter is not closed')
}

function renderMarkdown(prefix: string, session: DocumentSessionState): string {
  const body = session.head.blocks.map((block) => block.markdown).join('\n\n')
  return `${prefix}${body}\n`
}

export function managedMemoryPath(wikiDirectory: string): string {
  const wiki = wikiDirectory.replace(/^\/+|\/+$/g, '')
  if (!wiki || wiki.includes('\\') || wiki.includes(':') || wiki.split('/').some((part) => !part || part === '.' || part === '..')) {
    throw new RepositoryIOError('host.vault.info returned an invalid wiki_dir')
  }
  return `${wiki}/${MEMORY_WORKSPACE_FILENAME}`
}

export function canonicalMemoryFrontmatter(documentId: string): string {
  assertMemoryDocumentId(documentId)
  return `---\ntype: Memory\ncdr:\n  document_id: ${documentId}\n---\n`
}

function assertMemoryDocumentId(documentId: string): void {
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(documentId)) {
    throw new RepositoryIOError('Memory document id must be a lowercase UUID')
  }
}

export async function inspectManagedDocument(
  vaultPath: string,
  request: RepositoryRequest = (method, params) => bridge().request(method, params),
): Promise<ManagedDocumentInspection> {
  const response = record(await request(CDR_MANAGED_INSPECT_METHOD, { vault_path: vaultPath }), CDR_MANAGED_INSPECT_METHOD)
  if (response.kind === 'missing') {
    exact(response, CDR_MANAGED_INSPECT_METHOD, ['kind'])
    return { kind: 'missing' }
  }
  if (response.kind !== 'located') io(`${CDR_MANAGED_INSPECT_METHOD} returned an unsupported response`)
  exact(response, CDR_MANAGED_INSPECT_METHOD, ['kind', 'document_id'])
  return { kind: 'located', documentId: nonEmptyString(response.document_id, `${CDR_MANAGED_INSPECT_METHOD}.document_id`) }
}

/** One fixed vault path, one aggregate, and one Host-visible commit boundary. */
export class ManagedDocumentStore implements AggregateStore {
  #generation = 0
  #representationSha256: string | null = null
  #frontmatter: string | null
  #status: ManagedRepresentationStatus = 'unloaded'
  #readOnlyReason: string | null = null

  constructor(
    readonly vaultPath: string,
    readonly documentId: string,
    frontmatter: string | null = null,
    private readonly request: RepositoryRequest = (method, params) => bridge().request(method, params),
  ) {
    assertMemoryDocumentId(documentId)
    this.#frontmatter = frontmatter
  }

  get managedStatus(): ManagedDocumentStatus {
    return { status: this.#status, readOnlyReason: this.#readOnlyReason }
  }

  async load(documentId: string): Promise<StoredAggregate | undefined> {
    this.assertDocument(documentId)
    const response = record(await this.request(CDR_MANAGED_LOAD_METHOD, {
      document_id: documentId,
      vault_path: this.vaultPath,
    }), CDR_MANAGED_LOAD_METHOD)
    if (response.kind === 'missing') {
      exact(response, CDR_MANAGED_LOAD_METHOD, ['kind'])
      this.#generation = 0
      this.#representationSha256 = null
      this.#status = 'missing'
      return undefined
    }
    if (response.kind !== 'loaded') io(`${CDR_MANAGED_LOAD_METHOD} returned an unsupported response`)
    exact(response, CDR_MANAGED_LOAD_METHOD, ['kind', 'generation', 'aggregate', 'representation'])
    const loadedGeneration = generation(response.generation, `${CDR_MANAGED_LOAD_METHOD}.generation`)
    const aggregate = await parseManagedAggregate(response.aggregate, documentId)
    const representation = record(response.representation, `${CDR_MANAGED_LOAD_METHOD}.representation`)
    const status = representation.status
    if (status === 'missing') {
      exact(representation, `${CDR_MANAGED_LOAD_METHOD}.representation`, ['vault_path', 'committed_sha256', 'status'])
      this.#setDrift('missing', '受控 Markdown 已被外部删除；当前显示最后提交版本。')
    } else if (status === 'in-sync' || status === 'external-drift') {
      exact(representation, `${CDR_MANAGED_LOAD_METHOD}.representation`, [
        'vault_path', 'committed_sha256', 'status', 'disk_sha256', 'markdown', 'profile_type',
      ])
      if (representation.profile_type !== 'Memory') io('managed representation has an unsupported profile type')
      const markdown = nonEmptyString(representation.markdown, `${CDR_MANAGED_LOAD_METHOD}.representation.markdown`)
      const diskHash = sha256(representation.disk_sha256, `${CDR_MANAGED_LOAD_METHOD}.representation.disk_sha256`)
      if (await sha256Hex(markdown) !== diskHash) io('managed Markdown hash does not match its bytes')
      if (status === 'in-sync') {
        this.#frontmatter = frontmatterPrefix(markdown)
        if (renderMarkdown(this.#frontmatter, aggregate.session) !== markdown) {
          io('managed Markdown bytes do not match the committed aggregate')
        }
        this.#status = 'in-sync'
        this.#readOnlyReason = null
      } else {
        this.#setDrift(
          'external-drift',
          `编辑器显示最后提交版本；外部内容仍保留在 ${this.vaultPath}；当前阶段不支持导入。`,
        )
      }
    } else {
      io(`${CDR_MANAGED_LOAD_METHOD}.representation.status is unsupported`)
    }
    if (representation.vault_path !== this.vaultPath) io('managed representation returned another vault path')
    const committedHash = sha256(representation.committed_sha256, `${CDR_MANAGED_LOAD_METHOD}.representation.committed_sha256`)
    this.#generation = loadedGeneration
    this.#representationSha256 = committedHash
    return { generation: loadedGeneration, aggregate: aggregate.session }
  }

  async commit(command: AggregateCommit): Promise<AggregateCommitResult> {
    this.assertDocument(command.documentId)
    if (this.#readOnlyReason) throw new RepositoryWriteBlockedError(this.#readOnlyReason)
    if (command.expectedGeneration !== this.#generation) {
      throw new RepositoryIOError('managed store generation does not match the session generation')
    }
    const prefix = this.#frontmatter ?? canonicalMemoryFrontmatter(this.documentId)
    const markdown = renderMarkdown(prefix, command.aggregate)
    const aggregate = await buildManagedAggregate(command.aggregate)
    const response = record(await this.request(CDR_MANAGED_COMMIT_METHOD, {
      document_id: this.documentId,
      expected_generation: command.expectedGeneration,
      aggregate,
      representation: {
        vault_path: this.vaultPath,
        expected: this.#representationSha256
          ? { kind: 'present', sha256: this.#representationSha256 }
          : { kind: 'missing' },
        markdown,
      },
    }), CDR_MANAGED_COMMIT_METHOD)

    if (response.kind === 'committed') {
      exact(response, CDR_MANAGED_COMMIT_METHOD, ['kind', 'generation', 'representation_sha256'])
      const nextGeneration = generation(response.generation, `${CDR_MANAGED_COMMIT_METHOD}.generation`)
      if (nextGeneration !== command.expectedGeneration + 1) io('managed commit returned a non-successor generation')
      const representationHash = sha256(response.representation_sha256, `${CDR_MANAGED_COMMIT_METHOD}.representation_sha256`)
      if (await sha256Hex(markdown) !== representationHash) io('managed commit returned the wrong representation hash')
      this.#generation = nextGeneration
      this.#representationSha256 = representationHash
      this.#frontmatter = prefix
      this.#status = 'in-sync'
      return { kind: 'committed', generation: nextGeneration }
    }

    if (response.kind === 'aggregate-conflict') {
      exact(response, CDR_MANAGED_COMMIT_METHOD, ['kind', 'current'])
      const current = record(response.current, `${CDR_MANAGED_COMMIT_METHOD}.current`)
      exact(current, `${CDR_MANAGED_COMMIT_METHOD}.current`, ['generation', 'aggregate', 'representation_sha256'])
      const currentGeneration = generation(current.generation, `${CDR_MANAGED_COMMIT_METHOD}.current.generation`)
      const currentAggregate = await parseManagedAggregate(current.aggregate, this.documentId)
      const currentHash = sha256(current.representation_sha256, `${CDR_MANAGED_COMMIT_METHOD}.current.representation_sha256`)
      if (await sha256Hex(renderMarkdown(prefix, currentAggregate.session)) !== currentHash) {
        throw new RepositoryWriteBlockedError('并发提交的聚合状态与 Markdown 表示不一致。')
      }
      this.#generation = currentGeneration
      this.#representationSha256 = currentHash
      return { kind: 'conflict', current: { generation: currentGeneration, aggregate: currentAggregate.session } }
    }

    if (response.kind === 'external-drift') {
      this.#setDriftFromCommit(response)
      throw new RepositoryWriteBlockedError(this.#readOnlyReason!)
    }
    return io(`${CDR_MANAGED_COMMIT_METHOD} returned an unsupported response`)
  }

  #setDriftFromCommit(response: UnknownRecord): void {
    exact(response, CDR_MANAGED_COMMIT_METHOD, ['kind', 'current', 'representation'])
    const representation = record(response.representation, `${CDR_MANAGED_COMMIT_METHOD}.representation`)
    exact(representation, `${CDR_MANAGED_COMMIT_METHOD}.representation`, ['vault_path', 'disk'])
    if (representation.vault_path !== this.vaultPath) io('external drift returned another vault path')
    const disk = record(representation.disk, `${CDR_MANAGED_COMMIT_METHOD}.representation.disk`)
    if (disk.status === 'missing') {
      exact(disk, `${CDR_MANAGED_COMMIT_METHOD}.representation.disk`, ['status'])
      this.#setDrift('missing', '受控 Markdown 已被外部删除；本次修改未写入。')
      return
    }
    if (disk.status === 'external-drift') {
      exact(disk, `${CDR_MANAGED_COMMIT_METHOD}.representation.disk`, ['status', 'disk_sha256', 'markdown'])
      sha256(disk.disk_sha256, `${CDR_MANAGED_COMMIT_METHOD}.representation.disk.disk_sha256`)
      nonEmptyString(disk.markdown, `${CDR_MANAGED_COMMIT_METHOD}.representation.disk.markdown`)
      this.#setDrift(
        'external-drift',
        `本次修改未写入；外部内容仍保留在 ${this.vaultPath}；当前阶段不支持导入。`,
      )
      return
    }
    io('external drift returned an unsupported disk status')
  }

  #setDrift(status: 'missing' | 'external-drift', reason: string): void {
    this.#status = status
    this.#readOnlyReason = reason
  }

  assertDocument(documentId: string): void {
    if (documentId !== this.documentId) throw new RepositoryIOError('managed store cannot change document identity')
  }
}
