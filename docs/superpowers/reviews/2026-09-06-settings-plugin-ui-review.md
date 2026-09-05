# 设置与插件界面审查及修改记录

日期：2026-09-06。分支：`fix/apple-settings-plugin-ui`。设计依据与验收约束见 [设计契约](../specs/2026-09-05-settings-plugin-ui-review.md)。

## 结论与范围

本轮源码审查、修改与浏览器验收已完成，原生发布验收未执行。按 Apple HIG 的信息层级、清晰反馈、键盘可达性和可访问性原则，修改 Host 设置、插件市场及 14 个独立插件窗口。不是复制系统设置，也不声称通过 Apple 认证或完整可访问性认证。既有插件业务、权限、路径规则、Memory 决策协议和 provider 运行协议保持不变。

工作在独立 worktree 完成，基于已提交的 CDR 编辑修复 `a81be2a9`，合入 `origin/main@7cdbb88d` 后的基线为 `9abc8c37`。原始工作区的大量未提交改动未被覆盖。本轮不升版本、不推送、不发布；已安装的应用和插件不会自动获得这些改动。

## 主要问题与处理

| 优先级 | 发现的问题 | 修改与证据 |
| --- | --- | --- |
| P1 | 模态可把键盘焦点留在背景，Escape 与保存过程可能相互干扰 | 共用最小 `modalFocus` action，处理进入、Tab/Shift+Tab、嵌套模态、IME Escape、返回触发点；关闭权限留给调用方。真实组件与 Chromium 交互验证。 |
| P1 | 减少动态效果样式造成菜单显示后无法进入键盘焦点 | 纯 DOM 实验证实 `transition-duration:0.01ms` 会使默认无过渡变成全属性微过渡，聚焦时计算后的 visibility 仍隐藏；改为 `0s`，配合 DOM 更新后聚焦，不叠加延时补丁。 |
| P1 | Idea/Trace 的显式目录保存吞掉错误，窗口关闭后看似已保存 | 先等待同一持久化函数成功再切换当前目录；失败就地显示并保留草稿。真实 App fixture 注入写入失败、重试成功通过。 |
| P1 | 设置重试只落盘，没有补齐相关运行时开关；异步控件失焦 | Host 重试保留原 Daily Notes/MCP 动作；三家 Agent 完成保存后恢复原控件焦点，但不抢占用户已经移动的焦点。 |
| P1 | OpenClaw 重连/配对分支可能卸载未发送草稿，发送失败会中断连续输入 | 在 App 保留同一份临时草稿，失败保留输入并恢复焦点；真实 main.ts 下连接、消息流、发送失败与重连验证。 |
| P1 | Memory 表单保存期间可继续修改并丢掉后续输入，失败提示在模态背后 | 添加/编辑/合并表单保存期间保护字段与关闭/取消；六类弹窗复用同一错误提示 snippet，在当前弹窗中显示失败。新增合并失败/busy 回归和添加失败真实浏览器验收。CDR 正文仍保留自己的连续编辑机制。 |
| P2 | Host 设置导航、底部动作和复选框在窄窗拥挤 | 固定导航/动作与独立内容滚动；390px 复选框与文字同排，长标签内部换行。逐页截图检查。 |
| P2 | 插件支持文字过小、浅深色对比不一致、控件缺名称 | 统一系统字体、13px 界面正文、通常不低于 12px 的辅助文字、语义文本色、主/次/危险动作；补标签及当前页面状态。没有强行统一各插件的领域布局。 |
| P2 | Next 窄窗标题被挤成竖列；Decision 评分栏挤压主看板 | Next 标题/工具栏分行；Decision 小屏评分栏下置，保留看板内部横向滚动。 |
| P2 | Weekly 过去/未来日期低对比；Meetings 首屏中英文混排 | 日期文字改为语义文字色，保留日历领域色；Meetings 在首帧前初始化语言并增加中文首帧测试。 |
| P2 | Meetings 空态迁移入口绕过设置保存保护 | 所有迁移入口与入口函数共同检查保存状态，避免失败错误随设置面板关闭而不可见。 |
| P2 | 菜单样式重复，button reset 可能覆盖 hover | Host 和插件共享唯一 `.menu-panel/.menu-row` 定义；AgentPicker 修改 canonical 后机械同步副本，补键盘、焦点和浏览器 hover 验证。 |
| P2 | 主题相关原生动作失败只在控制台可见 | 文件选择、预处理、打开目录、恢复内置主题提供可见错误；保留原有调用与错误报告。取消清理失败不把用户锁在模态。 |

## 入口覆盖清单

