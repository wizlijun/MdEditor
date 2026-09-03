# Memory v2：认识论安全条目与 macOS 交互

## 问题

Memory v1 已经把 `USER.md` / `MEMORY.md` 变成受控投影，但条目 API 只理解正文、状态、  
优先级和来源。Vault 中已有的 `confidence::`、`origin::` 等字段只是未解析的 Markdown，  
不会进入候选 SHA、批准 diff、CLI 或插件。因此 Agent 仍可能把一条 `pending` 派生材料读成  
确定事实，也无法快速分辨“应遵循的偏好”和“必须避免的误判”。

Memory 1.0.0 的界面同时依赖浏览器原生 `window.confirm`，按钮命中区、焦点态和字体层级也  
没有形成稳定的 macOS 组件体系。在 Tauri 插件窗口中，这会表现为操作没有可靠反馈，甚至  
像“点不动”。

## 条目契约

每个普通条目继续使用 Markdown bullet + `key:: value`，但核心必须理解以下语义：

```markdown
- 一条原子 claim。
  priority:: critical | high | normal | low
  polarity:: positive | negative | neutral
  epistemic-status:: owner-stated | source-supported | inferred | contested | unknown
  certainty:: high | medium | low | unknown
  agent-guidance:: Agent 应如何使用这条记录
  avoid-error:: 不得做出的推断或动作（可选）
  source:: /精确来源#锚点
  id:: UUID
  revision:: 0
  status:: pending
  proposal:: UUID
```

这些轴不可合并：

- `status` 是人工审阅状态。`pending` 永远不是确定记忆；`approved-by` 只表示 owner 同意  
  记住该条目，不等于外部世界的客观真理。
- `epistemic-status` 是证据性质。`owner-stated` 表示 owner 直接表达；  
  `source-supported` 表示有一手来源支持；`inferred` 是 Agent 推断；`contested` 表示存在  
  冲突；缺失或无法判定时为 `unknown`。
- `certainty` 是当前材料的可信程度。它不能从人工批准状态自动升级；`inferred` 不得为  
  `high`。
- `polarity` 是 Agent 行为方向，不是情绪：`positive` 表示应遵循的偏好/原则，  
  `negative` 表示禁止、纠错或隐私边界，`neutral` 只是背景上下文。
- `priority` 表示避免错误时的处理顺序。`critical` 用于身份、隐私、授权和会导致现实行动  
  的错误；`high` 用于稳定偏好和重要产品边界。
- `agent-guidance` 必须是可执行的一句话。negative、inferred、contested、low/unknown 条目  
  必须同时有 `avoid-error`。

## 兼容和失败关闭

- state、v1 candidate 和 v1 event 继续可读；不可原地改写现有 37 个候选。
- v1 条目缺少新字段时，在 API 中显示保守默认：`neutral / unknown / unknown`，并标记  
  `classification_complete=false`。绝不把旧 `confidence:: high` 静默映射成 v2 certainty。
- 新 create/replace/merge 候选必须把所有分类字段写进 frontmatter，使 SHA-256 绑定正文、  
  分类与 Agent guidance。revoke 继承目标元数据；set-priority 只改变优先级。
- 重复或非法的安全属性必须报 integrity error；不能采用“最后一个获胜”或静默回退。
- managed schema upgrade 只做格式规范化和安全默认，不生成决定事件、不制造人审、不改变  
  entry ID/revision/status/candidate 字节。真实语义分类另以 replacement proposal 提交。
- 搜索或直接读取投影时，`status:: pending` 与 `certainty:: unknown` 必须紧邻 claim；控制  
  notice 明确要求 Agent 对 pending 使用 verify-first。

## 插件信息架构

窗口采用 macOS 三层结构：固定工具栏、分段导航、滚动内容。界面不使用网页式大卡片堆叠。

- 工具栏：标题、精简说明、刷新按钮和投影健康状态。
- 分段导航：记忆、待审、改善；整个 segment 至少 36px 高，当前项使用系统 accent 填充。
- 列表行：左侧用清晰的 polarity 标识；首行显示 claim；第二行显示审阅状态、证据性质、  
  certainty 和 priority；negative/critical 始终优先排序并使用高对比警示色，但不整行染红。
- 详情/编辑：使用 label + control 表单，所有控件统一 13px SF Pro Text，正文 14px，标题  
  20px；主要按钮最小 44px 高，其余交互最小 32px 高。
- 确认：用窗口内 sheet 展示 exact SHA、before/after 和分类 diff；禁止使用  
  `window.confirm`。确认按钮只有在 sheet 可见且用户主动点击时才调用 `host.memory.decide`。
- 所有 button/input/select/textarea 明确 `pointer-events:auto`、`-webkit-app-region:no-drag`，  
  有 hover、active、focus-visible 和 disabled 状态。busy 只禁用会写入的动作，不冻结导航。
- 窄窗口转为单栏；不允许主要控件在 760px 最小宽度下溢出。

## 验证

1. Rust：解析/渲染 round-trip、非法/重复字段失败、v1 保守默认、candidate SHA 绑定元数据、  
   schema upgrade 幂等且不改候选/事件。
2. Domain：筛选、排序、badge、完整 metadata diff、pending/negative 的 Agent 使用规则。
3. Component：实际触发 click/input/change，验证导航、打开编辑、打开/取消确认 sheet、批准/  
   拒绝各只发送一次 RPC；测试中禁止 mock `window.confirm`。
4. 真实 sotvault：upgrade 前后 37 candidates/0 events 不变，entry ID/revision/status 不变，  
   projection hash 与 state 一致，无 drift；旧“Bruce 可以直接编辑本文件”协议矛盾被移除。