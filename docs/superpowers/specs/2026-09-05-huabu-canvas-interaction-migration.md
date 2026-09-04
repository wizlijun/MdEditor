# Huabu Canvas 编辑交互迁移报告

- 日期：2026-09-05
- 状态：范围内实现完成，自动化门禁通过
- 目标项目：mdeditor / note.md
- 技术路线：Svelte 5 + `@xyflow/svelte` + 标准 JSON Canvas

## 1. 结论

迁移可行，并且不需要 React 19。最终选择将画布编辑交互作为 note.md 的内置基础能力，而不是独立插件：纯几何算法保持框架无关，视图交互接入现有 CanvasView，持久化继续只写标准 JSON Canvas。

本次“完成”只指约定的 Canvas 编辑与交互范围，不表示复制 Huabu 整个产品。账户、协作、AI/Agent、服务端、搜索、图层面板、跨画布引用、Sketch/笔迹、音频、网页/PDF 处理、Huabu 私有业务节点与 structured Frame 均未迁移，也不会因本次变更进入 note.md。

## 2. 为什么采用基础能力而不是插件

| 方案 | 可行性 | 主要代价 | 结论 |
|---|---:|---|---|
| 内置基础能力 | 高 | 需要直接接入 CanvasView、Flow 手势、历史和文档事务 | 已采用 |
| note.md 插件 | 中 | 插件需拥有 Canvas surface、选择态、pointer capture、viewport、撤销事务等高权限接口；iOS 又没有相同插件运行时 | 不适合作为当前交付形态 |
| React 微前端/桥接层 | 低 | 引入 React/ReactDOM、第二套响应式状态和事件边界，触控、焦点、主题、bundle 与历史一致性都更复杂 | 不采用 |

适合未来插件化的是框架无关的命令或算法，例如自动布局、对齐策略、节点生成器；画布核心手势和文档事务仍应由宿主管理。

## 3. 不使用 React 19 的实际代价

代价为中高，但属于一次性视图重写，而不是能力损失。Huabu 的 React 组件、hook 和 store 不能直接复用，需要把交互语义拆成两部分：

1. 纯 TypeScript 几何与领域函数：吸附、对齐、分布、套索命中、多选缩放、分组闭包、自动端点与避障路由。
2. Svelte 5 视图编排：工具状态、pointer 生命周期、预览与提交分离、Flow 投影、快捷键、焦点和弹层。

相较同时运行 React 19，当前方案避免了双框架 bundle、React/Svelte 状态同步、两套事件系统和跨框架卸载问题。以基线 `769667b0` 计，本次迁移及测试共修改 11 个源码/测试文件，约新增 3,711 行、删除 118 行；其中交互算法集中在 `src/lib/canvas/interactions.ts`，没有把 React 语义扩散到文档模型。

## 4. 架构

```text
Pointer / Keyboard
        │
        ▼
CanvasView (Svelte 5)
  ├─ 工具、手势所有权、Flow 预览
  ├─ 一次性文档事务 / 历史提交
  └─ View id → canonical id 校验
        │
        ├──────────────► interactions.ts
        │                 纯几何、空间索引、吸附、路由、套索、缩放
        │
        ▼
CanvasDocument / model.ts
  ├─ 节点与边不变量
  └─ 标准 JSON Canvas 写入
        │
        ▼
现有 codec / tab / save pipeline
```

关键约束：

- `CanvasDocument` 是唯一持久化真相，Flow 只保存临时投影。
- viewport、selection、活动工具、参考线和手势预览不进入 `.canvas`。
- 缺省边端点和避障结果只用于渲染，不反向补写 side。
- group 使用标准 JSON Canvas 的几何包含语义，不引入 `parentId` 或私有成员字段。
- 拖动和缩放只在结束时生成一次文档历史提交。

## 5. 已迁移能力

- 选择、平移、套索、拖拽框组工具，以及 Space 临时平移和视图级交互锁。
- 边缘、中心与等间距智能吸附，参考线、六向对齐、水平/垂直分布和重叠散开。
- 单节点与多选整体缩放；Shift 等比缩放；键盘方向键操作缩放句柄。
- group 几何闭包复制、拖动与“适配内容”；新建/粘贴优先落到最后指针位置。
- 触控套索与框组；第二触点取消自定义绘制并交还 Flow 进行双指导航。
- 连线自环拒绝、智能相向端点、局部避障、主体补连、空白处创建标准节点并连接。
- 边标签原位编辑、低缩放 LOD、视口缩放快捷键和全局菜单样式复用。
- 大画布热路径优化：空间索引、等距二分查询、拖动原点 Map、套索 RAF 节流和文档快照缓存。
- Flow 诊断节点隔离；所有 UI 新建边必须引用文档内唯一有效节点。

## 6. 执行阶段

1. 建立 Huabu 与 note.md 的交互差距矩阵，冻结只迁移 Canvas 编辑交互的边界。
2. 抽取框架无关几何函数并以单元测试固定行为。
3. 在 CanvasView 接入工具、套索、吸附、多选缩放、放置和历史事务。
4. 补齐框组、自动平移、连线、标签、LOD、容器适配和避障路由。
5. 评审收口触控、合法端点、性能、响应式默认值、键盘可访问性和菜单一致性。
6. 运行定向测试、全量测试、Svelte 检查、生产构建和差异审计。

## 7. 验证结果

- 收口定向测试：62/62 通过。
- 全量 Vitest：245/245 文件、2,620/2,620 测试通过。
- `svelte-check`：0 error；43 个 warning 均为仓库既有告警。
- Vite production build 与 Editor Kit build check：通过。
- `git diff --check`：通过。
- 本机趋势穿刺：10k/20k 节点等距吸附约 5.5/6.3ms，约 10k 节点避障路由约 35ms，10k 节点索引套索约 2.8ms。

自动化已经覆盖 pointer 触控路径和第二触点接管。发版前仍建议在 iPad/iPhone 与 Android WebView 各做一次真实双指缩放、系统 pointer-cancel 和边缘自动平移手感穿刺；这是设备兼容性验收，不是已知的功能缺口。

## 8. 完成判定

约定范围内的迁移已经完成：没有引入 React 19，没有迁移 Huabu 业务能力，没有改变标准 JSON Canvas schema。若后续要求 structured Frame、笔迹、业务节点或插件开放 Canvas surface，应作为新的架构项目评估，不能视为本次迁移遗漏。