“浏览器”指真实 Svelte 界面加隔离 Host/RPC fixture，不代表真实账户或原生能力执行。所有窗口先做静态入口审查，再按代表状态检查浅色、深色和窄窗；不是每个业务数据组合都逐屏遍历。

| 入口 | 审查/修改范围 | 浏览器与保留边界 |
| --- | --- | --- |
| Host 设置 | 核心、区块、CLI、更新、搜索与索引、手记快捷键/目录、动态插件设置及 Share | 7 个可见导航页；VaultSettingsTab 单独验证。保留桌面/iOS 门禁，不把 Agent 私有设置移入 Host。 |
| ThemeImportDialog | 验证报告、失败与重试、保存保护、嵌套模态 | 真实组件覆盖成功/失败/busy/焦点；原生 ZIP/文件选择采用隔离测试。 |
| 插件市场与授权 | 分类/卡片、更新状态、启停名称、能力授权、预览重试、安装保护 | 市场浅深窄窗；授权失败/重试及 busy 模态为真实组件测试。保留版本和重启提示、确认后才安装。 |
| Claude Agent | 运行、任务、历史、设置、环境、日志、用量、产物 | 主窗与设置浅深窄；640×420 历史菜单、保存失败、键盘恢复。 |
| Codex Agent | 同上 | 同上；不新增模型配置或改变 harness 选择。 |
| DeepSeek Agent | 同上 | 同上；不改变 provider/任务协议。 |
| OpenClaw | 会话、消息、输入、附件、重连、设备请求、配对/引导组件 | 已连接中文聊天、消息流、360×480、失败保稿和 forced-colors。PairingDialog 原本未接入产品入口，仅组件测试，不虚构配对功能。 |
| Idea Spark | 正文/Inbox、设置、确认、上下文菜单、AgentPicker | 浅深窄窗，目录保存失败与重试，回到设置触发点。 |
| Trace Source | 正文/Inbox、设置、确认、上下文菜单、AgentPicker | 编辑器输入、设置焦点/反向 Tab/Escape、目录保存失败与重试；短中文空态不能用字数阈值误判成白屏。 |
| Next | 看板与设置、创建任务/想法、位置/元数据等 sheet、选择菜单 | 正常空看板浅深窄窗；保持 pointer 拖动与既有元数据流程，不改状态模型。 |
| Decision Log | 看板/卡片、评分、复盘、签署/裁决 sheet | 浅深窄窗；SignSheet 为真实组件回归。看板内部横向滚动是有意设计。 |
| Weekly Review | 年历、月/周网格、空态 | 真实 12 个月年历浅深窄窗；保留年历品牌字体及周报/笔记领域色。 |
| Ebook Import | 书库、主题导航/管理/分类审核、配置、AgentPicker | 空书库/未配主题浅深窄窗；保留分类/导入/AI 队列语义，不执行真实导入。 |
| Roam Import | 配置、诊断、导入操作 | CLI 未启用代表状态浅深窄窗；不执行真实迁移。 |
| Power Mode | 动效设置及演示 | 默认配置浅深窄窗；保留实际体验能力，不将所有动效在业务模型中禁用。 |
| Meetings | 列表、设置、迁移预检及确认 | 列表/设置浅深窄窗；中文首帧、保存失败/保护和既有迁移测试通过；不执行真实迁移。 |
| Memory | 主张/候选、Role/Scope、冲突历史、六类弹窗、CDR 样式、AgentPicker | 主窗与添加弹窗浅深窄窗、焦点/Escape；合并失败/busy 为真实组件测试。CDR 本轮仅界面样式，不重定义版本或决策语义。 |
| Position Log (`pos-log`) | Manifest、贡献入口 | 无独立设置/窗口；不为凑界面新增设置。 |
| Export to PDF (`md2pdf`) | Manifest、贡献入口 | 无独立设置/窗口；保留原导出命令。 |
| Custom Editor Fixture | Manifest、开发用途 | 开发测试插件，不作为生产 UI 改版对象。 |

共检查 17 个 manifest：14 个实际窗口、2 个无独立窗口插件、1 个开发 fixture。`agent-run-core` 是无 manifest 的后台共用模块，不是遗漏的用户界面。

## 共享实现边界

- `src/styles/ui-foundation.css`：只提供真实页面需要的颜色/文字/焦点等基础，显式 `.ui-surface` 选择加入，不改正文排版模型。
- `src/styles/popup-menu.css`：原 Host 全局菜单样式的唯一来源，由 Host `app.css` 和插件基础样式共同导入；保留高特异性的全局 hover。
- `src/lib/ui/modal-focus.ts`：只管理焦点和标准键盘，不决定保存、删除、授权或业务生命周期。
- AgentPicker 保留现有 canonical＋同步脚本机制，不再维护四套菜单逻辑。插件构建会内联这些共享源码，不要求已安装插件运行时去读取仓库 CSS。

