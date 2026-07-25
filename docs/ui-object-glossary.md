# note.md 产品对象字典 / UI Object Glossary

面向大纲(outliner)+ markdown 阅读/编辑器的关键 UI 对象命名表。每条给出**英文常用名**、  
**中文**、**说明**、**在本项目中的落点**(组件/类名,便于对齐代码与设计讨论)。英文名尽量采用  
Roam / Workflowy / Obsidian / Logseq 等成熟产品的通行叫法。

> 约定:`code` = 代码里的实际类名/标识符;斜体 = 同义词/别名。

---

## 1. 大纲结构 Outline structure

| 英文名 | 中文 | 说明 | 落点 |
| --- | --- | --- | --- |
| **node** / *item* / *block* | 节点 / 条目 | 大纲里的一行,可含子节点。Roam 叫 block,Workflowy 叫 item,Logseq 叫 block。 | `OutlineNode.svelte`, `.node` |
| **row** | 行 | 单个节点的一行容器(bullet+内容),不含其子树。 | `.row` |
| **bullet** / *dot* | 项目符号 / 圆点 | 每个节点左侧的小圆点。可点击 = zoom-in。 | `.bullet`(CSS 圆点 `::after`) |
| **tri** / **caret** / *disclosure triangle* / *twistie* / *chevron* | 折叠三角 | bullet 左侧的展开/折叠三角(▾/▸)。默认隐藏,悬停显现。 | `.tri` |
| **collapsed ring** / **halo** | 折叠光环 | 节点折叠(有隐藏子节点)时,bullet 外圈的灰色同心圆环。Roam/Workflowy 的经典标志。 | `.bullet.closed::before` |
| **guide line** / *indent guide* / *nesting line* / *thread* | 缩进引导线 / 竖线 | 展开节点从 bullet 下引出、贯穿子节点的竖直细线。 | `.node.has-guide::before` |
| **gutter** | 沟槽 | bullet 左侧容纳悬浮 tri 与引导线的留白列。 | 行 `padding-left` 沟槽 |
| **indent / indentation** | 缩进 | 每层级的水平缩进量(本项目 1.5em/级)。 | `--depth` × `1.5em` |
| **children / subtree** | 子节点 / 子树 | 某节点下的全部后代。 | `childrenOf`, `collectDescendantIds` |
| **sibling** | 同级节点 | 同一父节点下的节点。 | `normalizeSiblingOrders` |
| **ancestor / parent chain** | 祖先链 | 从某节点上溯到根的路径。 | `ancestorsOf`, `parentId` |
| **leaf** | 叶子节点 | 无子节点的节点。 | — |
| **fold / unfold** *(v.)* | 折叠 / 展开 | 收起或展开子树。 | `node.collapsed`, `onCollapse` |

---

## 2. 缩放导航 Zoom / focus navigation

| 英文名 | 中文 | 说明 | 落点 |
| --- | --- | --- | --- |
| **zoom-in** / **focus** / **hoist** | 聚焦深钻 | 点 bullet 把某节点当作新根,只显示其子树。Roam=zoom,Workflowy=focus/hoist。 | `focusRootId`, `onFocus` |
| **zoom-out** | 退出聚焦 | 回到上一级或全文。 | `onFocusChange(null)` |
| **breadcrumb** / *breadcrumbs* | 面包屑 | 聚焦时顶部显示祖先路径的导航条。 | `OutlineBreadcrumb.svelte`, `.crumbs` |
| **crumb** | 面包屑段 | 面包屑里的单个可点节点。 | `.crumb` |
| **breadcrumb separator** | 面包屑分隔符 | 段与段之间的 `›`。 | `.sep` |
| **root label / home crumb** | 根标签 | 面包屑最左段(笔记名/日期),点它完全 zoom-out。 | `.crumb.root`, `rootLabel` |
| **back / forward** | 后退 / 前进 | 导航历史前后跳转。 | `NavHistory`, `DailyToolbar` |

---

