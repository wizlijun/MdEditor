// OKF v0.2 的派生读侧:信任层级(§5.3)与生命周期(§5.4)。
//
// 两者都是**派生值,不存储**。消费者 MUST NOT 因为缺信任数据而拒绝概念,
// 所以这里的每条路径都以"未知 = unverified / stable"收场,绝不抛错。
import { parse as parseYaml } from 'yaml'

export type TrustTier = 'unverified' | 'machine-confirmed' | 'human-reviewed'
export type LifecycleStatus = 'draft' | 'stable' | 'deprecated'

const STATUSES: readonly string[] = ['draft', 'stable', 'deprecated']

function fmObject(raw: string | null): Record<string, unknown> {
  if (!raw) return {}
  try {
    const v = parseYaml(raw)
    return v !== null && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : {}
  } catch {
    return {}
  }
}

/** `verified` 的条目列表。裸 mapping **MUST** 当作单元素列表(§11)。 */
function verifiedEntries(v: unknown): Array<{ by?: unknown }> {
  if (Array.isArray(v)) return v.filter((x): x is { by?: unknown } => x !== null && typeof x === 'object')
  if (v !== null && typeof v === 'object') return [v as { by?: unknown }]
  return []
}

/** 由 `verified` 派生的信任层级(§5.3)。 */
export function trustTier(frontmatter: string | null): TrustTier {
  const entries = verifiedEntries(fmObject(frontmatter).verified)
  if (entries.length === 0) return 'unverified'
  return entries.some((e) => typeof e.by === 'string' && e.by.startsWith('human:'))
    ? 'human-reviewed'
    : 'machine-confirmed'
}

/**
 * 生命周期(§5.4):缺省 `status` 即 `stable`;`today >= stale_after` 即过期。
 * `today` 为 `YYYY-MM-DD`,不传则取本地当天(字符串比较即可,同为 ISO 日期)。
 */
export function lifecycleOf(
  frontmatter: string | null,
  today?: string,
): { status: LifecycleStatus; stale: boolean; staleAfter: string | null } {
  const fm = fmObject(frontmatter)
  const declared = typeof fm.status === 'string' ? fm.status : ''
  const status = (STATUSES.includes(declared) ? declared : 'stable') as LifecycleStatus
  const staleAfter = typeof fm.stale_after === 'string' ? fm.stale_after : null
  const day = today ?? new Date().toISOString().slice(0, 10)
  return { status, stale: staleAfter != null && day >= staleAfter, staleAfter }
}
