# 插件七类认知分类与 AI 展示设计

日期：2026-09-01

## 目标

插件一级分类按用户完成插件主流程后获得的主要结果划分；直接改变操作反馈与使用感受的工具归入体验增强。AI 是跨分类的协作方式，不作为一级分类。

## 一级分类

| 稳定键 | 中文 | 英文 | 说明 |
|---|---|---|---|
| `record` | 记录 | Capture | 留住重要的信息 |
| `reading` | 阅读 | Read | 读懂更多 |
| `inspiration` | 灵感 | Ideas | 发现新的可能 |
| `advance` | 推进 | Move Forward | 把想法变成下一步 |
| `reflect` | 回顾 | Reflect | 从经历中持续改进 |
| `create` | 创作 | Create | 让想法成为作品 |
| `experience` | 体验增强 | Experience | 改善操作反馈与使用感受 |
| `other` | 其他 | Other | 未知或缺失分类的兼容兜底 |

## 正式插件映射

| 分类 | 插件 |
|---|---|
| 记录 | `notemd.pos-log`、`notemd.roam-import` |
| 阅读 | `notemd.ebook-import`、`notemd.trace-source` |
| 灵感 | `notemd.idea-spark` |
| 推进 | `notemd.next`、`notemd.claude-agent`、`notemd.codex-agent`、`notemd.deepseek-agent`、`notemd.openclaw-chat` |
| 回顾 | `notemd.decision-log`、`notemd.weekly-review` |
| 创作 | `notemd.md2pdf` |
| 体验增强 | `notemd.power-mode` |

## AI 角色

| 角色 | 插件 |
|---|---|
| AI 阅读 | `notemd.ebook-import` |
| AI 启发 | `notemd.idea-spark` |
| AI 推理 | `notemd.trace-source` |
| AI 执行 | Claude / Codex / DeepSeek Agent、OpenClaw |

AI 角色只描述插件当前明确提供的能力；Next 等能与 Agent 协作但自身不执行 AI 的插件不标 AI。

## 兼容规则

1. 新 manifest 直接写七类稳定键。
2. 旧键按最接近的新类降级：`agents → advance`、`capture → record`、`thinking → reflect`、`import-export → create`、`editing → experience`。
3. 通用映射无法区分旧 `capture` 中的 Idea Spark / Trace Source，或旧 `thinking` 中的 Next，因此正式插件按 ID 使用权威映射覆盖旧 category。
4. 未知第三方键进入 Other，不能隐藏插件。
5. Native 菜单、已安装缓存、远端 registry 和公开市场页使用同一顺序。

## 市场展示

- 页面顶部增加紧凑的“与 AI 一起完成”区域，列出当前市场中具备 AI 角色的插件；点击名称滚动到原分类卡片。
- AI 卡片增加紫蓝渐变描边和角色徽标；有更新时继续由橙色更新状态占最高视觉优先级。
- AI 精选区是发现入口，不复制完整卡片，不改变插件的一级分类。
- 页面保持高密度布局，避免 AI 展示挤占首屏。

## 验收

- 单元测试固定分类顺序、正式插件映射、旧键兼容、AI 角色和非 AI 排除。
- 组件测试固定 AI 精选区、角色徽标、旧缓存迁移和七类 DOM。
- Rust 测试固定 Native 菜单顺序、旧 manifest 按 ID 迁移和 installed API category。
- 浏览器验收中检查宽屏与窄屏、AI 卡片、更新卡片优先级和精选入口滚动。