## 3. 编辑器外壳 Editor chrome

| 英文名 | 中文 | 说明 | 落点 |
| --- | --- | --- | --- |
| **toolbar** | 工具栏 | 编辑器顶部操作条(搜索/重生成/保存)。 | `.toolbar` |
| **doc title** | 文档标题 | 工具栏左侧的当前笔记名。 | `.doc-title` |
| **search bar** / *filter* | 搜索栏 | 大纲内文本过滤输入。 | `.search-row`, `.search-input` |
| **save button (dirty state)** | 保存按钮(脏标记) | 有未保存改动时高亮。 | `.hbtn.dirty`, `saveDirty` |
| **conflict banner** | 冲突横幅 | 外部改动检测提示(重载/覆盖)。 | `.conflict-banner` |
| **empty state** | 空态占位 | 无内容/无搜索结果时的提示文本。 | `.empty` |
| **body / scroll body** | 正文滚动区 | 承载大纲行的滚动容器。 | `.body` |
| **band select / marquee** | 框选 | 鼠标拖拽框选多个节点。 | `onBandDown/Move/Up` |

---

## 4. 行内内容 Inline content & menus

| 英文名 | 中文 | 说明 | 落点 |
| --- | --- | --- | --- |
| **wikilink** / *page link* / *internal link* | 双链 / 内部链接 | `[[页面名]]` 形式的链接。 | `InlineRender`, `onPageClick` |
| **backlink** | 反向链接 | 指向当前页的其他页链接。 | `backlinks.ts` |
| **linked references** | 链接引用 | 页面底部聚合所有反链的区块。 | `LinkedReferences.svelte`, `RefTreeNode.svelte` |
| **tag** | 标签 | `#标签` 形式。 | 解析于 `backlinks` |
| **highlight / mark** | 高亮 | `^^…^^` 高亮文本(金色下划线)。 | `.hl`, `source:'highlight'` |
| **annotation** *(inline note)* | 行内批注 | CriticMarkup `{>>…<<}` 批注。 | `source:'annotation'/'note'` |
| **block reference** | 块引用 | `((节点id))` 引用某节点。 | `copy-ref`, `pinnedIds` |
| **slash menu** / *command menu* | 斜杠菜单 | 输入 `/` 触发的命令菜单。 | `SlashMenu.svelte`, `menu.kind:'slash'` |
| **page-link menu / autocomplete** | 双链补全菜单 | 输入 `[[` 触发的页面选择。 | `menu.kind:'link'` |
| **context menu** | 右键菜单 | 节点右键操作菜单。 | `EditorContextMenu.svelte`, `onContextMenu` |
| **inline render** | 行内渲染 | 非编辑态把 markdown 渲染为富文本。 | `InlineRender.svelte` |
| **drop indicator (sibling/child)** | 拖放指示 | 拖拽时提示落为兄弟/子节点。 | `.drop-sibling`, `.drop-child` |

---

## 5. 每日日志 Daily Notes

| 英文名 | 中文 | 说明 | 落点 |
| --- | --- | --- | --- |
| **feed** | 信息流 | 连续滚动、按日期排列的多天流。 | `DailyFeed.svelte`, `.feed` |
| **day block** | 日块 | feed 中单独一天的区块。 | `DailyDay.svelte`, `.day` |
| **date header** | 日期标题 | 日块顶部的本地化日期(作 H1 显示)。 | `.date`, `displayDate` |
| **day divider / separator** | 日分隔线 | 天与天之间的分隔线(取主题 `<hr>`)。 | `.day-sep`, `.sep-wrap` |
| **active day vs read-only day** | 活动日 / 只读日 | 唯一可编辑的一天 vs 其余只读渲染。 | `active`, `readonly` |
| **page view** | 页视图 | 打开某 wiki 页的独立视图。 | `DailyPage.svelte`, `view.kind:'page'` |
| **focus view** | 聚焦视图 | 聚焦某节点子树的独立视图(zoom)。 | `DailyFocus.svelte`, `view.kind:'focus'` |
| **lazy window / infinite scroll** | 懒加载窗口 | 上下滚动时增量加载更多日期。 | `IntersectionObserver`, `extendOlder/Newer` |
| **jump to date** | 跳到日期 | 工具栏日期跳转。 | `jumpTo`, `onJump` |

