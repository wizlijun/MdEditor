// GENERATED COPY — do not edit. Source: src/lib/agent-picker/types.ts. Run `node scripts/sync-agent-picker.mjs` after changing it.
// The shape every agent picker renders from, whichever surface it appears on.
//
// CANONICAL COPY: src/lib/agent-picker/types.ts. The plugin copies are kept
// byte-identical by a test; edit this file and rerun
// `node scripts/sync-agent-picker.mjs`.

/** One harness, as its agent plugin's `harness-status` reports it. */
export interface AgentHarness {
  /** The harness's own name: "Claude Code", "DeepSeek Harness". */
  harness: string
  /** Is it usable? Everything else is decoration if this is false. */
  ok: boolean
  version?: string | null
  /** Where it resolved from — a path, or "monorepo checkout at …". Also the
   *  tell between "not installed" and "installed but will not start". */
  origin?: string
  /** The model a run uses when its task pins none. */
  default_model?: string | null
  hint?: string | null
  /** An environment-level failure from the newest run — expired credentials, a
   *  rate limit. The run would fail the same way again. */
  warning?: string | null
}

/** One installed agent plugin, with the harness behind it. */
export interface AgentOption {
  /** Plugin id: `notemd.claude-agent`. */
  id: string
  /** Plugin display name, as a fallback when the harness will not answer. */
  name: string
  /** null when the plugin could not be asked. */
  harness?: AgentHarness | null
}

/** `host.agent.providers` answer. */
export interface AgentProviders {
  providers: Array<AgentOption & { selected?: boolean }>
  /** What the host would pick when a caller names no harness. */
  default: string
}

/**
 * Remember one surface's choice.
 *
 * Per surface, not global: answering a note with Claude while having the ebook
 * queue read with DeepSeek is a reasonable thing to want, and a single shared
 * setting cannot express it.
 *
 * A choice that names an agent which is no longer installed falls back rather
 * than failing — uninstalling a plugin must not wedge the button beside it.
 */
export function rememberedProvider(
  surface: string,
  installed: string[],
  fallback: string,
  storage: Pick<Storage, 'getItem'> = localStorage,
): string {
  let saved: string | null = null
  try {
    saved = storage.getItem(providerKey(surface))
  } catch {
    // Private mode, or a webview with storage disabled. The fallback is right.
  }
  if (saved && installed.includes(saved)) return saved
  return installed.includes(fallback) ? fallback : (installed[0] ?? fallback)
}

export function rememberProvider(
  surface: string,
  id: string,
  storage: Pick<Storage, 'setItem'> = localStorage,
): void {
  try {
    storage.setItem(providerKey(surface), id)
  } catch {
    // Not being able to remember the choice is not a reason to refuse it.
  }
}

export function providerKey(surface: string): string {
  return `notemd.agent.provider.${surface}`
}

// ── where the menu goes ─────────────────────────────────────────────────────

export interface Rect {
  top: number
  left: number
  width: number
  height: number
}

export interface Placement {
  top: number
  left: number
  /** Which way it opened, for the caller's styling/animation. */
  side: 'up' | 'down'
}

/**
 * Place the menu relative to its button, in VIEWPORT coordinates.
 *
 * Viewport coordinates because the menu renders `position: fixed`: the button
 * sits inside scrolling panels and list rows, and an absolutely-positioned menu
 * is clipped by the first ancestor with `overflow` — no amount of choosing a
 * direction fixes that, since the clip happens before the menu ever reaches the
 * window edge.
 *
 * Preference order, and why:
 *  - **Below, right-aligned.** The button is right-of-centre in every surface
 *    that uses it, and reading down from what you clicked is the default habit.
 *  - **Flip up** when there is not enough room below and more room above — a
 *    row near the bottom of a long ebook queue is the common case.
 *  - **Flip to left-aligned** when right-aligning would push it off the left
 *    edge, which happens in the narrow sidecar panel.
 *  - **Clamp** as the last resort, so a menu taller than the viewport still
 *    has its top on screen rather than its middle.
 */
export function placeMenu(
  anchor: Rect,
  menu: { width: number; height: number },
  viewport: { width: number; height: number },
  gap = 4,
  margin = 8,
): Placement {
  const below = anchor.top + anchor.height + gap
  const above = anchor.top - gap - menu.height
  const roomBelow = viewport.height - margin - below
  const roomAbove = anchor.top - gap - margin

  let side: 'up' | 'down' = 'down'
  if (menu.height > roomBelow && roomAbove > roomBelow) side = 'up'
  let top = side === 'down' ? below : above

  // Right-aligned to the button, falling back to left-aligned, then clamped.
  let left = anchor.left + anchor.width - menu.width
  if (left < margin) left = anchor.left
  left = Math.min(Math.max(left, margin), Math.max(margin, viewport.width - menu.width - margin))
  top = Math.min(Math.max(top, margin), Math.max(margin, viewport.height - menu.height - margin))
  return { top, left, side }
}
