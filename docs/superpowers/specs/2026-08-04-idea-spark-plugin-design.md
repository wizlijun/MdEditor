# 奇思妙想(Idea Spark)插件设计

- 日期:2026-08-04
- 状态:已与用户逐节确认通过
- 插件 id:`notemd.idea-spark`,英文名 Idea Spark,i18n 中文显示「奇思妙想」

## 一句话

托盘/插件菜单一键开窗,快速写下一个 idea,一键委托 claude-agent 按「先找落差→证伪→最小验证」的研究顾问流程把它论证成可下一步的文档;idea 原文和论证文档都落在 vault 的 idea 目录(默认 `inbox/ideas`,可在窗口设置中改)。

## 已确认的关键决策

| 决策点 | 结论 |
|---|---|
| 架构 | 方案 A:纯前端插件 + 宿主桥扩展(`host.plugin.execute` + `host.agent.watch`) |
| idea 目录默认值 | `inbox/ideas`(vault 相对路径,窗口设置可改,存 vault 级 `.notemd/idea-spark.json`) |
| 等待交互 | 可关窗,后台由宿主守望,完成后系统通知;窗口开着则就地欢庆 |
| 缺依赖降级 | 未装/未启用 claude-agent 时仍可记 idea,委托按钮引导去市场安装 |
| 编辑器形态 | 复用主程序那套 rich/source 双模式富文本(内嵌 `@moraya/core`,见 §2.1);预填轻模板(一句话念头 + 领域/迁移场景/现有条件/期望成果四个可删小节) |
| 任务模板归属 | 由奇思妙想插件内嵌并幂等种入 vault `.notemd/agent-tasks/idea-proof/`,claude-agent 自动发现,无需给 claude-agent 发版 |

## 1. 总览与组件

四个改动面:

| 组件 | 位置 | 内容 |
|---|---|---|
| 插件本体 | `plugins-src/idea-spark/`(新) | 纯前端插件,照 `plugins-src/decision-log/` 形态:manifest.v2.json + 独立窗口 UI(Svelte)+ 复制的 `bridge.ts`/`strings.ts`/`okf/concept.ts` |
| 宿主桥扩展 | `src-tauri/src/plugin_runtime/`(host_api.rs 等) | 新增 `host.plugin.execute`、`host.agent.watch` 两个桥方法及对应 capability |
| 宿主守望模块 | `src/lib/agent-watch/`(新) | 主程序前端 run 守望器:轮询 claude-agent `run-status`,终态发系统通知;守望列表持久化,重启恢复 |
| 任务模板 | 插件内嵌资源 | `idea-proof` 任务模板(task.json + CLAUDE.md + `.claude/settings*.json`),首次委托时种入 vault,已存在文件绝不覆盖 |

### manifest 要点

- `manifest_version: 2`,`id: "notemd.idea-spark"`,`kind: "native"`,纯 `ui: "ui/"`(无 binary,发 universal 包)
- `engines.notemd`:钉到包含桥扩展的主程序新版本(旧宿主装不上,市场按宿主兼容过滤)
- `activation.events: ["onCommand:open"]`
- `contributes.menus`:`[{location: "plugins", label: "Idea Spark", command: "open"}]`
- `contributes.windows`:`[{id: "main", entry: "index.html", 约 720×640, singleton: true, open_command: "open"}]`
- `contributes.tray`:`[{window: "main"}]`(托盘条目点击直接开窗,机制现成,同 decision-log/weekly-review)
- `capabilities`:`["vault.read", "vault.write", "toast", "editor.open", "plugin.execute:notemd.claude-agent", "agent.watch"]`
- `i18n`:en/zh/ja/de 四语,zh name「奇思妙想」

## 2. 插件窗口 UX 与数据流

窗口三区:**新 idea 编辑区**(主体,rich/source 双模式编辑器,见 §2.1)、**历史列表**(idea 目录下各 idea 及状态:草稿/论证中/已完成/失败)、**设置**(齿轮弹层:idea 目录路径)。

### 2.1 编辑器:内嵌 rich/source 双模式(与主程序同源)

插件不能 import 主程序 `src/`,但 rich 内核 `@moraya/core` 是独立包,插件作为 pnpm workspace 成员直接依赖并打进 ui bundle:

