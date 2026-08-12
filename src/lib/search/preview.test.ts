import { describe, it, expect } from 'vitest'
import { cleanLineText, previewLine, parseHighlightTerms, highlightParts } from './preview'

describe('cleanLineText', () => {
  it('剥 HTML 标签但保留标签间文字', () => {
    expect(cleanLineText('新对话<br/>抽取出的事实')).toBe('新对话 抽取出的事实')
    expect(cleanLineText('<span class="x">看得见</span>')).toBe('看得见')
  })

  it('解码 HTML 实体,且不对解码结果再剥一次标签', () => {
    // 原文写 `&lt;div&gt;` 是想让人看见 `<div>` 这几个字。先剥标签再解码,
    // 解码出来的尖括号就不会被当成标签吃掉。
    expect(cleanLineText('用 &lt;div&gt; 包起来')).toBe('用 <div> 包起来')
    expect(cleanLineText('A&nbsp;B &amp; C &#39;q&#39; &#x27;r&#x27;')).toBe("A B & C 'q' 'r'")
  })

  it('图片整体删除,连 alt 一起', () => {
    expect(cleanLineText('看这张 ![架构图](a.png) 图')).toBe('看这张 图')
    expect(cleanLineText('嵌入 ![[diagram.png]] 完')).toBe('嵌入 完')
  })

  it('wikilink 取显示名', () => {
    expect(cleanLineText('见 [[外骨骼笔记|那篇]] 里')).toBe('见 那篇 里')
    expect(cleanLineText('见 [[外骨骼笔记]] 里')).toBe('见 外骨骼笔记 里')
  })

  it('wikilink 在行内标记之前处理,下划线不被当强调吃掉', () => {
    expect(cleanLineText('[[a_b_c]]')).toBe('a_b_c')
  })

  it('markdown 链接取文字', () => {
    expect(cleanLineText('参考 [规范](https://x/y) 第三节')).toBe('参考 规范 第三节')
  })

  it('CriticMarkup:高亮/新增留文字,删除/批注整体去掉,替换留新值', () => {
    expect(cleanLineText('{==重点==}在这')).toBe('重点在这')
    expect(cleanLineText('{++补的++}在这')).toBe('补的在这')
    expect(cleanLineText('前{--删的--}后')).toBe('前后')
    expect(cleanLineText('前{>>这是批注<<}后')).toBe('前后')
    expect(cleanLineText('前{~~旧~>新~~}后')).toBe('前新后')
  })

  it('剥行首块标记', () => {
    expect(cleanLineText('## 标题')).toBe('标题')
    expect(cleanLineText('> 引用')).toBe('引用')
    expect(cleanLineText('- 列表项')).toBe('列表项')
    expect(cleanLineText('  3. 有序项')).toBe('有序项')
    expect(cleanLineText('- [ ] 待办')).toBe('待办')
    expect(cleanLineText('- [x] 已办')).toBe('已办')
  })

  it('剥行内标记', () => {
    expect(cleanLineText('**粗** *斜* `码` ~~删~~ ^^亮^^ ==高==')).toBe('粗 斜 码 删 亮 高')
  })

  it('分隔线整行清空', () => {
    expect(cleanLineText('---')).toBe('')
    expect(cleanLineText('***')).toBe('')
    expect(cleanLineText('___')).toBe('')
  })

  it('折叠空白并 trim', () => {
    expect(cleanLineText('   a     b   ')).toBe('a b')
  })

  it('JSON 结构符号原样保留', () => {
    expect(cleanLineText('    "entity_boost": ...,     # 实体加成贡献')).toBe(
      '"entity_boost": ..., # 实体加成贡献',
    )
  })
})

