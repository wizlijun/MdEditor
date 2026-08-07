# 文件夹视图跟随当前打开的文件

2026-08-08

## 问题

启动后文件夹视图的根默认指向 vault(`defaultRootToVault`,v6.806.1)。此后打开别处的 md 时,视图常常不跟着走:

`syncToActiveFile` 只在文件**落在当前根的子树之外**时换根(VS Code 式 reveal 语义)。于是打开 `vault/a/b/c.md` 时根仍停在 vault,而 `setRootDir` 又不会展开祖先目录 —— 用户看到的就是"打开了文件,文件夹视图里却找不到它"。

期望:每次打开一个 md,文件夹视图的根就是这个文件所在的目录;但用户在视图里手动换过目录后,不要把他的位置抢回去。

## 行为规范

1. **活动文档变化即跟随**:打开新文件或在 tab 间切换 → 根 = 该文件所在目录。不再判断"是否已在当前根子树内",一律定到文件所在那一层。
2. **树内打开不换根**:在文件夹视图树里点开的文件(含行尾笔记角标打开 `.note.md`)不改根 —— 那本身就是用户在视图里浏览,重定根会让树在脚下跳动。
3. **手动导航保留到下一次文档切换**:↑「上一目录」换过根之后,同一文件再次触发同步(面板开合导致组件重挂载、无关状态刷新)不得覆盖手动位置。判据是"活动文件路径变了没有",不是"根等不等于文件父目录"。
4. **空白启动不变**:根为 `null` 时仍由 `defaultRootToVault` 指向 vault。
5. **换根清空展开态**:沿用 `setRootDir` 既有行为,不改。

## 实现

改动集中在两个文件。

### `src/lib/folder-view.svelte.ts`

模块级跟随状态:

- `lastFollowedPath: string | null` —— 上一次同步过的活动文件路径。
- `pendingSuppressPath: string | null` —— 一次性抑制标记。

`syncToActiveFile(filePath)` 新逻辑:

1. `filePath == null` → no-op(未命名文档)。
2. `filePath === lastFollowedPath` → 早退。这是规范第 3 条的实现:手动导航后活动文件没变,不重定根。
3. 记下 `lastFollowedPath = filePath`。
4. 若 `pendingSuppressPath === filePath` → 消费该标记后返回(规范第 2 条)。
5. `parent = parentDir(filePath)`;`untrack(() => folderView.rootDir) === parent` → 返回;否则 `await setRootDir(parent)`。

新增导出:

- `suppressFollowFor(path)` —— 树内打开前登记一次抑制。
- `resetFollowState()` —— 清模块状态,供单测隔离用例(模块状态在测试文件内跨用例存活)。

`folderView.rootDir` 的读取必须走 `untrack`:该函数在 `$effect` 里同步调用,而 `setRootDir` 会写 `rootDir` / `expanded` / `entriesCache`,读+写同一状态会自失效(项目里踩过的死循环坑)。

### `src/components/FolderView.svelte`

`open(path)` 在 `openFile(path)` 之前调 `suppressFollowFor(path)`。该函数是树内所有打开路径的唯一出口(文件行 + 笔记角标都经 `FolderTreeNode` 的 `onOpen`),一处覆盖即可。

`$effect(() => { void syncToActiveFile(activePath) })` 保持在组件内:面板关着时不跟随,重新打开面板时按当前活动文件定根 —— 语义正确,且无需把接线挪进 `App.svelte`。

## 已知边界(接受)

点击树里的 vault 副本时,`openFile` 会重定向到源文件,活动路径与抑制登记的路径不同 → 根会跳到源文件所在目录。这一跳有用(显示真件位置),不额外处理。

## 测试

`src/lib/folder-view.test.ts`(vitest,纯逻辑,`readDir` 已 mock):

- 改写既有用例「文件在子树内保持根」→ 断言跟到文件父目录(`root=/a`,打开 `/a/b/c.md` → 根为 `/a/b`)。
- 保留:根在别处时跟到父目录;`null` no-op。
- 新增:同一路径二次同步不覆盖手动导航后的根。
- 新增:`suppressFollowFor(p)` 后同步 `p` 不换根;此后同步另一文件仍换根(标记只消费一次)。

各用例前 `resetFollowState()`。

## 不动的部分

`defaultRootToVault`、`watchRoot`、排序 / 视图模式 / 隐藏文件夹、↑ 按钮本身、置顶与重命名。
