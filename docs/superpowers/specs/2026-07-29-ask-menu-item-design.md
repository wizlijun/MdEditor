# 右键菜单「提问」项 + Cmd+? 快捷键

日期:2026-07-29
状态:已确认,待实现

## 背景

批注文本里含 `?/？` 即被识别为「提问」(`isQuestionText`),进而触发问答闭环:
`scheduleQuestionCapture` 把提问写进伴生 `.note.md`,外部 agent sweep 后回填答复。

今天要发起一次提问,用户得先「插入批注」再自己敲问号。缺一个把「我要问 agent」
这个意图一步表达出来的入口。

## 目标

1. 编辑器右键菜单里、「笔记」**之上**多一个「提问」项,点击即创建一条**预填问号**的批注。
2. 快捷键 Cmd+?(Mac 物理键位 Cmd+Shift+/),rich 与 source **两种模式都响应**。
3. 图标是双问号 `??`,呼应批注编辑气泡上已有的「⁇ 提问」按钮。

非目标:斜杠菜单入口、大纲侧入口、批注气泡改版 —— 均不在本次范围。

## 设计

### 1. 菜单项(数据层)

`src/lib/context-menu/menu-model.ts` 的 `emphasis` 组顺序改为:

```
question / note / highlight / wikilink
```

`question` 与 `note` 同规格:`emphasis: true`,不设 `needsSelection`(有无选区都可用)。
i18n 新增 `ctxmenu.question`:zh「提问」/ en「Ask」/ ja「質問」/ de「Frage」。

### 2. 图标

`src/lib/context-menu/icons.ts` 新增 `question`:两个 Feather 风格描边问号并排,
即 `??` 的线条化写法(24×24 / currentColor / stroke-width 2,与其余菜单图标同规格)。
不上橙色 —— 橙色 ✦ 是「AI 写的」专属语义,提问是人写的。

### 3. 插入行为:给既有函数加 seed 参数,不复制逻辑

- `insertNoteMarkup(value, start, end, seed = '')`(source)
  - 无选区:`{>>?<<}`;有选区:`{==sel==}{>>?<<}`
  - 光标落在 `?` **之前**,用户直接打字即成一句问句
  - `seed === ''` 时行为与现状逐字节一致(回归由测试锁住)
- `insertNoteRich(view, seed = '')`(rich)
  - annotation mark / `note_anchor` 以 `note: seed` 创建,随后照旧打开编辑气泡

### 4. 编辑气泡的光标位

`NoteEditPopup` 现在是 `focus(); select()`(全选),预填 `?` 会被首个按键覆盖。
`NoteEditState` 增加可选 `caret?: number`:

- 有值 → `setSelectionRange(caret, caret)`,提问传 `0`
- 无值 → 维持全选,编辑已有批注的行为不变

### 5. 快捷键

判定用 `mod && event.key === '?'`(按 `key` 而非 `code`,兼容非 US 布局),
`preventDefault + stopPropagation`:

- rich:`RichEditor.svelte` 的 `handleRichKeydown`,紧邻 Cmd+Shift+N 分支
- source:`SourceView.svelte` 的 `onTextareaKeydown` 的 Cmd 分支内

### 6. 落盘守卫:纯问号不算问题

副作用:插入 `?` 后静置 1.5s,`scheduleQuestionCapture` 会把一条内容只有 `?` 的
question 写进 `.note.md`(防抖只在继续输入时才被取消)。

守卫加在**落盘侧**:批注文本去掉问号与空白后为空的,不算真问题,不触发捕获。
`isQuestionText` 的大纲升格语义不动 —— 只改 question-capture 这条写盘链路,
避免影响已有 `.note.md` 的解析与渲染。

## 测试

- `text-format.test.ts`:seed 的选区/无选区/光标位;`seed=''` 回归
- `menu-model.test.ts`:emphasis 组顺序与图标名
- `question-capture.test.ts`:纯问号(`{>>?<<}` / `{>>？ <<}`)不落盘,
  含实质内容的问句照常落盘

## 改动清单

| 文件 | 改动 |
| --- | --- |
| `lib/context-menu/menu-model.ts` | 新增 question 项 |
| `lib/context-menu/icons.ts` | 新增 `??` 图标 |
| `lib/i18n/{zh,en,ja,de}.ts` | `ctxmenu.question` |
| `lib/context-menu/text-format.ts` | `insertNoteMarkup` seed 参数 |
| `lib/context-menu/{rich,source}-actions.ts` | `case 'question'` |
| `lib/note-anno/note-commands.ts` | `insertNoteRich` seed 参数 |
| `lib/note-anno/note-ui.svelte.ts` | `NoteEditState.caret?` |
| `lib/note-anno/NoteEditPopup.svelte` | 按 caret 定位光标 |
| `components/RichEditor.svelte` | Cmd+? |
| `components/SourceView.svelte` | Cmd+? |
| `lib/outline/question-capture.ts` | 纯问号守卫 |