遵循 YAGNI/KISS：没有新设计系统框架、通用表单引擎或 settings schema；DRY 用于已有重复样式/焦点/菜单，业务状态仍由各插件持有。

## 验证

基线 Host：263 个测试文件、2807 条测试通过；类型检查 0 errors / 43 warnings。

- 最终 Host：**267 个测试文件、2842 条测试通过**；`pnpm build` 成功且 Editor Kit v1/v2 产物检查通过。与插件合计 4157 条测试执行通过，不把重复执行计入总数。
- 14 个插件最终全量测试：**1315 条通过**（Claude 64、Codex 68、DeepSeek 68、OpenClaw 35、Idea 307、Trace 53、Next 253、Decision 83、Weekly 40、Ebook 122、Roam 47、Power 6、Meetings 11、Memory 158）。
- 14 个插件类型检查均 0 errors / 0 warnings，14 个生产前端构建成功。
- Host 类型检查 0 errors / 37 既存 warnings；本轮修改的 Settings/Vault/ThemeImport 已无 Svelte 警告。Host 构建包含 Editor Kit v1/v2 产物检查。
- 浏览器：Chromium **140.0.7339.16**。总矩阵为 1100×760 light/dark、390×640 narrow；Agent 专项补 640×420、OpenClaw 360×480 和 forced-colors。
- 最终总矩阵 **18/18 入口通过**（14 个插件＋Host 设置、Vault、市场、AgentPicker），四个 Agent 专项 **4/4 通过**。分别生成 70 张、14 张最终截图；无未捕获浏览器错误。截图数量不是独立业务场景覆盖率。
- 共用重点颜色计算抽检：浅底 accent/文本语义色对比约 5.51–6.59:1，深底文本语义色约 9.38–10.76:1；不推断所有自定义背景、领域色或组合均已达标。
- `scripts/check-ui-browser.mjs`：全部窗口代表状态、Host 导航、AgentPicker、局部编辑与设置/弹窗交互。
- `scripts/check-agent-ui-browser.mjs`：四个 Agent 的真实 `main.ts` 入口、历史/菜单/保存失败与已连接聊天。

复验（已有 Playwright 与 Chromium 安装时）：

```sh
pnpm test
pnpm check
pnpm build
PLAYWRIGHT_MODULE=/absolute/path/to/playwright/index.mjs node scripts/check-ui-browser.mjs
PLAYWRIGHT_MODULE=/absolute/path/to/playwright/index.mjs node scripts/check-agent-ui-browser.mjs
```

可通过 `PLAYWRIGHT_CHROMIUM_EXECUTABLE` 指定现有浏览器。总矩阵支持 `UI_REVIEW_FILTER` / `UI_REVIEW_OUTPUT`；Agent 专项使用 `AGENT_UI_OUTPUT`。所有 fixture 使用虚构数据，没有访问用户 Vault、真实模型、账户或执行真实安装。

最终本机证据目录：`/private/tmp/notemd-ui-review-final`（70 张总矩阵）、`/private/tmp/notemd-agent-ui-final`（14 张 Agent 专项）；Host 独立复核在 `/private/tmp/notemd-ui-host-review`，另有 `notemd-ui-review-{trace,idea,next,decision,calendar}` 定向截图。临时目录可能被系统清理，脚本可重建证据；较早 `notemd-ui-review-images` 中的旧 `*-failure.png` 为修复前诊断，不能作为最终通过截图。最终 `*-save-failure.png` / `*-settings-failure.png` 是有意注入保存失败、验证恢复能力的通过证据。

## 未验证与发布门禁

1. 没有做 macOS WKWebView、iOS 真机、VoiceOver 或全量 WCAG 审计。现有原生窗口最小尺寸、焦点/输入法、辅助技术与自定义主题需要发布前真机抽验。
2. GitHub/Keychain、CLI 安装、宿主更新、插件安装、公网资源、真实导入与模型调用均未实机执行；组件中的成功/失败反馈由隔离替身验证，不能等同于端到端服务验收。
3. CLI 初次状态与 manifest 后台加载仍保留既有控制台诊断；本轮未将后台诊断全面改造成新的通知体系。
4. CDR 编辑修复在本分支基线中保留，发布时仍需 Memory 与 Host Editor Kit 配套。源码构建成功不等于原生签名、插件打包、商店发布或用户当前安装已更新。
