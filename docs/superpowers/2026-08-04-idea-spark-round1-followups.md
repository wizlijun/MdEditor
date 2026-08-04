# 奇思妙想(Idea Spark)第一轮实现:待办与验证清单

分支 `feat/idea-spark`,基线 `d165f6e`,2026-08-04。
设计见 `docs/superpowers/specs/2026-08-04-idea-spark-plugin-design.md`,计划见 `docs/superpowers/plans/2026-08-04-idea-spark-plugin.md`。

本轮做完了:`host.vault.read_bytes`、Editor Kit(第二 vite entry + `__host__/assets/` 运行时下发)、`host.theme.css` 与主题推送、OKF 类型登记、插件脚手架、纯逻辑库、`idea-proof` 任务模板、主界面。**委托 agent 的链路整体押后**(改用并行会话「AI 先读」正在实现的 `host.agent.run`/`host.agent.status`),因此「委托 Agent」按钮当前是禁用态。本轮**不发布**。

## 一、合并前需人工 GUI 验证(自动化测试碰不到)

dev 模式下 `__host__` 读的是磁盘上的 `dist/`(不是 Vite dev server),所以**必须先 `pnpm build` 再 `pnpm tauri dev`**;改了 kit 源码没有 HMR,要重跑 build。

1. 托盘与插件菜单出现「奇思妙想」;点开窗口,标题为中文。
2. 编辑器 rich 模式 live-preview 正常;切主题后插件窗口跟随;rich/source 切换内容不丢。
3. **带本地图片的 idea 能否渲染**——这条最要紧:CSP 放行 `blob:` 的修复只有实机能证(单测只能证 CSP 串对了)。
4. source 模式高度不塌(kit 内部 `height:100%` + 绝对定位,依赖容器有确定高度)。
5. 写 idea → 保存 → vault `inbox/ideas/` 出现带 OKF frontmatter 的 `.md`(Obsidian/CLI 可直接读)。
6. 历史列表列出已存 idea;点击载入编辑器;设置里改 idea 目录生效;vault 未打开时给提示。
7. 「委托 Agent」按钮为禁用态并给出「等 agent 接口就绪」提示(本轮预期行为)。
8. 其余待确认交互:`Cmd/Ctrl+S` 与编辑器 keymap 是否抢焦;关窗时 `beforeunload` 里的 `host.toast` 能否送达;富文本脏检查有无伪阳性;设置弹层与 kit/庆祝动画的 z-index;booting 首屏无 loading 指示的观感。

## 二、终审后仍开着的三条(已裁定 park,建议随下一轮处理)

1. **`rebuildIdeaDoc` 的 salvage 是一次性的**(`plugins-src/idea-spark/src/lib/idea-doc.ts:38-48` + `store.svelte.ts` 的 `saveIdea`)。frontmatter 坏掉时会把原块救进正文,但保存后 `savedMarkdown` 与编辑器内容都不含 salvage,**同一会话第二次保存就把救出来的字节写没**;`idea-doc.ts:44-46` 声称 "so the user's bytes survive (visibly, editable)" 在当前实现下不成立。触发面窄(手改坏 frontmatter + 不重载连存两次)。修法:salvage 时把 `bodyOf(text)` 推回编辑器并 rebaseline。
2. **`task-template.ts:161-163` 的注释已作废**:它仍称桥无法给 `precheck.sh` 加可执行位,而终审修复波已让 `host.vault.write` 对 `.sh` 结尾路径 chmod 0o755。会把人引向过时的 mitigation。
3. **`.sh` 自动 chmod 偏宽**(`src-tauri/src/plugin_runtime/ui_rpc.rs` 的 `vault_write`):无条件 `set_permissions(0o755)`,覆盖写既有 `.sh` 会把用户原本 0600 的脚本变成 0755。建议收窄到 `.notemd/agent-tasks/` 路径前缀,或只在新建时设;更彻底的做法是改消费侧(`precheck.rs` 用 `sh <script>` 调用,或对「文件存在但 spawn 失败」fail-closed),那样宿主桥根本不需要 chmod 语义。

## 三、接委托链路(押后的 Task 3/8/13)时的交接点

- **API 对齐**:用并行会话「AI 先读」定义的 `host.agent.run` / `host.agent.status`(capability `agent`),**不要**做通用 `host.plugin.execute`(其 spec 明确列为非目标)。完成提醒进它的托盘提醒注册表(`OpenPath` 打开 `.proof.md`),不自造通知机制。manifest 届时补 `agent`。
- **宿主守望器仍然需要**:奇思妙想是纯前端插件、没有后端进程,窗口一关就没人轮询;而「AI 先读」的模型假设插件有后端自行轮询。宿主必须替这类插件守望。
- **store 侧已备好、零生产调用方**:`markPending()` / `applyRunDone()` / `seedTaskTemplate()` / `state.pending` 持久化 / `state.lastResult`。两个坑:`markPending()` 之后必须补一次 `persist()`(否则重启丢在跑的 run);`applyRunDone` 现在保证「返回 done ⇔ 徽标 done ⇔ 行上有打开结果」,推送里的 `open_path` 只写 `lastResult`,若要按它打开结果得读 `lastResult` 而不是 `proofPathFor`。
- **`precheck.sh` 守卫**:即便有了 chmod,也要实机确认 `idea-proof` 的 precheck 真的生效(`precheck.rs::run` 对 spawn 失败是 fail-open,失效时表现为「守卫静默不拦」而非报错)。

## 四、已知设计权衡(非缺陷,备查)

- `idea-proof` 给 agent 的权限面含 `Read(${VAULT}/**)` + `WebFetch`(协议第 2 步要查是否撞题),`deny` 了 `Bash`/`Task`。这构成一条 prompt-injection 数据外泄通道:vault 内容被读到后可拼进 URL 查询串经 WebFetch 送出。spec 有意为之,若不接受可去掉 WebSearch/WebFetch 并接受「撞题判断只能靠模型已有知识」。
- `editor.kit` 与其它 capability 一样是 **manifest 自声明**,安装期没有白名单也没有用户确认。它保证普通插件默认无此面、且 manifest 可被市场审计,但不抵抗一个刻意声明它的恶意插件——真正的边界是路径段校验、只读 GET,以及「`dist/` 中不含机密」这一前提。
- 保存走全篇 PM 序列化:载入一个非 PM 规范格式的 idea(手改的、外部工具写的)并编辑后保存,整篇会按 PM 规范重排。`.proof.md` 不受影响(历史列表明确排除,只经 `host.editor.open` 去主窗口)。
