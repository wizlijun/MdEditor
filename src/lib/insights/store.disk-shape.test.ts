import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import type { DayFile } from './store.svelte'

/**
 * 盘上日文件格式的**跨语言钉子**。
 *
 * v6.817.1 的事故:Rust 侧的摄取(`searchidx/src/attention.rs`)照着
 * `model.ts` 的 `DeviceAnalytics` 写成了 `docs: docKey -> day -> counters`
 * 两层嵌套 —— 那是**内存**结构。盘上的是这里 `DayFile` 的形状:`docs` 只有
 * **一层**(`docKey -> counters`),天由文件名隐含、另有一个顶层 `day` 字段。
 *
 * 后果:每份文件在 Rust 侧反序列化失败、被静默跳过,整个注意力加权功能对所有
 * 用户完全没生效,而两侧共 23 条单测全绿 —— 因为每条都用各自手写的 fixture,
 * 谁也没看过一份真实文件。
 *
 * 所以两侧现在钉**同一份** fixture:这里断言写入端产出的键与嵌套层级与它一致,
 * Rust 侧 `collect_reads_the_real_on_disk_shape` 直接读它。改了格式而只改一边,
 * 必有一边变红。
 */
const FIXTURE = join(__dirname, '../../../searchidx/tests/fixtures/analytics/2026-08-17.DEV-1.json')

describe('日文件盘上格式', () => {
  const fixture = JSON.parse(readFileSync(FIXTURE, 'utf8'))

  it('fixture 的顶层键就是 DayFile 的字段', () => {
    expect(Object.keys(fixture).sort()).toEqual(
      ['day', 'deviceId', 'deviceName', 'docs', 'sessions'].sort(),
    )
  })

  /** 这一条就是事故本身:多一层嵌套,Rust 侧整份文件就解析失败。 */
  it('docs 是一层 —— docKey 直接映射到计数器,不按天再嵌套', () => {
    const first = Object.values(fixture.docs)[0] as Record<string, unknown>
    expect(typeof first.read_ms).toBe('number')
    expect(typeof first.edit_ms).toBe('number')
    // 若哪天真的加回按天嵌套,这里拿到的会是一个以日期为键的对象。
    expect(Object.keys(first)).not.toContain(fixture.day)
  })

  it('day 是顶层字段,且与文件名一致', () => {
    expect(fixture.day).toBe('2026-08-17')
    expect(FIXTURE).toContain(`${fixture.day}.DEV-1.json`)
  })

  /** 写入端产出的对象必须能原样赋给 DayFile —— 类型层的对齐。 */
  it('fixture 满足 DayFile 的类型契约', () => {
    const typed: DayFile = fixture
    expect(typed.deviceId).toBe('DEV-1')
    expect(typed.docs['rel:notes/read-a-lot.md'].read_ms).toBe(600_000)
    expect(typed.sessions?.['rel:notes/read-a-lot.md']).toHaveLength(1)
  })

  it('docKey 用 rel: / abs: 前缀,两种都在 fixture 里', () => {
    const keys = Object.keys(fixture.docs)
    expect(keys.some((k) => k.startsWith('rel:'))).toBe(true)
    expect(keys.some((k) => k.startsWith('abs:'))).toBe(true)
  })
})
