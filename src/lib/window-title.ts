/** The marker shown on a document that lives OUTSIDE the vault but has a vault
 *  mirror. Opening the mirror redirects to the source (`tabs.svelte.ts`), so the
 *  file under the cursor is always the source — this says so. */
export const SYNC_MARK = '↔'

/** Window title: the document name when a single tab is open, plain otherwise.
 *  A mirrored source carries the marker so it's visible even with no tab bar. */
export function windowTitleFor(docTitle: string | null, mirroredSource: boolean): string {
  if (!docTitle) return 'note.md'
  return `${mirroredSource ? `${SYNC_MARK} ` : ''}${docTitle} — note.md`
}
