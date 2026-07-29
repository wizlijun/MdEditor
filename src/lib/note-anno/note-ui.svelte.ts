// Shared UI state for annotation popovers. Rich mode writes it from DOM
// event handlers; NotePopover / NoteEditPopup render from it.

/**
 * The current document theme (mint paper, serif type, etc.) lives scoped inside
 * `[data-theme] .moraya-editor`; the note popups are position:fixed siblings
 * mounted outside that scope, so they'd otherwise fall back to the system
 * Canvas/CanvasText chrome and the system UI font, clashing with a themed
 * document. We read the editor's *rendered* colors and typography at open time
 * so the popups match any theme — hardcoded palettes, system colors, custom
 * fonts, anything — without the compiler having to export variables.
 *
 * Colors come from the annotated element itself (`fg`), climbing ancestors for
 * the first opaque background (`bg`) because the editor container is often
 * `background: transparent`. Typography comes from the editor content root
 * (`.moraya-editor`), not the inline annotation span, so badge-sized or
 * superscript runs don't leak their shrunk metrics into the popup.
 */
export interface ThemeStyle {
  bg: string
  fg: string
  fontFamily: string
  fontSize: string
  fontWeight: string
  fontStyle: string
  lineHeight: string
  letterSpacing: string
  fontFeatureSettings: string
}

export function readThemeStyle(el: Element | null | undefined): ThemeStyle | null {
  if (!el) return null
  const cs = getComputedStyle(el)
  const fg = cs.color
  let node: Element | null = el
  let bg = ''
  while (node) {
    const c = getComputedStyle(node).backgroundColor
    // Skip fully-transparent backgrounds; keep climbing to the painted surface.
    if (c && c !== 'transparent' && !/rgba?\([^)]*,\s*0\s*\)$/.test(c)) { bg = c; break }
    node = node.parentElement
  }
  if (!bg) return null
  // Typography from the editor content root so we get the theme's body type,
  // not the annotation run's (which may be superscript/badge-sized).
  const typo = (el.closest('.moraya-editor') as Element | null) ?? el
  const t = typo === el ? cs : getComputedStyle(typo)
  return {
    bg,
    fg,
    fontFamily: t.fontFamily,
    fontSize: t.fontSize,
    fontWeight: t.fontWeight,
    fontStyle: t.fontStyle,
    lineHeight: t.lineHeight,
    letterSpacing: t.letterSpacing,
    fontFeatureSettings: t.fontFeatureSettings,
  }
}

/** Serialize a captured theme style into the `--note-*` custom properties the
 *  popup components read. Empty string when no style was captured, so the
 *  components fall back to their system-chrome defaults. */
export function styleVars(s: ThemeStyle | null | undefined): string {
  if (!s) return ''
  return [
    `--note-bg:${s.bg}`,
    `--note-fg:${s.fg}`,
    `--note-font-family:${s.fontFamily}`,
    `--note-font-size:${s.fontSize}`,
    `--note-font-weight:${s.fontWeight}`,
    `--note-font-style:${s.fontStyle}`,
    `--note-line-height:${s.lineHeight}`,
    `--note-letter-spacing:${s.letterSpacing}`,
    `--note-font-feature:${s.fontFeatureSettings}`,
  ].join('; ')
}

export interface NoteEditState {
  x: number
  y: number
  note: string
  /** Collapse the popup's caret here instead of selecting the whole note.
   *  Ask uses 0 so the pre-filled `?` isn't wiped by the first keystroke. */
  caret?: number
  style?: ThemeStyle | null
  save: (note: string) => void
  remove: () => void
}

export interface NoteHoverState {
  x: number
  y: number
  note: string
  style?: ThemeStyle | null
}

export const noteUi = $state({
  edit: null as NoteEditState | null,
  hover: null as NoteHoverState | null,
})
