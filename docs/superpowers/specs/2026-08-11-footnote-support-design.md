# Markdown 脚注 `[^label]` 支持 —— 设计

> 类型:设计规格 · 日期:2026-08-11
> 落点:`@moraya/core`(本地 fork `/Users/bruce/git/moraya-core`)+ mdeditor 宿主样式
> 触发:rich 模式下 `[^loop]` 不显示角标,且**往返会破坏源文件**

## 0 · 一句话

让 rich 模式认识脚注:角标可读可点,而磁盘上的 `.md` 一个字节都不许被改写。

## 1 · 为什么 —— 这不是缺样式,是数据破坏

rich 模式的 markdown 解析在 `@moraya/core` 的 `src/markdown.ts:33`,markdown-it 实例只启用了 table/strikethrough/deflist/texmath/mark,**没有任何脚注规则**,`schema.ts` 里也没有脚注节点。

后果不是"角标没渲染出来"那么轻。实测 `parseMarkdown → serializeMarkdown` 往返(2026-08-11):

| 输入 | rich 模式表现 | 保存后文件变成 |
|---|---|---|
| `脚注[^loop]。`<br>`[^loop]: 循环的注释。` | `[^loop]` 渲染成**超链接**,定义行**整行消失** | `脚注[^loop](循环的注释。)。` |
| `text[^loop] here.`<br>`[^loop]: This is a loop note.` | 两行都是纯文本 | `text\[^loop\] here.`<br>`\[^loop\]: This is a loop note.` |
| `只有引用[^loop]，没有定义。` | 纯文本 | `只有引用\[^loop\]，没有定义。` |

第一种是**永久数据丢失**:CommonMark 里 `[^loop]` 是合法 link label,而 `[^loop]: 循环的注释。` 中文内容不含空格 → markdown-it 的 `reference` 规则把整行当作**链接引用定义**吃掉,`[^loop]` 随即降级为 shortcut reference link。脚注定义就此消失,再存盘写回的是一个行内链接。英文脚注只因内容带空格、destination 解析失败才侥幸留下文本。

第二三种也不无辜:反斜杠转义每次往返都是既成改写,且 Obsidian 打开就看到 `\[^loop\]` —— 直接违反 CLAUDE.md 的 file-over-app 硬原则。

## 2 · 上游调查结论(已排除)

- **真上游是 `zouwei/moraya-core`**(npm `@moraya/core`,最新 0.19.1,2026-08-09)。下载 0.19.1 跑同一组往返测试,**三种破坏逐字一致** —— 上游没解决,升级或等待都无效。`dist/**/*.js` 里没有任何脚注逻辑(CHANGELOG 中 `footnote` 只出现在"待办 fixtures"清单)。
- **上游没留扩展点。** 0.19.1 的 markdown 导出面仍只有 `parseMarkdown` / `parseMarkdownAsync` / `serializeMarkdown` 三个纯函数,markdown-it 实例是模块级私有,`CreateEditorOptions` 里没有 tokenizer/plugin 注入位。宿主侧无论如何碰不到这条链路。
- **本地已是分叉。** `/Users/bruce/git/moraya-core` 的 origin 是 `wizlijun/moraya-core`,停在 0.1.0,在初始抽取之上叠了 48 个 commit 的 mdeditor 专属改动(CriticMarkup annotation、`^^` caret_highlight、`note_anchor`、schema mark 顺序修正)。分叉早已发生,脚注不改变这个事实;"回归上游"是独立且大得多的决策,不在本设计范围。

**结论:必须在本地 fork 改 core,没有第二条路。**

## 3 · 为什么不用 `markdown-it-footnote`

实测 `markdown-it-footnote@4.0.0` 的 token 行为(2026-08-11):

- ✅ `footnote_ref.meta.label` 保留原名(`loop`),往返能还原 label
- ❌ **所有定义被搬到文档末尾**的 `footnote_block`,无论源文件里写在哪
- ❌ 定义顺序按**首次引用顺序**重排,不是源文件顺序
- ❌ **未被引用的定义从 token 流里彻底消失** —— `[^orphan]: 内容` 一行输出都没有

最后一条是致命的:编辑器里"定义先写、引用后写"或"删了引用但想留定义"都极常见,套用即**永久删除孤儿定义**。前两条则意味着每次保存都把源文件重排一遍。

