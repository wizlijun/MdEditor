type Locale = 'zh' | 'en' | 'ja' | 'de'

const messages = {
  zh: {
    title: '记忆', subtitle: 'USER.md 与 MEMORY.md 是只读投影；所有变化先成为候选，再由人确认。',
    current: '当前条目', pending: '待确认', improve: '改善建议', add: '添加条目', migrate: '开始受控迁移',
    migrateHint: '当前文件仍是旧格式。迁移只会生成待确认候选，不会冒充人工批准。',
    drift: '检测到投影被直接编辑。批准和写入已停止，请先修复 drift。', search: '搜索条目…',
    all: '全部', active: '当前', revoked: '已撤销', pendingState: '待确认', highOnly: '只看高优先级',
    user: '用户画像', memory: '长期记忆', high: '高优先级', normal: '普通', edit: '编辑', revoke: '撤销', restore: '恢复',
    saveApprove: '保存并确认', cancel: '取消', source: '来源', section: '分区', content: '内容',
    approve: '批准', reject: '拒绝', before: '当前', after: '建议后', reason: '理由', proposedBy: '提议者',
    refresh: '刷新', loading: '正在读取…', noEntries: '没有匹配条目', noPending: '没有待确认候选',
    runSuggest: '检查可改善项', confirmDecision: '确认这项变化并写入只读投影？', confirmReject: '确认拒绝这个候选？',
    confirmMigrate: '迁移会为现有条目分配稳定 ID，并全部标为待确认。继续吗？', directReadOnly: '不要直接编辑 USER.md / MEMORY.md',
    claimOwner: '确认 Vault owner', ownerActor: 'owner actor，例如 human:bruce', ownerNames: '称呼，用逗号分隔',
  },
  en: {
    title: 'Memory', subtitle: 'USER.md and MEMORY.md are read-only projections. Changes become proposals before human approval.',
    current: 'Current', pending: 'Review', improve: 'Suggestions', add: 'Add entry', migrate: 'Start controlled migration',
    migrateHint: 'These files still use the legacy format. Migration creates pending proposals and does not fake human approval.',
    drift: 'Direct projection edits were detected. Decisions and writes are blocked until drift is repaired.', search: 'Search entries…',
    all: 'All', active: 'Active', revoked: 'Revoked', pendingState: 'Pending', highOnly: 'High priority only',
    user: 'User profile', memory: 'Long-term memory', high: 'High', normal: 'Normal', edit: 'Edit', revoke: 'Revoke', restore: 'Restore',
    saveApprove: 'Save and approve', cancel: 'Cancel', source: 'Source', section: 'Section', content: 'Content',
    approve: 'Approve', reject: 'Reject', before: 'Before', after: 'After', reason: 'Reason', proposedBy: 'Proposed by',
    refresh: 'Refresh', loading: 'Loading…', noEntries: 'No matching entries', noPending: 'No pending proposals',
    runSuggest: 'Find improvements', confirmDecision: 'Approve this exact change and update the read-only projection?', confirmReject: 'Reject this proposal?',
    confirmMigrate: 'Migration assigns stable IDs and marks every legacy entry pending. Continue?', directReadOnly: 'Do not edit USER.md / MEMORY.md directly',
    claimOwner: 'Claim vault owner', ownerActor: 'owner actor, e.g. human:bruce', ownerNames: 'names, comma separated',
  },
  ja: {} as Record<string, string>, de: {} as Record<string, string>,
} as const

let locale: Locale = 'en'
export function setLocale(raw: string): void { locale = raw.toLowerCase().startsWith('zh') ? 'zh' : raw.startsWith('ja') ? 'ja' : raw.startsWith('de') ? 'de' : 'en' }
export function t(key: keyof typeof messages.en): string {
  const table = messages[locale] as Record<string, string>
  return table[key] ?? messages.en[key]
}
