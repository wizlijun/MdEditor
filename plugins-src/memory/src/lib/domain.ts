import type {
  ApprovalKind,
  ClaimKind,
  EffectiveClaim,
  MemoryClaimRevision,
  PendingClaim,
  ProjectionTarget,
  RevisionRef,
  RiskClass,
} from './types'

export const categoryOptions: Record<ProjectionTarget, Array<{ id: string; label: string; kind: ClaimKind }>> = {
  user: [
    { id: 'owner', label: '所有者', kind: 'identity' },
    { id: 'identity', label: '身份', kind: 'identity' },
    { id: 'preferences', label: '偏好', kind: 'preference' },
    { id: 'work-style', label: '工作方式', kind: 'practice' },
    { id: 'boundaries', label: '边界', kind: 'boundary' },
    { id: 'other', label: '其他', kind: 'belief' },
  ],
  memory: [
    { id: 'decisions', label: '已确认决定', kind: 'decision' },
    { id: 'constraints', label: '约束', kind: 'boundary' },
    { id: 'practices', label: '实践', kind: 'practice' },
    { id: 'context', label: '背景', kind: 'material-fact' },
    { id: 'other', label: '其他', kind: 'belief' },
  ],
}

export const claimKindLabels: Record<ClaimKind, string> = {
  identity: '身份表达', preference: '个人偏好', boundary: '行为边界', decision: '已作决定', belief: '个人判断',
  observation: '观察记录', commitment: '长期承诺', practice: '稳定做法', 'material-fact': '外部事实',
  quotation: '原话记录',
}

export const approvalLabels: Record<ApprovalKind, { label: string; explanation: string; button: string }> = {
  'self-representation': {
    label: '忠实表达本人',
    explanation: '表示这条主张忠实表达了你的身份、偏好、判断或决定；不验证外部事实，也不授权现实行动。',
    button: '确认记住',
  },
  'behavioral-authorization': {
    label: '允许助理据此调整行为',
    explanation: '只在显示的 Space、用途和行为范围内生效；不授权发送、支付、删除或替你承诺。',
    button: '允许此行为',
  },
  'factual-verification': {
    label: '确认外部事实正确',
    explanation: '表示你确认这条外部主张在给定证据与时间范围内可作为已核验事实。',
    button: '确认事实正确',
  },
}

export function approvalKindFor(kind: ClaimKind): ApprovalKind {
  if (kind === 'boundary' || kind === 'practice') return 'behavioral-authorization'
  if (kind === 'material-fact') return 'factual-verification'
  return 'self-representation'
}

export function riskFor(kind: ClaimKind): RiskClass {
  if (kind === 'boundary') return 'action-sensitive'
  if (kind === 'decision' || kind === 'practice' || kind === 'commitment') return 'behavioral'
  return 'informational'
}

export function categoryLabel(target: ProjectionTarget, category: string): string {
  return categoryOptions[target].find((item) => item.id === category)?.label ?? category
}

export function subjectLabel(claim: MemoryClaimRevision): string {
  if (claim.subject.label) return claim.subject.label
  if (claim.subject.kind === 'vault-owner') return 'Vault 所有者本人'
  return `${claim.subject.kind} · ${claim.subject.id}`
}

export function actorLabel(actor: MemoryClaimRevision['recorded_by']): string {
  if (actor.kind === 'owner') return 'Vault 所有者'
  if (actor.kind === 'agent') return `Agent · ${actor.id}`
  if (actor.kind === 'host') return 'note.md'
  return `${actor.kind} · ${actor.id}`
}

export type ConfirmedSort = 'priority' | 'recent' | 'oldest' | 'text'

export interface ConfirmedClaimGroup {
  key: string
  label: string
  categories: Array<{ key: string; label: string; items: EffectiveClaim[] }>
}

const claimKindRank: Record<ClaimKind, number> = {
  identity: 0, preference: 1, boundary: 2, decision: 3, belief: 4,
  observation: 5, commitment: 6, practice: 7, 'material-fact': 8, quotation: 9,
}
const claimTextCollator = new Intl.Collator('zh-CN', { numeric: true, sensitivity: 'base' })

function compareRecordedAt(left: MemoryClaimRevision, right: MemoryClaimRevision, newestFirst: boolean): number {
  const leftTime = Date.parse(left.recorded_at)
  const rightTime = Date.parse(right.recorded_at)
  const leftValid = Number.isFinite(leftTime)
  const rightValid = Number.isFinite(rightTime)
  if (leftValid !== rightValid) return leftValid ? -1 : 1
  if (!leftValid) return 0
  return newestFirst ? rightTime - leftTime : leftTime - rightTime
}

function sectionKey(claim: MemoryClaimRevision): 'memory' | 'user' | 'structured' {
  return claim.projection.visibility === 'projection' ? claim.projection.target : 'structured'
}

function sectionRank(claim: MemoryClaimRevision): number {
  return { memory: 0, user: 1, structured: 2 }[sectionKey(claim)]
}

function categoryRank(claim: MemoryClaimRevision): number {
  const known = categoryOptions[claim.projection.target].findIndex((item) => item.id === claim.projection.category)
  return known < 0 ? categoryOptions[claim.projection.target].length : known
}

