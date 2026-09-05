# 设置与插件界面：Apple HIG 审查与改进契约

日期：2026-09-05。范围：Host 设置及插件管理界面、14 个具有独立窗口的官方插件，另检查无窗口插件的设置贡献。保留既有业务、权限、品牌及本地化，不复制 macOS 系统设置或虚构原生控件。

## 依据

- [Apple HIG · Settings](https://developer.apple.com/design/human-interface-guidelines/settings)：按任务组织相关设置，减少不必要的配置。
- [Apple HIG · Buttons](https://developer.apple.com/design/human-interface-guidelines/buttons)：以明确标签和视觉层级区分主操作、次操作及破坏性动作。
- [Apple HIG · Accessibility](https://developer.apple.com/design/human-interface-guidelines/accessibility)：浅深色可读性、非颜色单一反馈与可访问控件。
- [Apple HIG · Keyboards](https://developer.apple.com/design/human-interface-guidelines/keyboards)、[Focus and selection](https://developer.apple.com/design/human-interface-guidelines/focus-and-selection)：标准键盘行为和可见焦点，焦点与当前选择不是同一状态。

具体像素值和布局为 note.md 的 WebView 实施选择，不声称 Apple 强制规定这些值。

## 设计规则

1. 系统字体；界面正文 13px 起，辅助文字通常 12px 起，避免用 10px 或极低透明度承载重要说明。标题表达当前位置，不在各级重复大标题。
2. Host 设置使用可扩展的持久导航与独立内容滚动区域；窄窗不得裁掉导航或底部操作。保留现有分组与设置保存语义，不擅自新增设置项。
3. 插件保留自身任务布局，统一窗口背景、分组表面、分隔线、次级文字、焦点环与按钮层级。设置表单减少层层卡片，标签和说明靠近控件；长路径允许换行或横向滚动，不能撑破窗口。
4. 仅实际主动作使用 accent；普通导航和工具栏使用中性 hover，当前导航用克制的选中背景。危险操作使用明确文字和独立确认，不把整片区域染红。
5. 字段有可访问名称；只含图标的按钮有 label。导航能表达当前页面；确实采用 tab pattern 时必须同时实现 tabpanel 关系与键盘切换，不能只添加不完整 ARIA 角色。
6. 弹层进入后焦点可达、Escape 能关闭且恢复触发点；不能让设置背景在模态打开时继续被键盘操作。点击遮罩和 Escape 必须尊重未完成的保存/已有确认规则。
7. 保存、加载和错误就地显示；已保存不能先于实际持久化结果。错误采用可读文本和恢复动作，不能只有红点或控制台日志。
8. 采用语义颜色，检查 light/dark、prefers-reduced-motion 和高对比/forced-colors。普通正文目标 WCAG AA 对比，不把所有浅色 disabled 样式误当正文。
9. 共享样式只提取多个真实页面需要的 token、焦点和弹层基础；不建立新的通用组件框架，不全局覆盖编辑器正文或插件领域颜色。
10. 菜单继续复用 Host 的 `.menu-panel/.menu-row` 样式；共享到插件时维护唯一来源，不复制菜单 hover。持久侧栏不使用弹层菜单样式。

## 分工与边界

- Host：SettingsDialog、VaultSettingsTab、插件管理与授权界面；不改 Rust API 或设置语义。
- Agent：Claude、Codex、DeepSeek、OpenClaw 的主界面与设置；不改 provider/运行协议。
- 其他插件：Idea Spark、Trace Source、Next、Decision Log、Weekly Review、Ebook Import、Roam Import、Power Mode、Meetings、Memory；不改知识授权、路径或数据模型。
- 共用基础和验收由主代理负责；单个页面明显合规则记录保留理由，不为凑改动重写它。

## 验收矩阵

- 每个 manifest 建立入口清单：有窗口/宿主贡献/无设置/测试 fixture。
- 静态检查全部入口；所有有实际修改的页面至少通过真实组件渲染、类型检查与构建，代表布局做浏览器截图和交互。
- 浏览器采用真实组件与隔离 Host fixture，覆盖浅/深色、常规/窄窗、Tab/Shift+Tab/Escape、可见焦点、保存失败和主操作可达性。
- 自动化不等于原生 WKWebView、VoiceOver 或真实账号调用。逐项记录缺口，不把全局样式变化当成所有页面已逐屏验收。
- 在独立 `fix/apple-settings-plugin-ui` 分支提交，不覆盖原始脏工作区，不推送或发布。
