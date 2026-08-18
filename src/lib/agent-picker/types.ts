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
