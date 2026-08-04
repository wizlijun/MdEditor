# 奇思妙想(Idea Spark)主界面重做 + 委托链路接入(设计)

- 日期:2026-08-04
- 状态:已与用户逐节确认通过
- 前置:第一轮实现已合入 main(merge `737af4d`),见 `2026-08-04-idea-spark-plugin-design.md`。本文**取代**该文档的 §2(窗口 UX)与 §3.2(守望器),其余部分继续有效。

## 一句话

把窗口改成「打开就写字」:去掉标题栏、空白文档、自动保存,四个动作收进底部一条 38px 动作条;inbox 默认隐藏、可右键操作;委托链路接上并用 claude-agent 代发托盘提醒。

## 已确认的关键决策

| 决策点 | 结论 |
|---|---|
| 布局 | 方案 A:编辑区从窗口顶端铺到底部,底部 38px 动作条。**无标题栏** |
| 模式切换 | 绝对定位在编辑区右上角的悬浮胶囊,照抄主程序 `src/components/ModeToggle.svelte` 形态 |
| 新文档 | **空白**,不预填模板。灰字占位符五句轮换 |
| 保存 | 停笔 1.5s 自动写盘,**删掉保存按钮**;`Cmd/Ctrl+S` 保留为「立即写盘」 |
| 文件名 | 默认 `YYYY-MM-DD-HHmm-idea.md`(创建时刻),右键重命名可覆盖 |
| inbox | 原「历史」改名;默认隐藏、动作条按钮切换、状态持久化;展开为右侧 240px 挤压式面板 |
| 右键菜单 | 委托给 Agent / 在主编辑器打开 / 重命名 / 删除(确认后连带 `.proof.md`) |
| 委托 | 本轮真接上,用 `host.agent.run` + `notify` 规格 |
| 完成提醒 | claude-agent 代发托盘提醒;插件窗口开着时额外欢庆动画 |
| **不做宿主守望器** | 第一轮设计里的 `src/lib/agent-watch/` 与 `host.agent.watch` **取消** —— 见 §4 |

## 1. 布局

`main` 是 `100vh` 纵向 flex:

```
┌───────────────────────────────────────┐
│                              ┌──────┐ │  ← 悬浮 ModeToggle(绝对定位)
│  编辑区(flex:1, min-height:0) │👁│</>│ │
│                              └──────┘ │
│                                       │
├───────────────────────────────────────┤
│ 已保存 19:42    新想法 委托Agent 📥 ⚙ │  ← 38px 动作条
└───────────────────────────────────────┘
```

- **标题栏删除**(`<header class="topbar">` 及 `t('title')` 的使用点)。窗口标题由 Tauri 窗口本身承担。
- **ModeToggle**:新建 `plugins-src/idea-spark/src/components/ModeToggle.svelte`,视觉照抄主程序 `src/components/ModeToggle.svelte`(眼睛 SVG / `</>` SVG、32×26 按钮、圆角药丸容器、`.active` 白底投影、`role="tablist"`)。**复制而非 import**(隔离 webview 不能引主程序 `src/`)。`position:absolute; top:0; right:12px; z-index:10`,挂在编辑区容器内。kit 加载失败(降级 textarea)时不渲染。
- **动作条**从左到右:保存状态文字、弹性空隙、`新想法`、`委托 Agent`、`📥 inbox`(切换态,`aria-pressed`)、`⚙`。
- 编辑区容器必须保持确定高度链(`flex:1; min-height:0`),否则 kit 的 source 模式会塌成 0 高。

## 2. 空白文档与轮换占位符

新文档内容为**空字符串**,不再预填四小节模板。`idea-proof` 任务模板的 CLAUDE.md 早已写明「缺哪个小节就标注未提供,不要臆造」,因此无需改任务侧。

### 2.1 五句轮换

灰字提示,**一律无句末句号**(占位符惯例,视觉更轻)。五句各拆一个借口,均为出处扎实的名人警句:

| # | 中文 | 出处 |
|---|---|---|
| 1 | 写小说有三条规矩,可惜没人知道是哪三条 —— 毛姆 | 没有正确写法 |
| 2 | 想法像兔子,养两只很快就一打 —— 斯坦贝克 | 先写一个就够 |
| 3 | 写作很简单,盯着白纸直到额头渗出血珠 —— 吉恩·福勒 | 盯白纸难受是正常的 |
| 4 | 灵感是业余选手的事 —— 查克·克洛斯 | 别等灵感 |
| 5 | 这封信写长了,因为我没时间把它写短 —— 帕斯卡 | 写长写乱都不算错 |

四语按**母语表达重写而非直译**,同样不带句末句号。英文用原句(毛姆/斯坦贝克/福勒/克洛斯/帕斯卡皆有英文原文或通行英译);日德按当地习惯改写。

