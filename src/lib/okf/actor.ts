// OKF v0.2 §7 actor 约定 + §5.2 的 verified 写入。
//
// 信任分级以 `human:` 前缀为键(§5.3),所以人工确认必须用 human: 形式 —— 这正是
// 产品信念 1「你确认过的判断」在元数据层的表达。
import { parseDocument, isMap, isSeq, YAMLMap, YAMLSeq } from 'yaml'

export const actor = {
  /** 人:`human:<id>` */
  human: (id: string) => `human:${id}`,
  /** agent / 工具:`<producer>/<version>` —— 版本段不可省 */
  agent: (producer: string, version: string) => `${producer}/${version}`,
  /** 自动流程:`process:<id>` */
  process: (id: string) => `process:${id}`,
}

const HUMAN = /^human:.+$/
const PROCESS = /^process:.+$/
const AGENT = /^[^/\s:][^/\s]*\/.+$/

/** 是否符合 §7 的三种形式之一。裸 agent 名(无版本段)不合规。 */
export function isOkfActor(v: string): boolean {
  return HUMAN.test(v) || PROCESS.test(v) || AGENT.test(v)
}

/**
 * 本机人类身份 id(用于 `human:<id>`)。优先 git 邮箱的本地部分(稳定、天然唯一),
 * 其次 git 用户名的 slug,再次系统用户名;全空时退回 `local`。
 * CJK 原样保留(file-over-app:不做音译)。
 */
export function humanActorId(src: { name: string; email: string; osUser: string }): string {
  const local = src.email.split('@')[0]?.trim() ?? ''
  if (local !== '') return local
  const name = slug(src.name)
  if (name !== '') return name
  const os = slug(src.osUser)
  return os !== '' ? os : 'local'
}

function slug(v: string): string {
  return v.trim().replace(/\s+/g, '-').toLowerCase()
}

/**
 * 在 front-matter 上追加一条 `verified` 事件(§5.2)。裸 mapping 会被提升成列表
 * (§11 的 MUST:消费者必须把裸 mapping 当单元素列表);同一 by+at 不重复追加。
 * 非 mapping 的 front-matter 原样返回。
 */
export function addVerified(raw: string | null, by: string, at: string): string {
  const doc = parseDocument(raw ?? '')
  if (doc.contents == null) doc.contents = doc.createNode({}) as never
  else if (!isMap(doc.contents)) return raw ?? ''

  const existing = doc.get('verified', true)
  const seq = isSeq(existing)
    ? (existing as YAMLSeq)
    : doc.createNode([]) as YAMLSeq
  if (isMap(existing)) seq.add(existing as YAMLMap)

  const already = seq.items.some((item) => {
    const v = (item as { toJSON?: () => unknown }).toJSON?.() as { by?: string; at?: string } | undefined
    return v?.by === by && v?.at === at
  })
  if (!already) seq.add(doc.createNode({ by, at }))

  doc.set('verified', seq)
  return doc.toString().replace(/\n$/, '')
}