describe('previewLine', () => {
  it('优先返回包含关键词的那一行,而不是块首行', () => {
    const block = ['## 记忆检索', '这一行没有', '外骨骼的能量回收路径'].join('\n')
    expect(previewLine(block, ['外骨骼'])).toEqual({ text: '外骨骼的能量回收路径', lang: null })
  })

  it('关键词被行内标记切开时也能选中该行(匹配跑在清洗后的文本上)', () => {
    expect(previewLine('前言\n**外**骨骼的髋关节', ['外骨骼'])).toEqual({
      text: '外骨骼的髋关节',
      lang: null,
    })
  })

  it('围栏内命中带语言标签,定界行本身不入选', () => {
    const block = ['```json', '{', '  "entity_boost": 0.3', '}', '```'].join('\n')
    expect(previewLine(block, ['entity_boost'])).toEqual({
      text: '"entity_boost": 0.3',
      lang: 'json',
    })
  })

  it('围栏外的命中 lang 为 null', () => {
    const block = ['```json', '{ "a": 1 }', '```', '外骨骼在正文里'].join('\n')
    expect(previewLine(block, ['外骨骼'])).toEqual({ text: '外骨骼在正文里', lang: null })
  })

  it('~~~ 也是围栏定界符,语言取 info string 首词', () => {
    const block = ['~~~mermaid graph', 'A --> B', '~~~'].join('\n')
    expect(previewLine(block, ['A --> B'])).toEqual({ text: 'A --> B', lang: 'mermaid' })
  })

  it('mermaid 节点里的 <br/> 被清掉', () => {
    const block = ['```mermaid', '  NEW["新对话<br/>抽取出的事实"] --> LLM', '```'].join('\n')
    expect(previewLine(block, ['新对话'])).toEqual({
      text: 'NEW["新对话 抽取出的事实"] --> LLM',
      lang: 'mermaid',
    })
  })

  it('无命中时回退到第一条清洗后非空的行', () => {
    const block = ['---', '# 标题', '正文'].join('\n')
    expect(previewLine(block, ['不存在的词'])).toEqual({ text: '标题', lang: null })
  })

  it('剥掉块首的 frontmatter', () => {
    const block = ['---', 'type: Book Summary', 'tags: [a]', '---', '正文在这'].join('\n')
    expect(previewLine(block, [])).toEqual({ text: '正文在这', lang: null })
  })

  it('剥掉 HTML 注释与 script/style 块', () => {
    expect(previewLine('<!-- 隐藏\n说明 -->\n可见', [])).toEqual({ text: '可见', lang: null })
    expect(previewLine('<style>\n.a { color: red }\n</style>\n可见', [])).toEqual({
      text: '可见',
      lang: null,
    })
    expect(previewLine('<script>\nvar a = 1\n</script>\n可见', [])).toEqual({
      text: '可见',
      lang: null,
    })
  })

  it('整块都是标记时返回空文本', () => {
    expect(previewLine('---\n***\n', [])).toEqual({ text: '', lang: null })
  })

  it('语言标签小写并截断到 12 字符', () => {
    const block = ['```VeryLongLanguageName', 'x', '```'].join('\n')
    expect(previewLine(block, ['x']).lang).toBe('verylonglang')
  })
})

describe('parseHighlightTerms', () => {
  it('空白分词', () => {
    expect(parseHighlightTerms('外骨骼 髋关节')).toEqual(['外骨骼', '髋关节'])
  })

  it('闭合引号内是短语,内部空白不分词', () => {
    expect(parseHighlightTerms('"能量 回收" 外骨骼')).toEqual(['能量 回收', '外骨骼'])
  })

  it('未闭合引号退化成普通词', () => {
    expect(parseHighlightTerms('"外骨骼')).toEqual(['外骨骼'])
  })

  it('丢弃过滤器 token —— 它们约束文件属性,不是正文内容', () => {
    expect(
      parseHighlightTerms('tag:x type:y path:z ext:md origin:human after:2026-01-01 before:2026-12-31 page:[[A]] 外骨骼'),
    ).toEqual(['外骨骼'])
  })

  it('空 query 得到空词表', () => {
    expect(parseHighlightTerms('   ')).toEqual([])
  })
})

describe('highlightParts', () => {
  it('切出命中段与非命中段', () => {
    expect(highlightParts('说到外骨骼时', ['外骨骼'])).toEqual([
      { text: '说到', hit: false },
      { text: '外骨骼', hit: true },
      { text: '时', hit: false },
    ])
  })

  it('大小写不敏感,且保留原文大小写', () => {
    expect(highlightParts('the Exo suit', ['exo'])).toEqual([
      { text: 'the ', hit: false },
      { text: 'Exo', hit: true },
      { text: ' suit', hit: false },
    ])
  })

  it('长词优先,已匹配区间不再参与后续匹配', () => {
    // '骨' 也在词表里,但 '外骨骼' 先占了这段,不能再切出嵌套段。
    expect(highlightParts('外骨骼', ['骨', '外骨骼'])).toEqual([{ text: '外骨骼', hit: true }])
  })

  it('多处出现全部命中', () => {
    expect(highlightParts('a X b X c', ['X'])).toEqual([
      { text: 'a ', hit: false },
      { text: 'X', hit: true },
      { text: ' b ', hit: false },
      { text: 'X', hit: true },
      { text: ' c', hit: false },
    ])
  })

  it('词表为空或文本为空时整体作为非命中段', () => {
    expect(highlightParts('abc', [])).toEqual([{ text: 'abc', hit: false }])
    expect(highlightParts('', ['a'])).toEqual([{ text: '', hit: false }])
  })
})