- **依赖**:插件 `package.json` 声明 `"@moraya/core": "file:../../moraya-core"`,并显式 pin 全部 peerDependencies(prosemirror-*/markdown-it/highlight.js)到与仓库根相同版本,防止行为漂移。**不 import `@moraya/core/style`**(该导出指向不存在的文件,构建会炸)。
- **rich 模式**:照 `src/lib/editor-bridge.ts` 姿势(仅 3 个 import、~40 行)`createEditor`,选项:`inlineSyntaxScope: 'line'`(live-preview 当前行显源码)、`enableInlineMarkInputRules: false`、`enableMath: false`、`enableMermaid: false`(控体积,katex CSS 不进包)、`enableHistory: true`、`changeDebounceMs: 200`。挂载骨架从 `RichEditor.svelte` 抄必要部分(~150 行),批注/wikilink/slash menu/查找等 mdeditor 扩展一概不要。
- **MediaResolver**(createEditor 必填):自实现 bridge 版——vault 相对路径经新增桥方法 `host.vault.read_bytes`(见 §3.3)读 base64 转 blob URL;远程 URL 直通;vault 外本地路径显示占位。
- **source 模式**:照主程序方案自建「透明 textarea + `<pre>` 高亮层 + 行号」骨架(~180 行),复制零依赖的 `src/lib/source-highlight.ts`(136 行)与 `autopair.ts`。
- **模式切换与状态共享**:照主程序姿势——单一 markdown 字符串真源 + `lastSync` 去环哨兵(~30 行),切换按钮 + 记住上次模式。
- **CSS**:复制 `src/styles/editor-base.css` 基础子集(基础/cursor-syntax 标记/代码块/hljs token/列表,约 400 行)+ 自写 ~100 行排版变量。主题只跟明暗(桥的 `theme` 字符串 + 自声明 `color-scheme: light dark`,独立窗口惯例),**不跟随主程序自定义编辑器主题**——已知取舍。
- **已知风险与对策**:core 升级改 DOM 类名时插件 CSS 会静默漂移——backlog:把编辑器基础 CSS 上游进 moraya-core 真正产出 `dist/style.css`,本期不做。插件 ui 包体积约 1.2MB(关 math/mermaid 后),可接受。

主流程:

1. 托盘或插件菜单「奇思妙想」→ 开窗,编辑器预填轻模板:

   ```markdown
   # 一句话念头

   (在这里写你的 idea…)

   ## 领域/方向

   ## 可能迁移的场景

   ## 我现有的条件

   ## 期望成果
   ```

2. **保存 idea**(按钮 + Cmd/Ctrl+S):写 `<idea目录>/YYYY-MM-DD-<slug>.md`。slug 取首个标题行,重名追加序号;带 OKF frontmatter(见 §5)。保存后关窗不丢。
3. **委托 Agent** 按钮:
   1. 未保存则先自动保存;
   2. 探活 claude-agent:`host.plugin.execute` 调其 `tasks.list`;失败 → 提示引导去插件市场安装,流程终止,idea 保留;
   3. 幂等种入 `idea-proof` 模板到 `.notemd/agent-tasks/idea-proof/`(`host.vault.write`,逐文件 exists 检查,已存在绝不覆盖);
   4. `host.plugin.execute` → claude-agent `run-note`,参数 `{note_path: <idea 相对路径>, task: "idea-proof"}`,立即返回 `run_id`;
   5. `host.agent.watch` 登记 `{plugin_id, task, run_id, notify: {title, body, open_path: <预判的 proof 路径>}}`,宿主接管守望;
   6. 窗口内该 idea 状态变「论证中 ⏳」,提示"可以关窗,完成后会通知你"。
4. **完成**:宿主发系统通知「奇思妙想论证完成:<标题>」。窗口若开着,经 `window.__notemd_dispatch` 收到完成事件 → 彩带欢庆动画 + 「打开结果文档」按钮(`host.editor.open` 在主编辑器打开 proof 文档)。历史列表该条变「已完成 ✦」。
5. **失败/lost**:系统通知报失败;窗口内该条状态「失败」,展示错误摘要,可重试(重新 run-note)。

### 状态推导(历史列表)

- 已完成:同名 `.proof.md` 存在
- 论证中:守望器有该 idea 的活跃 run(窗口打开时问宿主/或经 run-status 查询)
- 草稿:仅 idea 文件存在
- 失败:最近 run 终态非 done 且无 proof 文件

## 3. 宿主改动(唯一动主程序的部分)

### 3.1 `host.plugin.execute`(桥方法,仅 UI 桥通道)

- 参数:`{plugin_id, command, context}`;转发到现有 `plugin_v2_execute` 逻辑。
- capability 带参:`plugin.execute:<target_id>`,按目标插件精确授权(风格同 `fs.read:dialog`)。未声明 → -32001。防止任意插件互调。

### 3.2 `host.agent.watch`(桥方法,仅 UI 桥通道,capability `agent.watch`)

- 参数:`{plugin_id, task, run_id, notify: {title, body, open_path}}`。
- 宿主前端 `src/lib/agent-watch/` 维护守望列表:
  - 轮询模式复用 `src/lib/agent-workspace/store.svelte.ts` 的姿势,调 claude-agent `run-status`;
  - 终态(done/lost)→ 系统通知;done 且 open_path 存在时通知点击尽量打开该文档(平台回调受限则只提醒);
  - 发起插件窗口开着时经 `WebviewWindow::eval("window.__notemd_dispatch(...)")` 推送完成事件;
  - 守望列表持久化到 app 数据目录,主程序重启后恢复轮询(claude-agent 的 run 记录在磁盘上,重启不丢判定依据)。
- 系统通知:新增 `tauri-plugin-notification` 依赖 + capability 权限(当前宿主无通知插件)。
- 实现纪律:轮询用 interval,不搭 `$effect` 链;`$effect` 内调读写 `$state` 的 store 函数必须 `untrack`(v4.2.4 教训)。

