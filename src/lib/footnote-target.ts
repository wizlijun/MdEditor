/**
 * 从脚注定义块里找出它指向的"来源地址"。
 *
 * 脚注在实际使用中多是**出处**:一条 `[^loop]: /2026-07-27-….md（说明）` 就是
 * "这个论断来自那篇笔记"。所以点定义前的编号,期待的是打开那个来源,而不是跳回
 * 正文里引用它的位置。
 *
 * 地址有三种写法,按可靠性排序:
 *  1. `[[wikilink]]` —— 已由编辑器渲染成元素,直接读属性
 *  2. `[文字](地址)` / 自动链接 —— 渲染成 `<a href>` 或带 `data-url` 的元素
 *  3. 裸路径 `/dailynote/2026/2026-07-21.note.md` —— 纯文本,只能靠正则捞
 *
 * 第 3 种是 vault 里最常见的写法(agent 生成的来源标注),偏偏它不是链接语法,
 * 拿不到 DOM 属性,所以必须兜底。
 */

export type FootnoteTarget =
  | { kind: 'wikilink'; value: string }
  | { kind: 'href'; value: string }

/** http(s) 链接。止于空白、尖括号、引号,以及中文全角括号(来源说明常紧跟其后)。 */
const URL_RE = /https?:\/\/[^\s（）()<>"'，、。；]+/

/**
 * 裸文件路径:`/` 或 `./` 开头、带扩展名。
 * 同样止于全角括号 —— `/x.md（说明）` 里的说明不属于路径。
 */
const PATH_RE = /(?:^|[\s（(])((?:\.{0,2}\/)[^\s（）()<>"'，、；]*\.[A-Za-z0-9]{1,8})/

/** 去掉末尾的标点,`见 /a/b.md。` 这类写法不该把句号算进路径。 */
function trimTrailingPunctuation(s: string): string {
  return s.replace(/[。，、；：!?.,;:]+$/, '')
}

/**
 * @param defEl 脚注定义块元素(`[data-footnote-def]`)
 * @returns 找到的地址;整条定义纯属文字说明(如 `Watkins 2008, Psychological
 *          Bulletin`)时返回 null —— 这种没有可打开的东西。
 */
export function findFootnoteTarget(defEl: HTMLElement): FootnoteTarget | null {
  const wiki = defEl.querySelector('[data-wikilink]')
  if (wiki) {
    const value = wiki.getAttribute('data-wikilink') || ''
    if (value) return { kind: 'wikilink', value }
  }

  const urlEl = defEl.querySelector('[data-url]')
  if (urlEl) {
    const value = urlEl.getAttribute('data-url') || ''
    if (value) return { kind: 'href', value }
  }

  const anchor = defEl.querySelector('a[href]')
  if (anchor) {
    const value = anchor.getAttribute('href') || ''
    if (value) return { kind: 'href', value }
  }

  return findAddressInText(defEl.textContent || '')
}

/** 从纯文本里捞地址。导出供单测直接覆盖第 3 种写法。 */
export function findAddressInText(text: string): FootnoteTarget | null {
  const url = URL_RE.exec(text)
  if (url) return { kind: 'href', value: trimTrailingPunctuation(url[0]) }

  const path = PATH_RE.exec(text)
  if (path?.[1]) return { kind: 'href', value: trimTrailingPunctuation(path[1]) }

  return null
}
