// src/lib/note-anno/answer-card.ts
// 正文内联答复卡片:贴在被批注段落之后的块级 decoration。
// 卡片是 decoration —— 不进文档、不进撤销历史、不影响序列化,源文件字节不变。
import { Plugin, PluginKey } from 'prosemirror-state'
import { Decoration, DecorationSet } from 'prosemirror-view'
import type { EditorView } from 'prosemirror-view'
import type { Node as PMNode } from 'prosemirror-model'
import { collectCardSites } from './answer-sites'
import type { AnswerEntry } from '../outline/answers'
import { t } from '../i18n/store.svelte'

const answerCardKey = new PluginKey<DecorationSet>('answer-cards')
/** 宿主在答复索引变化后用它强制重建 */
export const ANSWER_CARDS_REFRESH = 'answer-cards-refresh'

interface CardDeps {
  getEntries: () => Map<string, AnswerEntry>
  onAdopt: (entry: AnswerEntry, pos: number, view: EditorView) => void
}

/** 答复首个非空行,作折叠态摘要 */
function summaryOf(body: string): string {
  const line = body.split('\n').find(l => l.trim() !== '') ?? ''
  return line.length > 60 ? line.slice(0, 60) + '…' : line
}

function buildCard(entry: AnswerEntry, pos: number, view: EditorView, deps: CardDeps): HTMLElement {
  const root = document.createElement('div')
  root.className = 'answer-card'
  root.contentEditable = 'false'

  const head = document.createElement('button')
  head.className = 'answer-card-head'
  head.type = 'button'
  const sigil = document.createElement('span')
  sigil.className = 'answer-card-sigil'
  sigil.textContent = '✦'
  const title = document.createElement('span')
  title.className = 'answer-card-title'
  title.textContent = t('answerCard.label')
  const summary = document.createElement('span')
  summary.className = 'answer-card-summary'
  summary.textContent = summaryOf(entry.body)
  head.append(sigil, title, summary)
  head.title = t('answerCard.expand')
  root.appendChild(head)

  const bodyEl = document.createElement('div')
  bodyEl.className = 'answer-card-body'
  bodyEl.hidden = true
  root.appendChild(bodyEl)

  let rendered = false
  head.addEventListener('mousedown', (e) => { e.preventDefault(); e.stopPropagation() })
  head.addEventListener('click', (e) => {
    e.preventDefault(); e.stopPropagation()
    const show = bodyEl.hidden
    bodyEl.hidden = !show
    head.title = show ? t('answerCard.collapse') : t('answerCard.expand')
    root.classList.toggle('open', show)
    if (show && !rendered) {
      rendered = true
      // 懒渲染:展开时才把答复 markdown 变成 HTML
      void import('../plugins/host-render-html').then(({ renderMarkdownInline }) => {
        const html = document.createElement('div')
        html.className = 'answer-card-md'
        html.innerHTML = renderMarkdownInline(entry.body)
        const actions = document.createElement('div')
        actions.className = 'answer-card-actions'
        const adopt = document.createElement('button')
        adopt.type = 'button'
        adopt.className = 'answer-card-adopt'
        adopt.textContent = t('answerCard.adopt')
        adopt.addEventListener('mousedown', (ev) => { ev.preventDefault(); ev.stopPropagation() })
        adopt.addEventListener('click', (ev) => {
          ev.preventDefault(); ev.stopPropagation()
          deps.onAdopt(entry, pos, view)
        })
        actions.appendChild(adopt)
        bodyEl.append(html, actions)
      })
    }
  })
  return root
}

function build(doc: PMNode, view: EditorView | null, deps: CardDeps): DecorationSet {
  if (!view) return DecorationSet.empty
  const decos = collectCardSites(doc, deps.getEntries()).map(({ pos, entry }) =>
    Decoration.widget(pos, () => buildCard(entry, pos, view, deps), {
      side: 1, key: `answer-card-${pos}-${entry.questionId}`,
    }),
  )
  return DecorationSet.create(doc, decos)
}

export function answerCardPlugin(deps: CardDeps): Plugin<DecorationSet> {
  let view: EditorView | null = null
  return new Plugin<DecorationSet>({
    key: answerCardKey,
    view(v) { view = v; return {} },
    state: {
      init: () => DecorationSet.empty,
      apply(tr, old, _oldState, newState) {
        if (tr.docChanged || tr.getMeta(ANSWER_CARDS_REFRESH)) return build(newState.doc, view, deps)
        return old
      },
    },
    props: {
      decorations(state) { return answerCardKey.getState(state) },
    },
  })
}