### 2.2 轮换机制

`.notemd/idea-spark.json` 存 `placeholderSeq: number`,取 `lines[seq % 5]`。每次开始一份新的空白草稿(开窗时、点「新想法」时)`+1` 并写**状态文件**——注意这与「空文档不落盘」(§3)不冲突:递增的是状态文件,idea 文件仍然不会被创建。**不用随机**:五句都会轮到、行为可预测、测试不必注入随机种子。纯函数 `pickPlaceholder(lines: string[], seq: number): string` 单测。

### 2.3 rich 模式的占位符(需要动 Editor Kit)

**现状缺口**:`KitOptions.placeholder` 目前只对 source 模式生效——`createEditor` 没有 placeholder 选项,而主程序是靠一个额外的 ProseMirror 插件实现的。

修法:把主程序 `src/lib/placeholder-plugin.ts`(40 行,只依赖 `prosemirror-state`/`prosemirror-view`,**零 Tauri IPC**)加进 kit 的允许 import 清单,在 `rich.ts` 里把它 reconfigure 进 editor plugins(姿势同主程序 `RichEditor.svelte` 追加 analytics 插件)。它给空文档的唯一空段落挂 `data-placeholder`,再由 CSS `::before` 渲染——该 CSS 规则在主程序里位于 `RichEditor.svelte` 的局部样式而非 `editor-base.css`,所以 **kit 要在 `kit.css` 里自带一份**(几行)。

kit 的 v1 API 不变(`placeholder` 参数已存在),只是从此在两种模式下都真正生效。

## 3. 自动保存

- **节奏**:内容变化后停笔 **1.5s** 写盘(在 kit 自身 200ms onChange 防抖之上再加一层)。
- **强制 flush 点**:切换 idea、新建、切模式、委托前、关窗(`beforeunload`)、`Cmd/Ctrl+S`。
- **保存按钮删除**。动作条左侧显示状态:`已保存 HH:mm` / `保存中…` / `保存失败`(失败态用告警色并可点击重试)。
- **空文档不落盘**:内容为空(或仅空白)时不创建文件,避免 inbox 里堆一堆空条目。
- **命名**:首次落盘用 `YYYY-MM-DD-HHmm-idea.md`(**创建时刻**,不随标题变);写盘前仍用 `host.vault.exists` 兜底,撞名追加 `-2`/`-3`(同分钟内连开两个 idea 会撞)。右键重命名后记住新名,之后不再自动改名。
- 既有 `naming.ts` 的 `slugFromMarkdown` 从「文件命名来源」降级为「**inbox 行显示标题**的来源」,函数保留、语义变更需更新注释与测试。

## 4. 委托链路(不需要宿主守望器)

**关键发现**:`host.agent.run` 接受一个 `notify` 规格,由 **claude-agent 自己**在 run 终态时发托盘提醒。claude-agent 是常驻进程(manifest 无 `idle_shutdown_seconds`),不受插件窗口开关影响。因此第一轮设计里的「宿主前端守望器 + `host.agent.watch` 桥方法」**整块取消**——这是 ebook-import 已验证的姿势。

### 4.1 流程

1. flush 未保存内容;若文档为空则提示后终止。
2. 幂等种入 `idea-proof` 模板(`seedTaskTemplate`,已实现)。
3. `host.agent.run`,参数:
   ```json
   { "task": "idea-proof",
     "prompt": "<定位段落:本次只论证 <rel>,产物写到 <proof_rel>>",
     "note_path": "<vault 绝对路径>/<idea rel>",
     "notify": { "title_ok": "…已论证", "title_fail": "…论证失败",
                 "open_path": "<proof 绝对路径>", "expect_file": "<同上>" } }
   ```
   `notify` 四个字段**全必需**(claude-agent 侧解析失败会直接报错)。`note_path` 传**绝对路径**且文件必须已存在(claude-agent 用 `canonicalize`)。
4. 返回 `{run_id}` → 写入 `pendingRuns[rel] = run_id` 并**立即落盘**。
5. 窗口开着时每 2s 轮询 `host.agent.status { task: 'idea-proof', run_id }`,行内显示进度(`steps`/`last`)。窗口关闭即停止轮询——完成提醒由 claude-agent 发,不依赖窗口。
6. 开窗时对 `pendingRuns` 逐个 status 校正一次:`done` → 移出 pending 并落盘;`lost` → 标失败。

### 4.2 错误处理

| 场景 | 表现 |
|---|---|
| claude-agent 未装/未启用 | 错误消息前缀 `agent_unavailable:`(码是 -32000,不是专用码),据此弹引导安装 |
| claude CLI 未安装 | claude-agent 业务错,消息含 `claude executable not found`,原样透传 |
| 未声明 capability | -32001,不应发生(manifest 会加 `agent`) |
| run lost | 该条标失败,可重试 |