function compareProjectionGroup(left: MemoryClaimRevision, right: MemoryClaimRevision): number {
  const section = sectionRank(left) - sectionRank(right)
  if (section) return section
  if (sectionKey(left) === 'structured' && left.projection.target !== right.projection.target) {
    return left.projection.target === 'memory' ? -1 : 1
  }
  return categoryRank(left) - categoryRank(right)
    || left.projection.category.localeCompare(right.projection.category)
}

export function currentClaims(
  claims: EffectiveClaim[],
  query = '',
  target: 'all' | ProjectionTarget | 'structured' = 'all',
  sort: ConfirmedSort = 'priority',
): EffectiveClaim[] {
  const q = query.trim().toLocaleLowerCase()
  const salienceRank = { pinned: 0, normal: 1 }
  return claims.filter(({ claim, application_state }) =>
    application_state === 'current'
    && claim.workflow.state === 'approved'
    && claim.lifecycle.state === 'active'
    && (target === 'all' || (target === 'structured'
      ? claim.projection.visibility !== 'projection'
      : claim.projection.visibility === 'projection' && claim.projection.target === target))
    && (!q || `${claim.text} ${claim.claim_kind} ${claim.projection.category} ${claim.subject.label ?? ''}`.toLocaleLowerCase().includes(q)),
  ).sort((a, b) => {
    const group = compareProjectionGroup(a.claim, b.claim)
    if (group) return group
    const withinGroup = sort === 'priority'
      ? salienceRank[a.claim.salience] - salienceRank[b.claim.salience]
        || claimKindRank[a.claim.claim_kind] - claimKindRank[b.claim.claim_kind]
      : sort === 'text'
        ? claimTextCollator.compare(a.claim.text, b.claim.text)
        : compareRecordedAt(a.claim, b.claim, sort === 'recent')
    return withinGroup || a.claim.claim_id.localeCompare(b.claim.claim_id)
  })
}

export function groupCurrentClaims(claims: EffectiveClaim[]): ConfirmedClaimGroup[] {
  const sections: ConfirmedClaimGroup[] = []
  for (const item of claims) {
    const claim = item.claim
    const key = sectionKey(claim)
    let section = sections.find((candidate) => candidate.key === key)
    if (!section) {
      section = {
        key,
        label: key === 'memory' ? 'MEMORY.md' : key === 'user' ? 'USER.md' : '仅结构化上下文',
        categories: [],
      }
      sections.push(section)
    }
    const categoryKey = key === 'structured'
      ? `${claim.projection.target}:${claim.projection.category}`
      : claim.projection.category
    let category = section.categories.find((candidate) => candidate.key === categoryKey)
    if (!category) {
      const projectionPrefix = claim.projection.target === 'memory' ? 'MEMORY' : 'USER'
      category = {
        key: categoryKey,
        label: key === 'structured'
          ? `${projectionPrefix} · ${categoryLabel(claim.projection.target, claim.projection.category)}`
          : categoryLabel(claim.projection.target, claim.projection.category),
        items: [],
      }
      section.categories.push(category)
    }
    category.items.push(item)
  }
  return sections
}

export function pendingClaims(pending: PendingClaim[]): PendingClaim[] {
  const riskRank: Record<RiskClass, number> = { 'action-sensitive': 0, behavioral: 1, informational: 2 }
  return [...pending].sort((a, b) => riskRank[a.revision.risk_class] - riskRank[b.revision.risk_class]
    || a.revision.recorded_at.localeCompare(b.revision.recorded_at))
}

export function approvalForPending(item: PendingClaim): ApprovalKind {
  return item.required_approval_kind ?? item.revision.decision?.approval_kind ?? approvalKindFor(item.revision.claim_kind)
}

export function expectedHeads(claim: MemoryClaimRevision): RevisionRef[] {
  return [{ revision_id: claim.revision_id, payload_sha256: claim.payload_sha256 }]
}

export function temporalLabel(claim: MemoryClaimRevision): string {
  const { valid_from, valid_until, observed_at, uttered_at, review_after } = claim.temporal
  if (valid_from || valid_until) return `${valid_from ? formatDate(valid_from) : '过去'} — ${valid_until ? formatDate(valid_until) : '持续有效'}`
  if (observed_at) return `观察于 ${formatDate(observed_at)}`
  if (uttered_at) return `表达于 ${formatDate(uttered_at)}`
  if (review_after) return `需在 ${formatDate(review_after)} 后复核`
  return '未声明有效时间'
}

export function formatDate(value: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.valueOf())) return value
  return new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(date)
}

export function hostError(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error)
  if (message.includes('unknown method') || message.includes('METHOD_NOT_FOUND') || message.toLowerCase().includes('unsupported')) {
    return '当前 note.md 宿主尚未提供 Memory Protocol v2 RPC。请升级宿主后再使用此版本插件。'
  }
  if (message.includes('MEMORY_STALE_BASE')) return '这条主张已在另一设备发生变化。请刷新并重新查看差异。'
  if (message.includes('MEMORY_REVISION_HASH_CHANGED')) return '待确认内容已经变化，本次操作已安全停止。请刷新后重新确认。'
  if (message.includes('MEMORY_UNAUTHORIZED')) return '当前窗口没有执行这项人工决定的权限。'
  if (message.includes('MEMORY_CONTEXT_INCOMPLETE')) return '请选择明确的 Space、用途、Provider 和 Model。'
  if (message.includes('MEMORY_EXTERNAL_TRANSFER_DENIED')) return '当前 Provider 策略不允许外发这些主张。'
  return message
}
