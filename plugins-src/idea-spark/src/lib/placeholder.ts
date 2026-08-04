// src/lib/placeholder.ts — grey-text prompts shown on a blank idea document.
import { t } from './strings'

/** 空白文档的灰字提示。五句轮换,各拆一个不肯动笔的借口;出处均可考。 */
export const PLACEHOLDER_KEYS = ['ph1', 'ph2', 'ph3', 'ph4', 'ph5'] as const

export function placeholderLines(): string[] {
  return PLACEHOLDER_KEYS.map((k) => t(k))
}

/** `lines[seq % len]`。计数器而非随机:五句都会轮到、行为可预测、测试不必注入种子。 */
export function pickPlaceholder(lines: string[], seq: number): string {
  if (lines.length === 0) return ''
  const n = Number.isFinite(seq) ? Math.floor(seq) : 0
  return lines[((n % lines.length) + lines.length) % lines.length]
}
