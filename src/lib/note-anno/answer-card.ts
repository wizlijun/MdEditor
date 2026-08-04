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

/** 答复首个非空行,作折叠态摘要。答复只有子节点清单时首行是个列表项,摘要里不显示行首的 `- ` */
function summaryOf(body: string): string {
  const line = (body.split('\n').find(l => l.trim() !== '') ?? '').replace(/^\s*[-*+] /, '')
  return line.length > 60 ? line.slice(0, 60) + '…' : line
}

/**
 * 卡片正文是 **agent 写的、不受信任的**内容(vault 是多 agent 的公共地带),
 * 而主窗口没有 CSP 且能调 Tauri IPC —— 直接 innerHTML 会让
 * `<img src=x onerror=…>` 拿到文件系统权限。渲染前剥掉脚本类节点与所有
 * on* 事件属性、javascript: 链接。
 */
export function sanitizeInto(host: HTMLElement, html: string): void {
  const doc = new DOMParser().parseFromString(html, 'text/html')
  doc.querySelectorAll('script, iframe, object, embed, link, meta, style, base, form').forEach(el => el.remove())
  doc.querySelectorAll('*').forEach((el) => {
    for (const attr of [...el.attributes]) {
      const name = attr.name.toLowerCase()
      const value = attr.value.replace(/\s+/g, '').toLowerCase()
      if (name.startsWith('on')) el.removeAttribute(attr.name)
      else if ((name === 'href' || name === 'src' || name === 'xlink:href') && value.startsWith('javascript:')) {
        el.removeAttribute(attr.name)
      }
    }
  })
  host.replaceChildren(...Array.from(doc.body.childNodes))
}

function buildCard(entry: AnswerEntry, pos: number, view: EditorView, deps: CardDeps): HTMLElement {
  const root = document.createElement('div')
  root.className = 'answer-card'
  root.contentEditable = 'false'

  const head = document.createElement('button')
  head.className = 'answer-card-head'
  head.type = 'button'
  const chevron = document.createElement('span')
  chevron.className = 'answer-card-chevron'
  chevron.textContent = '▸'
  const sigil = document.createElement('span')
  sigil.className = 'answer-card-sigil'
  sigil.textContent = '✦'
  const title = document.createElement('span')
  title.className = 'answer-card-title'
  title.textContent = t('answerCard.label')
  const summary = document.createElement('span')
  summary.className = 'answer-card-summary'
  summary.textContent = summaryOf(entry.body)
  head.append(chevron, sigil, title, summary)
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
        sanitizeInto(html, renderMarkdownInline(entry.body))
        const actions = document.createElement('div')
        actions.className = 'answer-card-actions'
        const collapse = document.createElement('button')
        collapse.type = 'button'
        collapse.className = 'answer-card-collapse'
        collapse.textContent = t('answerCard.collapse')
        collapse.addEventListener('mousedown', (ev) => { ev.preventDefault(); ev.stopPropagation() })
        collapse.addEventListener('click', (ev) => { ev.preventDefault(); ev.stopPropagation(); head.click() })
        const adopt = document.createElement('button')
        adopt.type = 'button'
        adopt.className = 'answer-card-adopt'
        adopt.textContent = t('answerCard.adopt')
        adopt.addEventListener('mousedown', (ev) => { ev.preventDefault(); ev.stopPropagation() })
        adopt.addEventListener('click', (ev) => {
          ev.preventDefault(); ev.stopPropagation()
          // 卡片要等回写+索引刷新(磁盘 IO)才消失,期间 PM 会按同一 key 复用这个 DOM。
          // 不禁用的话双击会把答复插进正文两次。
          if (adopt.disabled) return
          adopt.disabled = true
          deps.onAdopt(entry, pos, view)
        })
        actions.append(collapse, adopt)
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
    view(v) {
      view = v
      // 插件是在若干 await import 之后才 reconfigure 进来的,索引可能已经加载完毕
      // (刷新事务打在没有本插件的 state 上就丢了)。自己补一次,否则要等到首次输入
      // 才会出卡片。
      queueMicrotask(() => {
        if (deps.getEntries().size > 0) v.dispatch(v.state.tr.setMeta(ANSWER_CARDS_REFRESH, true))
      })
      return {}
    },
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
