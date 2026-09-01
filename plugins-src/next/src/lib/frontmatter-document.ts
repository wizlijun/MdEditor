import { Document, isMap, parseDocument } from 'yaml'

export interface EditableFrontmatter {
  document: Document.Parsed
  body: string
  newline: '\n' | '\r\n'
}

/** Parse editable YAML frontmatter without normalizing the Markdown body. */
export function editableFrontmatter(markdown: string, allowMissing: boolean): EditableFrontmatter {
  let start: number
  let newline: '\n' | '\r\n'
  if (markdown.startsWith('---\r\n')) {
    start = 5
    newline = '\r\n'
  } else if (markdown.startsWith('---\n')) {
    start = 4
    newline = '\n'
  } else if (allowMissing) {
    const document = new Document({}) as Document.Parsed
    return { document, body: markdown, newline: markdown.includes('\r\n') ? '\r\n' : '\n' }
  } else {
    throw new Error('Document must start with YAML frontmatter')
  }

  const closing = /^---[ \t]*(?:\r?\n|$)/gm
  closing.lastIndex = start
  const match = closing.exec(markdown)
  if (!match) throw new Error('Document frontmatter is not closed')

  const document = parseDocument(markdown.slice(start, match.index), { prettyErrors: false })
  if (document.errors.length > 0) {
    throw new Error(`Document frontmatter is invalid YAML: ${document.errors[0]?.message ?? 'unknown error'}`)
  }
  if (!isMap(document.contents)) throw new Error('Document frontmatter must be a mapping')
  return {
    document,
    body: markdown.slice(match.index + match[0].length),
    newline,
  }
}

/** Serialize edited frontmatter while leaving every Markdown body byte untouched. */
export function serializeEditableFrontmatter(input: EditableFrontmatter): string {
  const yaml = input.document.toString().trimEnd().replace(/\n/g, input.newline)
  return `---${input.newline}${yaml}${input.newline}---${input.newline}${input.body}`
}
