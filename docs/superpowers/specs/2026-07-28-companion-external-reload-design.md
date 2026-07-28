# 伴生笔记外部变更即时重载 设计

日期:2026-07-28
状态:已确认
前置:`2026-07-27-annotation-qa-loop-design.md`、`2026-07-28-answer-node-inline-card-design.md`(均已随 v6.728.x 上线)

## 一句话

`.note.md` 被别的 agent 或别的设备改动、经 git 同步落到本地后:干净就静默重载、脏才提示;重载后正文的答复卡片同步刷新;滚动位置留在原处。

## 背景:现状是三个半机制,拼不成一条链

| 环节 | 现状 |
|---|---|
| `file-watcher` | 只监听 `tab.filePath`。panel 模式下的伴生 `.note.md` **无人监听** |
| `.note.md` 作为 tab 打开 | `external-state.ts` 的 `decide()` 里 `mode === 'rich'` 一刀切禁止 autoReload → 即使干净也只出横幅 |
| 横幅「重新加载」 | `reloadFromDisk` **不派发** `mdeditor:auto-reloaded`,而 OutlineEditor 只听这个事件 → 点了也不刷新大纲树(既有 bug) |
| git-sync | 拉取后不通知前端「哪些文件变了」,全靠 OS 文件事件冒泡 |
| 滚动位置 | 全仓无任何保存/恢复机制 |

**并且有一个现存 bug**:`answers-store` 的 `loadAnswersFor` 里有
`if (outline.docPath === notePath) → 用内存树`。本意是避免读到过期盘内容,
实际效果相反 —— **大纲面板开着时,外部写入的答复被内存树遮蔽**,既不进大纲树也不进
答复索引。伴生监听在最常见的场景下是失效的。

## 目标行为(已确认)

1. **干净就静默重载,脏才提示**。与主文档既有的 auto-reload 策略一致,最少打断。
2. 重载后,**对应打开的源 `.md` 的渲染同步刷新**(具体就是答复卡片:它们由 `.note.md` 派生)。
3. **滚动位置按像素恢复**。

## 1. 统一的伴生笔记变更管线

取代 `answers-store` 里那个「只喂卡片」的 watcher。一次读盘,两个消费者。

```
伴生 .note.md 的 FS 事件
      → 300ms 防抖
      → 读盘 + sha256
      → hash === 上次已知? ── 是 ──> 忽略(自写回声)
                             └ 否 ─> 大纲挂载着该 note 且 dirty?
                                        ├ 是 → outline.externalConflict = {diskText}
                                        │        (走现有冲突横幅:重新加载 / 用我的覆盖)
                                        └ 否 → 静默重载:
                                                 attachDoc(notePath, diskText, mainContent)
                                                 setAnswersFromText(notePath, diskText)
                                                 → 大纲树与答复卡片同时刷新
```

要点:
- **单一读盘点**。大纲树与答复索引吃的是同一份文本,不会出现两者不一致。
- **自写回声抑制**靠 hash 比对,与主文档 `file-watcher` 的做法一致(`decide()` 里的
  `hash === tab.lastKnownHash → ignore`)。我们自己每次落盘后都会更新 `noteDiskHash`
  基线,所以自写不会触发重载。
- **脏的判定**用 `outline.dirty`(既有字段)。脏时绝不静默覆盖 —— 这是数据安全底线。
- 大纲**未挂载**该 note 时(用户没开大纲面板),仍要刷新答复索引,因为正文卡片依赖它。

### 1.1 修掉内存树遮蔽

`loadAnswersFor` 的 `outline.docPath === notePath → serializeDoc(false)` 分支保留,
但它的语义收窄为**「切换文档时的初次加载」**:此时内存树就是最新的(刚 attach 过)。
**磁盘变更事件不再走 `loadAnswersFor`**,而是走上面的管线(先重建树、再由同一份文本
派生索引)。两者职责分离,遮蔽问题随之消失。

## 2. `.note.md` 作为 tab 打开时也能静默重载

两处修正:

1. **`decide()` 放行大纲笔记 tab**。`external-state.ts:63` 的 `tab.mode === 'rich'`
   之所以禁止 autoReload,是因为 ProseMirror 持有自己的文档状态,静默替换会被下一次
   按键或 destroy-flush 反向覆盖磁盘。但 `.note.md` tab 由 **OutlineEditor** 渲染,
   用的是可随时重建的 outline store,没有这层顾虑。故:**是大纲笔记 tab 且内容干净 →
   允许 autoReload**。
2. **`reloadFromDisk` 补派发 `mdeditor:auto-reloaded`**。它现在只改 tab 字段,
   而 OutlineEditor 只在该事件上重建树 —— 点了「重新加载」大纲不刷新是既有 bug,
   顺手修掉。RichEditor 有 `tab.currentContent` 入站 effect 兜底,不受影响。

## 3. 滚动保位

新小模块 `src/lib/scroll-keep.ts`,单一职责、可单测:

```ts
/** 抓取滚动位置,返回一个「重载后调用即复位」的闭包。元素为空则返回 no-op。 */
export function captureScroll(el: HTMLElement | null | undefined): () => void
```

实现:记住 `scrollTop`(以及 `scrollLeft`,source 模式横向也有意义),复位时在
`requestAnimationFrame` 内写回 —— 必须等 DOM 重建完,否则写进去会被随后的渲染清掉。
复位闭包**幂等且可安全重复调用**。

挂载点(三处重载路径):
- `EditorPane` 的 `mdeditor:auto-reloaded` 处理器:rich 用编辑器滚动容器、source 用 textarea。
  (该处理器已有 source 模式的**光标**行列重映射,滚动保位与之并存。)
- `OutlineEditor` 的重载路径(§1 静默重载 + `reloadRemote`):大纲滚动容器。
- 横幅的 `reloadFromDisk`。

不做块锚定式恢复:外部改动通常发生在你视线之外的位置,像素复位已足够;
块锚定要引入身份匹配逻辑,性价比不划算(YAGNI)。

## 4. 组件边界

| 文件 | 职责 |
|---|---|
| `src/lib/outline/companion-reload.ts`(新) | 纯决策函数:给定 `{diskHash, lastHash, dirty}` → `'ignore' \| 'reload' \| 'conflict'`。可单测 |
| `src/lib/note-anno/answers-store.svelte.ts` | 收窄:只管索引;把 watcher 换成调用统一管线 |
| `src/components/outline/OutlineEditor.svelte` | 装/卸伴生监听;静默重载与冲突分支 |
| `src/lib/external-state.ts` | `decide()` 放行大纲笔记 tab |
| `src/lib/tabs.svelte.ts` | `reloadFromDisk` 补派发事件 |
| `src/lib/scroll-keep.ts`(新) | 滚动抓取/复位 |
| `src/components/EditorPane.svelte` | 重载前后夹滚动保位 |

纯逻辑(重载决策、滚动保位)全部单测覆盖;watcher 装卸与视觉效果由 GUI 验证。

## 5. 验证

- **单测**:重载决策三分支(自写回声忽略 / 干净重载 / 脏冲突)、`captureScroll` 的
  capture-restore 与空元素 no-op、`decide()` 对大纲笔记 tab 的新行为(且不影响普通 rich tab)。
- **GUI(用户实机)**:
  1. 开着文档 + 大纲面板,从终端往 `.note.md` 追加一条答复 → 大纲树与正文卡片**都**刷新,滚动不跳
  2. 大纲有未保存改动时同样操作 → 出冲突横幅,不静默覆盖
  3. `.note.md` 作为 tab 打开且干净 → 外部改动静默重载,大纲树刷新
  4. 点横幅「重新加载」→ 大纲树确实刷新(修复前不刷新)
  5. 编辑器滚到中部再触发重载 → 位置留在原处

## 6. 明确不做(YAGNI)

- git-sync 的按文件前端通知(OS 文件事件已够用,且 rust 侧改动面大);
- 切 tab 的滚动保位(`{#key tab.id}` 整体销毁重建,是另一件事);
- 块锚定式滚动恢复;
- 光标位置恢复(rich 模式;source 模式的既有行列重映射保持不变)。
