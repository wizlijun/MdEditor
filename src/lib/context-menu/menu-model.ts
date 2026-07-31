import { t } from '../i18n/store.svelte'
import type { Messages } from '../i18n/en'

export interface MenuItemSpec {
  id: string
  label: string
  icon?: string
  emphasis?: boolean          // rendered bold/outlined (highlight, wikilink)
  needsSelection?: boolean    // disabled when there is no selection
  children?: MenuItemSpec[]   // one level of submenu
}

export interface MenuGroup {
  id: string
  items: MenuItemSpec[]
}

export interface MenuContext {
  hasSelection: boolean
  /** True when the right-click landed on an image — swaps in the image menu. */
  image?: boolean
}

function item(id: string, key: keyof Messages, extra: Partial<MenuItemSpec> = {}): MenuItemSpec {
  return { id, label: t(key), ...extra }
}

/**
 * The context menu as pure data. Backend adapters map `item.id` to an edit.
 * `ctx` currently only gates enablement of selection-dependent items; the
 * structure itself is identical for rich and source.
 */
export function getMenuModel(ctx: MenuContext): MenuGroup[] {
  // Right-clicking an image shows a dedicated, minimal menu — the text-format
  // items are meaningless on an image node.
  if (ctx.image) {
    return [
      { id: 'image', items: [ item('copyImage', 'ctxmenu.copyImage') ] },
    ]
  }
  return [
    { id: 'clipboard', items: [
      item('cut', 'ctxmenu.cut'),
      item('copy', 'ctxmenu.copy'),
      item('paste', 'ctxmenu.paste'),
      item('selectAll', 'ctxmenu.selectAll'),
    ] },
    { id: 'emphasis', items: [
      item('question', 'ctxmenu.question', { emphasis: true, icon: 'question' }),
      item('note', 'ctxmenu.note', { emphasis: true, icon: 'sparkle' }),
      item('highlight', 'ctxmenu.highlight', { emphasis: true, icon: 'highlight' }),
      item('wikilink', 'ctxmenu.wikilink', { emphasis: true, icon: 'wikilink' }),
    ] },
    { id: 'marks', items: [
      item('bold', 'ctxmenu.bold', { icon: 'bold' }),
      item('italic', 'ctxmenu.italic', { icon: 'italic' }),
      item('strike', 'ctxmenu.strike', { icon: 'strike' }),
      item('code', 'ctxmenu.code', { icon: 'code' }),
    ] },
    { id: 'link', items: [
      item('link', 'ctxmenu.link', { needsSelection: true, icon: 'link' }),
    ] },
    { id: 'block', items: [
      item('heading', 'ctxmenu.heading', { icon: 'heading', children: [
        item('h1', 'ctxmenu.h1'), item('h2', 'ctxmenu.h2'), item('h3', 'ctxmenu.h3'),
      ] }),
      item('quote', 'ctxmenu.quote', { icon: 'quote' }),
      item('codeblock', 'ctxmenu.codeblock', { icon: 'codeblock' }),
      item('list', 'ctxmenu.list', { icon: 'list', children: [
        item('bullet', 'ctxmenu.bullet'), item('ordered', 'ctxmenu.ordered'), item('task', 'ctxmenu.task'),
      ] }),
      item('hr', 'ctxmenu.hr', { icon: 'hr' }),
    ] },
    { id: 'insert', items: [
      item('insert', 'ctxmenu.insert', { icon: 'insert', children: [
        item('table', 'ctxmenu.table'), item('image', 'ctxmenu.image'),
        item('math', 'ctxmenu.math'), item('mermaid', 'ctxmenu.mermaid'),
        item('date', 'ctxmenu.date'),
      ] }),
    ] },
  ]
}
