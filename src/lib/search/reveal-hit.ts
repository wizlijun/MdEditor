/** Wire payload emitted by the Rust smart-search bridge. */
export interface SearchHitRevealRequest {
  requestId: string
  path: string
  /** 1-based source line. */
  line: number
  anchor: string
}

export interface SearchHitRevealDeps {
  openFile(path: string): Promise<void>
  activePath(): string | null
  reveal(line: number, anchor: string, path: string): void
}

/**
 * Open the hit in the main-window tab store, then reveal against the path the
 * editor actually opened. `openFile` may redirect a Vault mirror to its source,
 * so binding the reveal to the incoming path would make the new editor reject
 * the request as belonging to another document.
 */
export async function openAndRevealSearchHit(
  request: SearchHitRevealRequest,
  deps: SearchHitRevealDeps,
): Promise<string> {
  await deps.openFile(request.path)
  const finalPath = deps.activePath() || request.path
  deps.reveal(request.line, request.anchor, finalPath)
  return finalPath
}