### 4.3 manifest

`capabilities` 增加 `"agent"`。**不加** `notify`——提醒由 claude-agent 代发,插件自己不推(托盘提醒注册表**没有去重**,两边都推会出现两条)。

## 5. inbox

- 默认隐藏,`inboxOpen` 存进 `.notemd/idea-spark.json`,下次开窗沿用。
- 展开为**右侧 240px 挤压式**面板(编辑区变窄),不是覆盖。
- 每行:**标题**(`slugFromMarkdown` 从正文取 H1 → 首行非空文本 → 退回文件名)+ 相对时间 + 状态标记(`✦` 已论证 / `⏳` 论证中 / `⚠` 失败)。当前打开的条目高亮。
- `listFailed` 时面板顶部显示告警条(已实现,保留)。
- **右键菜单**(新建 `components/ContextMenu.svelte`,键盘可达:`Esc` 关闭、方向键移动、`Enter` 触发):
  1. 委托给 Agent(该条已在跑时禁用)
  2. 在主编辑器打开(`host.editor.open`;若 `.proof.md` 存在则改为二级项分别打开 idea / 论证)
  3. 重命名(行内输入,校验:非空、不含 `/`、不以 `.` 开头、不与既有文件撞名、自动补 `.md`)
  4. 删除(危险色,分隔线之上)
- **删除**:确认对话框逐条列出将删的文件(idea + 存在的 `.proof.md`),确认后真删(不做废纸篓)。删除当前打开的 idea 时清空编辑器并回到空白草稿。

## 6. 需要新增的宿主桥方法

右键的删除与重命名目前**没有桥能力**(`host.vault.*` 只有 read/read_bytes/write/exists/list/mkdir)。新增两个,均挂现有 `vault.write` capability,复用既有 `resolve_in_vault` 路径校验(拒绝绝对路径 / `..` / 符号链接逃逸):

- `host.vault.remove { path } → { ok }` —— 拒绝目录(只删文件),目标不存在时返回 `{ok:true}`(幂等)。
- `host.vault.rename { from, to } → { ok }` —— 两端都过校验;目标已存在则报错不覆盖;跨目录移动允许(同 vault 内)。

两者与 `read_bytes` 同属通用基建,其他插件同样受益。文档登记进 `docs/plugin-v2-development.md` §5。

## 7. 文件结构

`App.svelte` 现有 443 行,加右键菜单与轮询会继续膨胀。拆分:

| 文件 | 职责 |
|---|---|
| `components/ModeToggle.svelte`(新) | 悬浮模式切换胶囊 |
| `components/ContextMenu.svelte`(新) | 通用右键菜单(定位、键盘导航、点外部关闭) |
| `components/InboxPanel.svelte`(替换 `HistoryList.svelte`) | 列表 + 右键触发 + 告警条 |
| `components/ConfirmDialog.svelte`(新) | 删除确认 |
| `lib/autosave.ts`(新) | 防抖 + flush 的纯逻辑(注入 timer 以便单测) |
| `lib/agent-client.ts`(新) | 委托、轮询、状态解读 |
| `lib/placeholder.ts`(新) | 五句表 + `pickPlaceholder` |
| `lib/store.svelte.ts` | 增 `inboxOpen`(持久化)、`saveState`(idle/saving/saved+时刻/failed)、`placeholderSeq`(持久化)、以及 `pendingRuns` 的写入与清除(委托时写入并落盘、终态时移除并落盘) |

`Celebration.svelte` 保留(委托完成且窗口开着时触发)。

## 8. 测试

- **纯函数**:`pickPlaceholder` 轮换、时间戳文件名生成与撞名、重命名校验、删除的连带文件清单推导、`interpretStatus`(running/done/lost/形状异常)、autosave 的防抖与 flush 时序(注入假 timer)。
- **strings**:五句 × 四语齐全,由既有 `Record<MessageKey,string>` 类型 + `strings.test.ts` 双保险;删掉不再使用的 `templateH1`/`sectionDomain` 等键。
- **Rust**:`host.vault.remove`/`rename` 的路径校验(越界、目录、覆盖)与 capability 门禁。
- **GUI 实机**(交用户):空白文档的灰字在 rich 模式真的显示、自动保存不丢字、inbox 右键四项、删除确认、委托全链路 + 托盘提醒点击打开 `.proof.md`。

## 9. 不做

- 不做宿主守望器(claude-agent 代发提醒已覆盖)。
- 不做废纸篓、不做多选批量操作、不做 inbox 搜索/排序/分组。
- 不做撤销删除(确认对话框即闸门)。
- 不做占位符的随机化(计数器足够)。
