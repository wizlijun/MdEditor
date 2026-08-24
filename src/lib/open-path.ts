import { openFile } from './tabs.svelte'

/**
 * Open a path handed to the app from outside — CLI argv (`notemd .`,
 * `notemd xxx.md`), a Finder double-click, a deep link.
 *
 * A file becomes a tab; a **directory** becomes the folder view's root and
 * reveals the left panel. That split lives here rather than in `openFile`
 * because a directory is not a document: it has no content to read, no tab, no
 * dirty state — `openFile` would fall through to its plain-text branch and fail
 * on `readTextFile`.
 *
 * The path must already be absolute: callers upstream (Rust `argv_open_target`)
 * resolve it against the launching process's cwd, which the webview has no
 * access to.
 */
export async function openPath(path: string): Promise<void> {
  if (await isDirectory(path)) {
    const { setRootDir } = await import('./folder-view.svelte')
    const { setActiveView, setSideVisible } = await import('./side-panel/registry.svelte')
    await setRootDir(path)
    await setActiveView('left', 'folder-view')
    await setSideVisible('left', true)
    return
  }
  await openFile(path)
}

/**
 * A failed stat reads as "not a directory" on purpose: the path then goes to
 * `openFile`, which reports a missing file the way it always has, instead of
 * this helper inventing a second error message for the same situation.
 */
async function isDirectory(path: string): Promise<boolean> {
  try {
    const { stat } = await import('@tauri-apps/plugin-fs')
    return (await stat(path)).isDirectory
  } catch {
    return false
  }
}
