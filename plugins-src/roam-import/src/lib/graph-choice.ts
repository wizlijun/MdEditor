// Which Roam graph the CLI sync reads from — the decision alone, kept out of
// App.svelte so it can be tested (the repo does not unit-test components).

/**
 * The graph choice to persist after a probe.
 *
 * `current` is what the user picked (persisted in localStorage; `''` means
 * "let the CLI auto-select", which it only does when exactly one graph is
 * configured). `graphs` is what `plugin.probe` just reported.
 *
 * The rule is that a persisted choice is only ever thrown away when the probe
 * **contradicts** it. A probe that succeeds but reports *no* graphs — state
 * `missing` or `not_connected`, which is what an entirely healthy machine
 * answers when the Roam desktop app simply is not running — knows nothing
 * about which graph the user wants and must leave the choice alone. Clearing
 * it there loses a two-graph user's pick to a window they happened to open at
 * the wrong moment, and the next good probe then silently re-assigns
 * `graphs[0]` — after which a sync pulls a *different* graph's day into that
 * date's note without saying anything.
 */
export function reconcileGraphChoice(current: string, graphs: readonly string[]): string {
  // Nothing to contradict it with.
  if (graphs.length === 0) return current
  // Confirmed by the probe: keep it, one graph or ten. (Sending an explicit
  // `--graph` for the only configured graph is what the CLI would auto-select
  // anyway.)
  if (graphs.includes(current)) return current
  // Contradicted. With one graph there is no choice to make, so fall back to
  // "let the CLI auto-select" rather than pinning a name; with several, offer
  // the first — the picker is shown in that case, so the user can see and
  // change it.
  return graphs.length === 1 ? '' : graphs[0]
}
