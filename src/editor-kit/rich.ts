// Rich mode for the Editor Kit: a @moraya/core editor with the Tauri-backed
// pieces (renderer registry, link opener, spreadsheet / frontmatter views,
// insights analytics plugin) left out — a plugin webview has no Tauri IPC.
//
// The option values below mirror `mountRichEditor` in `src/lib/editor-bridge.ts`
// (that file cannot be imported here: it drags in tabs / insights / Tauri
// adapters). Keep the two in sync when either changes.

import { createEditor, setDocumentBaseDir, type MorayaEditorInstance } from '@moraya/core'
import { bridgeMediaResolver } from './media'

/** Base directory (absolute) used to resolve relative image paths in rich mode. */
export function setKitBaseDir(absoluteDir: string): void {
  setDocumentBaseDir(absoluteDir)
}

export async function mountRich(
  host: HTMLElement,
  initial: string,
  vaultRoot: string,
  onChange: (md: string) => void,
): Promise<MorayaEditorInstance> {
  return createEditor({
    container: host,
    initialContent: initial,
    mediaResolver: bridgeMediaResolver(vaultRoot),
    platform: { getCurrentFilePath: () => null, isMacOS: true },
    // Math / mermaid pull heavy renderers the kit's consumers do not need
    // (spec §3.4: kit options are narrowed for plugin windows).
    enableMath: false,
    enableMermaid: false,
    enableTableResize: true,
    enableImageSelection: false,
    enableHistory: true,
    // Do NOT auto-format inline markers as you type: `**`, `__`, `*`, `_`,
    // `` ` ``, `~~`, `^^`, `==` stay literal instead of collapsing into a mark.
    enableInlineMarkInputRules: false,
    // Marks parsed from the file still render; on the caret's line their source
    // delimiters are revealed (Live-Preview style) and re-render on exit.
    inlineSyntaxScope: 'line',
    onChange,
    changeDebounceMs: 200,
  })
}
