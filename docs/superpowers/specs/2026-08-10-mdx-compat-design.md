# `.mdx` 有限兼容设计

日期:2026-08-10
状态:已确认,待实现

## 背景

`.mdx`(Markdown + JSX)在 Astro / Next.js / Docusaurus 生态里被大量使用。note.md 目前**完全不认识**这个后缀:`src/lib/fs.ts` 的 `EXT_TABLE` 没有 `mdx` 条目,`classifyPath()` 返回 `null`,`isSupportedPath()` 为 false —— 文件双击打不开,文件夹视图和 vault 列表里也看不见。`.jsx` / `.tsx` 都当代码文件处理,所以 `.mdx` 掉进空档属于遗漏,不是有意排除。

## 核心约束:富文本回写会毁掉别人的构建

MDX 不是 markdown 的一种写法,而是 markdown + JSX:顶层 `import` / `export` 语句、`<Component />` 调用、JSX 内部的缩进块。moraya 的 rich 模式是「解析成 ProseMirror 文档 → 再序列化回文本」,这条链路上:

- `import X from '...'` 被当普通段落,`_` 触发强调、保存时反斜杠转义(同 `^^` 高亮转义不对称那一类问题);
- JSX 内部的 4 空格缩进行被识别成代码块,回写后结构改变;
- 这些文件通常是别人站点的**构建源码**,改坏 = 构建挂掉。

对一个把 file-over-app 写进信条的产品,静默改坏源文件是最不能犯的错。因此本设计的形态不是「把 mdx 当 markdown 全功能编辑」,而是:**认得、能读、能批注,但编辑器永不对它做序列化回写。**

## 目标场景

开发者用 note.md 阅读并批注自己文档站仓库里的 `.mdx` —— 源文件绝不能动,判断沉淀进 vault。这与信念 4「你的批注属于 vault,不属于路径」同源。

## 设计

### 1. 新增独立 `FileKind: 'mdx'`

不复用 `'markdown'`。现有代码里 `kind === 'markdown'` 事实上是「可被 ProseMirror 全文序列化回写」的同义词(`file-watcher.svelte.ts:59`、`plugins/share-baker.ts:335`、`outline/gate.svelte.ts:69` 均如此假设)。新开一个 kind 后,TypeScript 联合类型会强制每个消费点被显式访问一次 —— 漏掉的地方在编译期暴露,而不是运行时把用户源码写坏。

扩展名登记点(现各自维护一份 `['md','markdown','mdown','mkd']`,均需同步):

| 位置 | 作用 |
| --- | --- |
| `src/lib/fs.ts` `EXT_TABLE` | 主分类表 |
| `src/lib/vault-list.ts:1` | vault 列表图标 / isText |
| `src/lib/dialogs.ts:55,68,71` | 打开/保存对话框过滤器 |
| `src/lib/folder-view.svelte.ts:188` `EXT_RE` | 文件夹视图去后缀显示 |
| `src/components/SettingsDialog.svelte:238` | 文件关联设置 |
| `src-tauri/src/vault_ios/list_dir.rs:7` | iOS vault 目录白名单 |

### 2. 两种模式:source 可写,rich 只读

- **rich(默认)= 阅读视图,只读。** 复用现有 `RichEditor`,在 moraya `createEditor` 返回后调用 `view.setProps({ editable: () => false })`。走 ProseMirror 自带的 EditorView props,**不需要改 moraya-core**(省掉 tsup + `pnpm sync:core` 流程)。只读 ⇒ 文档永不脏 ⇒ 保存路径不会被触发,回写风险从「靠自觉」变成「结构上不可能」。
- **source = 唯一可编辑入口。** `SourceView` 逐字节保存,与现有 code 文件一致。改字走 Cmd+/。
- `modeKeyFor('foo.mdx')` 天然返回 `'mdx'`,模式偏好按扩展名单独记忆,不污染 `.md`。

### 3. MDX 构造的显示处理

新建 `src/lib/mdx/display.ts`,把 mdx 原文一次性转成「安全 markdown」再喂给只读 RichEditor:

- 顶层 `import` / `export` 行 → 包成 ` ```jsx ` 代码块;
- 块级 JSX(行首 `<大写标签` 或标签含点号)→ 整块包成代码块,按标签配对扫到闭合;
- 围栏代码块内的内容一律跳过(示例代码里的 `import` 不能被吞);
- 行内 JSX(`<Badge />` 夹在段落中)与 `{expr}` v1 不特殊处理,按原样显示。

**关键安全性质:因为 rich 模式永不写盘,这套启发式判断错了只损失显示效果,不可能损失一个字节。** 这使得我们可以接受一个简单而不完美的扫描器,不必引入完整 MDX AST 解析器。

### 4. 手记:`foo.mdx` → `foo.mdx.note.md`

- `companionPathFor()`(`src/lib/outline/store.svelte.ts:72`)增加 mdx 分支,**保留完整原后缀**。理由:同目录下 `index.md` 与 `index.mdx` 共存在 Astro / Next 仓库里不算罕见,沿用「去后缀 + `.note.md`」会让两份文档共用同一份手记且无法分开。`OUTLINE_SUFFIX_RE`(`/\.notes?\.md$/i`)已能把 `foo.mdx.note.md` 识别为手记本身,不会递归派生。
- `outline/gate.svelte.ts:69` 的 `tab.kind === 'markdown'` 放开到 `markdown | mdx`。
- 源 `.mdx` 与 vault 镜像副本都不被写入。行内批注(`src/lib/note-anno/`)对 mdx 整体禁用,右键菜单不出现该项。
- 镜像宿主链路:`src-tauri/src/sotvault/logic.rs:193` 的 `is_markdown()` 目前只认 `"md"`,需放开到 mdx,否则 vault 外的 mdx 做手记时找不到宿主。

### 5. 明确不做

- 不做 MDX 编译 / 组件预览;
- 不做 JSX 结构化编辑;
- 不做 mdx 上的行内批注;
- mdx 不进关系图(只有 `.note.md` 结网,与「关系只在人确认处生长」一致);
- 不碰 OKF(mdx 不是 OKF 知识文档格式)。

### 6. 已知取舍

只读 rich 模式下文档内查找(find)依赖 PM 视图,不受只读影响,可用;但编辑类操作(斜杠菜单、格式化、行内批注)全部不可用,这是刻意的。

## 测试

- `src/lib/fs.test.ts`:`classifyPath('a.mdx')` → `{ kind: 'mdx' }`;大小写不敏感。
- `src/lib/outline/store.test.ts`:`companionPathFor('/d/foo.mdx')` → `/d/foo.mdx.note.md`;`foo.md` 与 `foo.mdx` 不撞车;`foo.mdx.note.md` → `null`。
- `src/lib/mdx/display.test.ts`:import/export 行、块级 JSX、围栏代码块内免疫、纯 markdown 原样通过。
- `src/lib/folder-view.test.ts`:「markdown」视图模式把 mdx 计入。
- Rust:`sotvault` 的 `is_markdown` 与 `vault_ios` 的 `list_dir` 白名单单测。
