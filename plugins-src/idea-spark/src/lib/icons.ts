// The window's icon set — one table, because nearly every icon here is used
// more than once (`delegate` sits in the action bar AND in the row menu;
// `running` sits in the action bar's progress line AND on an inbox row at a
// smaller size). Pasting the same path data into two components is how two
// copies of "the same" icon quietly drift apart.
//
// Each entry is the *inner* markup of one 24×24 icon. The shared wrapper —
// `viewBox`, `fill="none"`, `stroke="currentColor"`, `stroke-width="2"`, round
// caps and joins, `aria-hidden` — lives in `components/Icon.svelte` and is
// stated exactly once; only the size varies (16 in the action bar and the
// menu, 12 for an inbox row's status badge).
//
// House rules for anything added here, all of them enforced by `icons.test.ts`:
//   * strokes only — no `fill`, so nothing turns into a solid blob at 12px;
//   * no color of its own — the icon takes `currentColor`, which is what lets
//     `failed` be tinted with the warning color and `delete` with the danger
//     color purely from CSS;
//   * primitives only (`path`/`circle`/`line`/`polyline`/`rect`) — no
//     `<image>`, `<filter>`, `<mask>`, `<style>`.
//
// The path data is transcribed verbatim from the reviewed source SVGs
// (docs/superpowers/specs/2026-08-05-idea-spark-icons-prompt.md describes the
// spec they were drawn against); it is deliberately not "tidied up".

export const ICONS = {
  /** A lightbulb: a spark, not a "new file". */
  'new-idea':
    '<path d="M9 16a7 7 0 1 1 6 0c-.7.7-1 1.3-1 2h-4c0-.7-.3-1.3-1-2z"/>' +
    '<line x1="10" y1="21" x2="14" y2="21"/>',
  /** Node → arrow → node: handing the idea over to someone else. */
  delegate:
    '<circle cx="4" cy="12" r="1.5"/>' +
    '<circle cx="20" cy="12" r="1.5"/>' +
    '<line x1="7.5" y1="12" x2="16.5" y2="12"/>' +
    '<polyline points="13 8.5 16.5 12 13 15.5"/>',
  /** A tray — a container of many things, distinct from a document. */
  inbox:
    '<path d="M3 9h18l-2 10H5L3 9z"/>' +
    '<polyline points="3 13 8 13 10 16 14 16 16 13 21 13"/>',
  /** A 6-toothed gear; more teeth smear into a circle at 16px. */
  settings:
    '<path d="M9 3v2.2L6.8 6.5 5 5.4 3 9l2 1.1v3.8L3 15l2 3.6 1.8-1.1L9 18.8V21h6v-2.2l2.2-1.3 1.8 1.1 2-3.6-2-1.1v-3.8L21 9l-2-3.6-1.8 1.1L15 5.2V3z"/>' +
    '<circle cx="12" cy="12" r="3"/>',
  /** An hourglass: work in flight. Static — the wait is minutes, not frames. */
  running:
    '<path d="M6 3h12M6 21h12"/>' +
    '<path d="M8 3c0 4 1.5 6 4 9-2.5 3-4 5-4 9m8-18c0 4-1.5 6-4 9 2.5 3 4 5 4 9"/>',
  /** A warning triangle holding a cross rather than an exclamation mark. */
  failed: '<path d="M12 3 2.5 20.5h19L12 3z"/>' + '<path d="m9.5 10.5 5 5m0-5-5 5"/>',
  /** Box with an arrow leaving it: open this somewhere else. */
  'open-idea':
    '<path d="M13 4H5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2v-8"/>' +
    '<polyline points="15 3 21 3 21 9"/>' +
    '<line x1="10" y1="14" x2="21" y2="3"/>',
  /** A document with a spark beside it — the ✦ ("written by AI") convention,
   *  reduced to two strokes and kept clear of the page so it reads at 16px. */
  'open-proof':
    '<path d="M4 3h6l4 4v14H4z"/>' +
    '<polyline points="10 3 10 7 14 7"/>' +
    '<path d="M19 4.5v5M16.5 7h5"/>',
  /** A name tag: editing the label, not the contents. */
  rename:
    '<path d="M3 6h11l7 6-7 6H3z"/>' +
    '<line x1="6" y1="10" x2="12" y2="10"/>' +
    '<line x1="6" y1="14" x2="11" y2="14"/>',
  /** A bin with two staves — three would smear together at this size. */
  delete:
    '<line x1="3" y1="6" x2="21" y2="6"/>' +
    '<path d="M9 6V3h6v3M6 6l1 15h10l1-15"/>' +
    '<path d="M10 11v6m4-6v6"/>',
} as const

/** Every icon this window knows how to draw. */
export type IconName = keyof typeof ICONS