自写规则可以让定义节点原地不动,孤儿定义、定义顺序全部天然保真,且实现完全可控。代价是不支持内联脚注 `^[...]`(本次不做,见 §8)。

## 4 · 解析层(`src/markdown.ts`)

两条自写 markdown-it 规则:

**块规则 `footnote_def`** —— 注册在 `reference` 规则**之前**。这是本设计最关键的一行:不抢在 `reference` 前面,`[^loop]: 内容` 就仍会被链接引用定义吃掉,即 §1 的根因。

匹配 `[^label]: 内容`,并**必须一并消费后续缩进续行**(4 空格或 Tab)及其间空行。不消费的话,续行会掉进 markdown-it 的缩进代码块规则 —— 那比现状破坏更重。定义内容递归解析为 `block+`。

**行内规则 `footnote_ref`** —— 注册在 `link` 规则之前,匹配 `[^label]`,产出携带 `label` 的 atom token。**有无对应定义都照样成节点**(与 Obsidian 一致),顺带根治"裸写 `[^x]` 被加反斜杠"的转义问题。

## 5 · Schema(`src/schema.ts`)

```
footnote_ref         inline atom, attrs { label }    → <sup data-footnote-ref data-label>
footnote_definition  block, content 'block+', attrs { label }
```

`footnote_ref` 照既有 atom 范式写(参照 `src/schema.ts:583` 的 `note_anchor`)。

**编号不进 attrs。** 编号是派生值,写进节点属性就会污染磁盘语义,违反 file-over-app。

> 注:这里新增的是 node 不是 mark,不涉及 `schema.ts` 里 mark 声明顺序决定序列化嵌套层级的那个坑(annotation 曾因此被排版 mark 切开)。

## 6 · 编号与交互(新增 `src/plugins/footnote-plugin.ts`)

`toDOM` 拿不到文档上下文,编号只能由插件算:扫描 doc,按 `footnote_ref` 首次出现顺序给每个 label 分配序号,用 Decoration 挂 `data-num`,CSS `content: attr(data-num)` 渲染为 ¹ ² ³。同一 label 多次引用共用同一编号。

- **hover**:浮层显示 `label` + 定义内容纯文本
- **点击**:查同 label 的 `footnote_definition` 位置 → `scrollIntoView` + 短暂高亮 decoration;定义块上提供回跳引用处的入口

## 7 · 序列化

- `footnote_ref` → `[^label]`
- `footnote_definition` → `[^label]: ` + 内容,第二段起缩进 4 空格

孤儿定义在本方案下天然保留 —— 它就是个普通块节点,与有无引用无关。

## 8 · 不做

- **内联脚注 `^[直接写内容]`**(Obsidian 写法)。YAGNI,且与既有 `^^高亮^^` 规则共用 `^` 字符,引入会多一层词法优先级冲突风险。不做的情况下它继续按纯文本处理,但**保证不被转义破坏**。
- 不引入 `markdown-it-footnote` 依赖。
- 不碰"回归上游 0.19.1"这个独立议题。

## 9 · 验证

新增 `src/__tests__/footnote.spec.ts`,先写失败测试。**往返保真是核心断言:输入字节 == 输出字节**,覆盖 §1/§3 实测过的全部破坏用例:

1. 中文定义(无空格,当前被当链接引用定义吃掉)
2. 英文定义(有空格,当前被加反斜杠)
3. 无定义的裸引用(当前被加反斜杠)
4. **孤儿定义**(定义存在但无引用 —— markdown-it-footnote 会删掉它)
5. 定义写在引用之前
6. 同一 label 引用两次
7. 多段缩进续行定义(防止退化成缩进代码块)

外加解析结构断言(节点类型/label/编号分配)与交互单测。

## 10 · 宿主侧(mdeditor)

角标 `<sup>` 样式、定义块样式、hover 浮层、跳转高亮动画。

**构建纪律**:改完 moraya-core 必须 `tsup` + `pnpm sync:core` 再重启,否则 Vite deps 缓存吃不到改动。

**GUI 验证**:rich 模式下光标移入角标的行为按 moraya 既有 `cursor-syntax` live-preview 惯例走(回显 `[^loop]` 源码供编辑);若实现中发现与 atom 节点冲突,回来重新确认交互。GUI 由用户实机验证。
