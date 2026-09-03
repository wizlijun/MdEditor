import type { SummaryStyle } from './settings'

export interface SummarySource {
  id: string
  path: string
  line: number
  lineEnd: number
}

export interface SummaryTaskStart {
  runId: string
  resolvedModel: string | null
  sources: SummarySource[]
  staleCount: number
}

export interface ValidSummary {
  content: string
  citations: string[]
}

export function validateSummaryOutput(
  value: string,
  sources: SummarySource[],
  style: SummaryStyle,
): ValidSummary {
  const content = value.trim()
  if (!content) throw new Error('简答为空')
  if (Array.from(content).length > 1_200) throw new Error('简答超出 1,200 字符上限')
  const known = new Set(sources.map((source) => source.id))
  const citations = Array.from(content.matchAll(/\[(S\d+)\]/g), (match) => match[1])
  if (!citations.length) throw new Error('简答缺少来源引用')
  const unknown = citations.find((id) => !known.has(id))
  if (unknown) throw new Error(`简答包含未知引用 [${unknown}]`)
  const blocks = content.split(/\n+/).map((line) => line.trim()).filter(Boolean)
  if (style === 'bullets' && blocks.length > 3) throw new Error('简答超过三个要点')
  if (style === 'sentence' && blocks.length !== 1) throw new Error('一句话简答只能包含一个段落')
  if (blocks.some((block) => !/\[S\d+\]/.test(block))) {
    throw new Error('每个简答要点都必须包含来源引用')
  }
  return { content, citations: Array.from(new Set(citations)) }
}
