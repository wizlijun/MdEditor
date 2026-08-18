// The window's icon set — same house rules as idea-spark's (strokes only,
// `currentColor`, primitives only; wrapper lives in components/Icon.svelte).
export const ICONS = {
  /** A magnifying glass — the same glyph as the host's right-click 「溯源」
   *  item (src/lib/context-menu/icons.ts `trace`), so the entry point and the
   *  window it opens visibly belong together. Standing obligation: if that
   *  glyph changes, re-copy it here. */
  trace: '<circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.65" y2="16.65"/>',
  /** Node → arrow → node: handing the passage over to an agent. Verbatim from
   *  idea-spark's `delegate` — the same verb everywhere is the same glyph. */
  delegate:
    '<circle cx="4" cy="12" r="1.5"/>' +
    '<circle cx="20" cy="12" r="1.5"/>' +
    '<line x1="7.5" y1="12" x2="16.5" y2="12"/>' +
    '<polyline points="13 8.5 16.5 12 13 15.5"/>',
} as const

/** Every icon this window knows how to draw. */
export type IconName = keyof typeof ICONS
