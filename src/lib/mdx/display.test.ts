import { describe, it, expect } from 'vitest'
import { toDisplayMarkdown } from './display'

describe('toDisplayMarkdown', () => {
  it('wraps a top-level import line in a jsx fence', () => {
    const src = "import Chart from '../components/Chart.astro'\n\n# Title\n"
    expect(toDisplayMarkdown(src)).toBe(
      "```jsx\nimport Chart from '../components/Chart.astro'\n```\n\n# Title\n",
    )
  })

  it('groups a consecutive import block into one fence', () => {
    const src = "import A from 'a'\nimport B from 'b'\nexport const x = 1\n\ntext\n"
    expect(toDisplayMarkdown(src)).toBe(
      "```jsx\nimport A from 'a'\nimport B from 'b'\nexport const x = 1\n```\n\ntext\n",
    )
  })

  it('leaves sample code inside an existing fence alone', () => {
    const src = "# Doc\n\n```ts\nimport A from 'a'\n```\n\ndone\n"
    expect(toDisplayMarkdown(src)).toBe(src)
  })

  it('wraps a self-closing block-level component', () => {
    const src = '# T\n\n<Chart data={items} />\n\ntext\n'
    expect(toDisplayMarkdown(src)).toBe('# T\n\n```jsx\n<Chart data={items} />\n```\n\ntext\n')
  })

  it('wraps a multi-line component through its closing tag', () => {
    const src = '<Callout type="warn">\n  indented body\n</Callout>\n\ntext\n'
    expect(toDisplayMarkdown(src)).toBe(
      '```jsx\n<Callout type="warn">\n  indented body\n</Callout>\n```\n\ntext\n',
    )
  })

  it('leaves lowercase html blocks to the markdown renderer', () => {
    // <div> is valid markdown-embedded HTML and renders fine; only JSX
    // components (capitalised or dotted) need the code-fence treatment.
    const src = '<div align="center">\n  hi\n</div>\n'
    expect(toDisplayMarkdown(src)).toBe(src)
  })

  it('leaves plain markdown untouched', () => {
    const src = '# Title\n\nSome **bold** text.\n\n- a\n- b\n'
    expect(toDisplayMarkdown(src)).toBe(src)
  })
})
