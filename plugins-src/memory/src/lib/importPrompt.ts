import raw from './import-prompt.md?raw'

/**
 * The prompt a user copies into another AI assistant so it exports its own
 * memory entries as `notemd memory propose` commands. Kept as Markdown so it
 * stays readable and diffable; the copied text is exactly what ships here.
 */
export const importPrompt = raw.trim()