---

## 6. 文件与 vault File / vault model

| 英文名 | 中文 | 说明 | 落点 |
| --- | --- | --- | --- |
| **vault** | 库 | 一批 `.md` 的中立公共仓库(类 git repo)。 | `sotvaultStore.vaultRoot` |
| **companion note / sidecar note** | 手记 | 与源文件配对的批注/大纲 `.note.md`。用户可见名"手记"(en: Sidecar Notes / ja: サイドノート / de: Randnotizen);内部 id/目录/后缀仍为 outline-notes/.note.md。 | `.note.md`, `companionPathFor` |
| **mirror-hosted marks** | 镜像宿主批注 | 批注属于 vault、不属于路径:源文件镜像进 vault 作批注宿主。 | `note-home`, `.notemd/mirrors` |
| **front-matter** | 前置元数据 | 文件头部的 YAML 元信息块。 | `frontmatter` |
| **anchor line** | 锚定行 | auto 节点指向源文档的 1-based 行号。 | `anchorLine`, `onJump` |
| **fold memory** | 折叠记忆 | 折叠状态存 KV(不入 `.note.md`)。 | `outliner-folds.json`, `applyFolds` |
| **tab** | 标签页 | 一个打开中的文档缓冲(脏标记/撤销)。 | `tabs.svelte.ts`, `Tab` |

---

## 7. 节点来源标记 Node source markers

| 英文名 | 中文 | 说明 | 落点 |
| --- | --- | --- | --- |
| **AI mark ✦ vs human mark ●** | AI 标记 / 人类标记 | `✦`=AI 所写,`●`=你所想(判断/意图)。 | 产品约定 |
| **source: toc / highlight / wikilink / annotation / note / manual** | 节点来源类型 | 决定 bullet 颜色与可编辑性。 | `NodeSource` |
| **auto node vs manual node** | 自动节点 / 手动节点 | 派生只读节点 vs 用户手写节点。 | `source!=='manual'` |

---

## 8. 面板与窗口 Panels & windows

| 英文名 | 中文 | 说明 | 落点 |
| --- | --- | --- | --- |
| **side panel** | 侧栏 | 注册表驱动的可切换左右侧栏。 | `OutlinePanel.svelte` |
| **panel switcher** | 侧栏切换器 | 标题栏下拉切换侧栏内容。 | 标题栏下拉 |
| **editor pane** | 编辑器面板 | 主编辑区。 | `EditorPane.svelte` |
| **standalone window** | 独立窗口 | 每日笔记等独立 Tauri webview。 | `daily-notes-app.svelte` |
| **tray (four states)** | 托盘(四态) | 系统托盘同步状态灯(含大文件黄灯)。 | 托盘 |

---

## 9. 插件与市场 Plugins & market

| 英文名 | 中文 | 说明 | 落点 |
| --- | --- | --- | --- |
| **plugin (v2)** | 插件 | 进程内隔离 webview,经 `window.notemd` 桥通信。 | `plugins-src/*` |
| **plugin market** | 插件市场 | 在线插件商店。 | `plugin-market-app.svelte` |
| **manifest** | 清单 | 插件元数据 `manifest.v2.json`。 | `manifest.v2.json` |
| **host bridge** | 宿主桥 | 插件调用主程序能力的 API。 | `window.notemd`, `host.*` |

---

## 附:命名速查(高频)

`node · row · bullet · tri(caret) · collapsed ring(halo) · guide line · gutter · indent · zoom-in(focus/hoist) · zoom-out · breadcrumb · crumb · toolbar · wikilink · backlink · linked references · slash menu · feed · day block · date header · day divider · companion/sidecar note · vault · anchor line · tab`