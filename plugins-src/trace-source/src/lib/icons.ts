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
  // The four below are verbatim copies from idea-spark's icon set — same
  // control in the same position must be the same glyph (its file documents
  // each one's provenance).
  /** "Fold this side panel away" — same as idea-spark's inbox header. */
  collapse:
    '<rect x="3" y="3" width="18" height="18" rx="2"/>' +
    '<line x1="15" y1="3" x2="15" y2="21"/>' +
    '<polyline points="8 9 11 12 8 15"/>',
  /** A tray — a container of many things, distinct from a document. */
  inbox:
    '<path d="M3 9h18l-2 10H5L3 9z"/>' +
    '<polyline points="3 13 8 13 10 16 14 16 16 13 21 13"/>',
  /** A 6-toothed gear; more teeth smear into a circle at 16px. */
  settings:
    '<path d="M9 3v2.2L6.8 6.5 5 5.4 3 9l2 1.1v3.8L3 15l2 3.6 1.8-1.1L9 18.8V21h6v-2.2l2.2-1.3 1.8 1.1 2-3.6-2-1.1v-3.8L21 9l-2-3.6-1.8 1.1L15 5.2V3z"/>' +
    '<circle cx="12" cy="12" r="3"/>',
  /** A document with a spark beside it — the ✦ ("written by AI") convention:
   *  a report is the agent's product. (idea-spark's `open-proof`.) */
  'open-report':
    '<path d="M4 3h6l4 4v14H4z"/>' +
    '<polyline points="10 3 10 7 14 7"/>' +
    '<path d="M19 4.5v5M16.5 7h5"/>',
  /** A bin with two staves — three would smear together at this size. */
  delete:
    '<line x1="3" y1="6" x2="21" y2="6"/>' +
    '<path d="M9 6V3h6v3M6 6l1 15h10l1-15"/>' +
    '<path d="M10 11v6m4-6v6"/>',
  /** An hourglass: work in flight. Static — the wait is minutes, not frames. */
  running:
    '<path d="M6 3h12M6 21h12"/>' +
    '<path d="M8 3c0 4 1.5 6 4 9-2.5 3-4 5-4 9m8-18c0 4-1.5 6-4 9 2.5 3 4 5 4 9"/>',
} as const

/** Every icon this window knows how to draw. */
export type IconName = keyof typeof ICONS
