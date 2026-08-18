// Feather/Lucide-style line icons matching the app toolbar (see ModeToggle.svelte):
// 24×24 viewBox, no fill, currentColor stroke, round caps/joins. Each entry is the
// inner markup; `iconSvg` wraps it in a sized <svg> for injection via {@html}.
// The brand ✦ sparkle from the app icon (orange fill). Path coordinates come
// from notemd-favicon-simple.svg (512-space), mapped into 24×24 via transform.
export const SPARKLE_PATH =
  '<path transform="translate(12 12) scale(0.083) translate(-185.5 -203)" ' +
  'd="M 185.49318,76.468676 C 202.86539,165.0158 220.23759,183.99019 301.30788,202.96457 ' +
  '220.23759,221.93895 202.86539,240.91333 185.49318,329.46046 168.12097,240.91333 ' +
  '150.74877,221.93895 69.67847,202.96457 150.74877,183.99019 168.12097,165.0158 ' +
  '185.49318,76.468676 Z" fill="#f59e0b" stroke="none"/>'

// `??` — two stroked question marks side by side, echoing the ⁇ on the note
// popup's Ask button. Each glyph is the Feather help-circle hook + dot, shifted
// left/right so the pair reads as one mark at 16 px.
const QUESTION_GLYPH = '<path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><line x1="12" y1="17" x2="12.01" y2="17"/>'

const PATHS: Record<string, string> = {
  sparkle:    SPARKLE_PATH,
  question:   `<g transform="translate(-4.5 0)">${QUESTION_GLYPH}</g><g transform="translate(3.5 0)">${QUESTION_GLYPH}</g>`,
  highlight:  '<rect x="9" y="4" width="6" height="16" rx="1.5" fill="#facc15" stroke="none"/>',
  wikilink:   '<path d="M5.5 5H3v14h2.5"/><path d="M10.5 5H8v14h2.5"/><path d="M18.5 5H21v14h-2.5"/><path d="M13.5 5H16v14h-2.5"/>',
  trace:      '<circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.65" y2="16.65"/>',
  note:       '<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/><line x1="8" y1="8" x2="16" y2="8"/><line x1="8" y1="12" x2="12" y2="12"/>',
  trash:      '<polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><line x1="10" y1="11" x2="10" y2="17"/><line x1="14" y1="11" x2="14" y2="17"/>',
  bold:       '<path d="M6 4h8a4 4 0 0 1 4 4 4 4 0 0 1-4 4H6z"/><path d="M6 12h9a4 4 0 0 1 4 4 4 4 0 0 1-4 4H6z"/>',
  italic:     '<line x1="19" y1="4" x2="10" y2="4"/><line x1="14" y1="20" x2="5" y2="20"/><line x1="15" y1="4" x2="9" y2="20"/>',
  strike:     '<path d="M16 4H9a3 3 0 0 0-2.83 4"/><path d="M14 12a4 4 0 0 1 0 8H6"/><line x1="4" y1="12" x2="20" y2="12"/>',
  code:       '<polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/>',
  link:       '<path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/>',
  heading:    '<polyline points="4 7 4 4 20 4 20 7"/><line x1="9" y1="20" x2="15" y2="20"/><line x1="12" y1="4" x2="12" y2="20"/>',
  quote:      '<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>',
  codeblock:  '<polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/>',
  list:       '<line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/>',
  hr:         '<line x1="5" y1="12" x2="19" y2="12"/>',
  insert:     '<line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>',
}

/** Full <svg> markup for a context-menu icon name, or '' if unknown. */
export function iconSvg(name: string | undefined): string {
  if (!name || !PATHS[name]) return ''
  return `<svg class="ctx-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${PATHS[name]}</svg>`
}