### 3.3 `host.vault.read_bytes`(桥方法,挂在现有 `vault.read` capability 下)

- 参数 `{path}`(vault 相对路径,同 `host.vault.read` 的路径校验:拒绝绝对路径/`..`/符号链接逃逸),返回 base64,10MB 上限。
- 动机:插件窗口零 Tauri IPC,rich 编辑器的 MediaResolver 需要读 vault 内图片字节;现有 `host.vault.read` 只回文本、`host.fs.read_bytes` 仅限 dialog 选中路径。通用基建,其他插件同样受益。

## 4. `idea-proof` 任务模板与产物

vault 内布局(claude-agent 约定):

```
.notemd/agent-tasks/idea-proof/
├── task.json          # name: "Idea Proof", timeout_seconds: 1800, prompt: 研究顾问流程
├── CLAUDE.md          # 任务协议
└── .claude/
    ├── settings.json         # ${VAULT} 占位权限模板
    └── settings.scoped.json  # 收窄:只允许写 idea 所在目录(${NOTE}/${SOURCE} 占位)
```

- **prompt**(用户提供的研究顾问 prompt,整理为固定六步):先找落差(结果/迁移/假设三类,每个写成可检验陈述)→ 证伪判断(能否证伪/观测/小规模验证/失败有信息/是否撞题)→ 3 个候选验证点 → 反方审稿 → 逐级验证门 G0–G4 → 结构化结论(直接判断/最大未知/最先验证动作/候选排序/验证门/最低成立标准/结论边界)。要求:区分事实、已有结论、推断;不编造文献,找不到证据写「尚未找到证据」;不回避简单强基线;输出语言跟随 idea 原文语言。
- **CLAUDE.md 协议**:读 `${NOTE}`(idea 原文);四个输入位缺失标「未提供」;产物**只写一个文件**——与 idea 同目录的 `<idea文件名去 .md>.proof.md`,带 OKF frontmatter;**绝不改动 idea 原文**(agent 永不写源 md)。
- 产物路径插件侧可预判,委托时把 `open_path` 一并传给守望器。

## 5. 文件格式与 OKF

- `src/lib/okf/concept.ts` 的 `CONCEPT_TYPE` 新增登记:`idea: 'Idea'`、`ideaProof: 'Idea Proof'`;登记后把该文件复制进插件。
- idea 原文 frontmatter:`type: Idea` + `created`;人写内容,不加 `generated`。
- 论证文档 frontmatter:`type: Idea Proof` + `generated: <producer>/<version>` + `sources`(指向 idea 原文)。`✦`/`●` 语义自然成立。
- 两类文件均过 `pnpm okf:lint`;文件名避开保留名 `index.md`/`log.md`。

## 6. 错误处理

| 场景 | 行为 |
|---|---|
| claude-agent 未装/未启用 | 委托按钮提示安装,记 idea 不受影响 |
| idea 目录不存在 | `host.vault.write` 自动建父目录,无需处理 |
| 未打开 vault | 窗口提示需先打开 vault |
| run lost | 通知「结果状态未知」,窗口内可重试 |
| 宿主版本过旧 | `engines.notemd` 门槛,市场按宿主兼容过滤,装不上 |

## 7. 测试与验证

- 插件侧:`strings.test.ts`(四语 key 齐全)、slug/文件名生成、模板种入幂等、frontmatter 过 `scripts/okf-lint-core.mjs` 单测;编辑器 markdown round-trip 冒烟(rich 挂载→setContent→getMarkdown 不变形,jsdom 下跑)。
- 注意:在 `.claude/worktrees` 下开发时,`file:../../moraya-core` 同样受「先 `ln -s moraya-core` 再 `pnpm install`」约束。
- 宿主侧:桥方法 capability 门禁单测(未声明 → -32001)、守望器状态机单测(done/lost/重启恢复)。
- GUI:dev 构建实机手动验证(托盘入口、写 idea、委托、系统通知、欢庆、打开结果);不跑桌面自动化。
- 发布:`scripts/dev-install-plugin.sh`、`scripts/release-plugins.sh` 各加 `idea-spark` case;先主程序发版(桥扩展 + 通知插件 + CONCEPT_TYPE 登记),再插件上架市场(`gen-plugin-index.mjs` 默认 merge,注意本地 dist-plugins 旧版回扫坑)。

## 8. 明确不做(YAGNI)

- 编辑器不带 mdeditor 扩展:批注、wikilink、slash menu、查找替换、图片工具条、数学公式、mermaid 一概不做——只要基础 markdown live-preview + source 模式。
- 不跟随主程序自定义编辑器主题,只跟系统明暗。
- 编辑器基础 CSS 上游进 moraya-core(产出真实 `dist/style.css`)记 backlog,本期不做。
- 不做 idea 之间的关系图/结网(纯 `.md` 不结网,符合产品原则)。
- 不做 per-plugin「装了默认关」机制(装 = 启用,是市场现状;"用户开启后"即"安装后")。
- 不做 CLI 子命令(需要了再加)。
- 不做多任务模板选择,只有 `idea-proof` 一个。
