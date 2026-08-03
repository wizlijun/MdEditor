# Roam CLI 当日同步 — 设计

> 2026-08-03。给 `notemd.roam-import` 插件增加「用 Roam 官方 CLI 同步某一天日记」的能力,
> 并把同一条逻辑暴露为 notemd 插件子命令。

## 0. 背景与目标

现有 `roam-import` 只能吃 Roam 的**整图 JSON 导出**(手动下载 `.zip`/`.json`,一次性全量)。
日常场景是另一种:今天在 Roam 里记了东西,想把**某一天**的日记拉进 note.md 的 dailynote,
且不覆盖我在 note.md 这边写的内容。

Roam 官方现在有 CLI/MCP 工具链 <https://github.com/Roam-Research/roam-tools>
(`npm i -g @roam-research/roam-cli`,`roam connect` 授权,token 存 `~/.roam-tools.json`,
读写走本机 Roam 桌面版的 local API)。本设计基于它。

**目标**

1. 插件窗口里勾一个「使用 Roam CLI」开关,显示 CLI 安装/连接状态,选日期,一键同步当日。
2. 同步结果与 note.md 已有的当日 dailynote **按块合并**,本地手写内容永不丢。
3. 同一条逻辑暴露为 `notemd roam-day`,可脚本化/定时跑。

**非目标(YAGNI)**

- 日期范围 / 批量补历史(先把单日跑通)
- 从 note.md 反向写回 Roam
- 块级 hash 台账(见 §4 的合并策略,不需要)

## 1. 关键事实(均已实测/查证)

| 事实 | 依据 |
|---|---|
| Roam 日记页 uid 就是 `MM-DD-YYYY` | `packages/core/src/relative-date.ts:26`;插件侧 `parse.ts:5` 同一约定 |
| `roam get-page --uid <MM-DD-YYYY>` 返回 markdown | `packages/core/src/operations/pages.ts:21` 的 `GetPageSchema` 支持 `uid` |
| `roam datalog-query` 递归 pull 能返回**与导出 JSON 同形**的页面树 | 本机实测,见 §3 的查询串 |
| `today`/`yesterday` 只在**写**侧参数上,读侧要自己拼日期 | `resolveDailyNotePage()` 只被 `create_block` 用 |
| 插件 CLI 子命令**必须**有 `binary` | `plugin_runtime/commands.rs:52` `is_process_plugin()`;UI-only 插件没有进程,`plugin_v2_execute` 无从投递 |
| host 能力表里**没有** exec/子进程能力 | `docs/plugin-v2-development.md` §5 / `host_api.rs` `method_capability()` |
| `CliRunner` 无条件要求 `payload.file` | `src/lib/cli/CliRunner.svelte:111` |

前两条合起来决定了取数方式;后三条决定了架构。

## 2. 形态:纯前端 → 后端 + 前端

`roam-import` 增加 `backend/`(独立 Cargo crate,产出 `bin/notemd-roam-import`),
骨架照抄 `plugins-src/ebook-import/`(它同样是 binary + ui + cli + 外部工具探测)。

**现有 JSON 导出导入路径(TS)一行不动。** 新能力是并列的第二条路:

```
notemd roam-day --date 2026-08-02          插件窗口「同步当日」
        │                                        │
        └────────► plugin_v2_execute ◄───────────┘
                        │
              backend: command "sync-day"
                        ├─ discover roam 可执行
                        ├─ exec roam datalog-query  → 页面树 JSON
                        ├─ convert  → outline 节点(id = roam uid)
                        ├─ host.vault.read 现有 .note.md → 解析
                        ├─ merge(§4)
                        └─ host.vault.write
```

**代价与缓解**:`.note.md` 的解析/序列化要在 Rust 里再写一份(TS 侧约 500 行的等价物),
与 TS 版有格式漂移风险 → 用 §7 的 golden fixture 双向钉死。

## 3. 取数

```
roam datalog-query --query '[:find (pull ?e [:node/title :block/uid :block/string
  :block/order :block/heading [:create/time :as "create-time"]
  [:edit/time :as "edit-time"] {:block/children ...}])
  :where [?e :block/uid "MM-DD-YYYY"]]' [--graph <name>]
```

- `{:block/children ...}` 是递归 pull,层数不限(比现有导入器写死 7 层的 pattern 更稳)。
- `[:create/time :as "create-time"]` 必须带别名:不加别名时 `:create/time` 与 `:edit/time`
  都会塌成同一个 `time` 键,先到先得。
- 返回形状 = `RoamPage`/`RoamBlock`(`types.ts`),**唯独多一个 `order`**:
  datalog 不保证子节点顺序,解析后必须按 `order` 升序排。
- 空结果 = 当天没有日记页 → 报 `not_found`,**不写盘**。

选 `datalog-query` 而不是 `get-page` 的原因:前者给结构化块树(uid/heading/时间戳齐全,
可直接复用现有转换语义),后者只给 markdown,还得再解析一遍缩进。

**已知取舍**:`datalog-query` **不做** `#.rm-hide` / `#.rm-private` 过滤(`get-page` 等
AI 读命令会跳过这些子树)。即当前设计会把标了隐藏的块也同步进 vault。这是本机自用、
数据不出机器的场景下的有意选择;若将来要尊重隐藏标记,在转换阶段自己过滤这两个 tag。

## 4. 合并

**策略(已定)**:按块 uid 合并,Roam 为准的单向镜像,本地手写永不丢。

节点 `id` 就是 Roam uid,所以能精确对位。设 `roamUids` = 本次 Roam 树的全部 uid;
本地树里 `id ∉ roamUids` 的节点即「本地块」。

逐层递归(根 = 页面):

1. **顺序**:该层输出 = Roam 子节点按 `order` 排列;本地块按**原兄弟锚点**插回 ——
   按本地原顺序逐个处理,三种情况:
   - 向前能找到最近的、**已落在输出里**的同级前驱(可能是存活的 Roam 块,也可能是刚插入的
     另一个本地块)→ 插到它后面;
   - 没有前驱、但向后能找到已落在输出里的同级后继 → 插到该层**头部**(它原本就排在所有
     Roam 块之前);
   - 两边都没有锚点 → 追加到该层**末尾**(例如一个 Roam 块下原本只有你写的子块,这次
     Roam 新增了子块:你的那块留在新块之后)。

   ⚠️ 早先本节写的是「找不到前驱就插到头部」,那是错的 —— 它与本设计自己的用例
   「Roam 块下的本地子块存活」冲突(该用例要求落到末尾)。实现期由测试证伪,已改为上面的三分支。
2. **同 uid**:`content` / `createdAt` / `updatedAt` 一律取 Roam 版,子树继续递归。
   `collapsed` 取本地版 —— 折叠是本地视图状态,不该被 Roam 覆盖。
   *你在 note.md 里直接改 Roam 块正文的修改会被覆盖 —— 这是选定的语义:想留下的判断写在新块里。*
3. **本地有、Roam 没有的 uid**:整棵子树原样保留(可能是 Roam 删了,也可能是移到别的页了;
   不替 Roam 删你的东西)。保留的本地子树里若有 `id ∈ roamUids` 的后代,丢弃该后代及其子树
   —— 它已经在 Roam 结构那边输出过了,它自己的本地子节点会在递归到它时被捡回,不重不漏。
   *块在 Roam 里被移到别的父节点下也由此覆盖:本地侧按 id 全局查找,子节点跟着走。*
4. **frontmatter**:保留本地的,只 touch `updated`;`title` 恒为 `yyyy-MM-dd`
   (与 `src/lib/outline/daily.ts` 的原生约定一致,不用 Roam 的 "August 2nd, 2026")。
5. **本地文件不存在**:等价于空树,结果就是纯 Roam 内容。
6. **幂等**:同一输入连跑两遍,输出逐字节相同。

**`id::` 强制落盘**:当日同步路径下**每一个** Roam 块都写 `id:: <uid>`。
现有 JSON 导入只给被 `((ref))` 引用的块写 id,但没有 id 就没法下次按 uid 对位,
合并会退化成整文件覆盖。代价是文件多若干 `id::` 行,可接受。

**目标路径**:`<dailyDir>/<yyyy>/<yyyy-MM-dd>.note.md`,`dailyDir` 来自 `host.vault.info`。

## 5. 界面

现有窗口顶部新增一块,不勾选时界面与今天完全一致:

```
[✓] 使用 Roam CLI 同步                         roam-tools ↗
    ✅ roam 0.9.2 · graph bruce
    ⚠️ 已安装但未连接 → 运行 roam connect
    ❌ 未安装 → npm i -g @roam-research/roam-cli
    [ 2026-08-02 ▾ ]   [ 同步当日 ]
    ✓ 新增 3 块 · 更新 5 块 · 保留本地 2 块
```

- checkbox 状态与可选的 `graph` / `roam` 路径覆盖存插件设置。
- 状态三态由 backend 的 `probe` 命令给:未找到可执行 / 找到但 `list-graphs` 报
  `CONFIG_NOT_FOUND` / 就绪(带版本 + graph 名)。
- 外链用 `<a target="_blank" rel="noopener">`(`ebook-import` 已有先例)。
- 日期选择器默认**昨天**。
- 四语言(en/zh/ja/de)字符串 + `strings.test.ts` 键齐全断言(插件 i18n 通病见
  `docs/` 的插件 i18n 审计结论)。

## 6. CLI

```
notemd roam-day [--date yyyy-MM-dd|today|yesterday] [--graph <name>] [--json]
```

- manifest:`contributes.cli` 增一条 `{ subcommand: "roam-day", command: "sync-day", … }`,
  `activation.events` 增 `"onCli:roam-day"`。
- `--date` 缺省为 `yesterday`;`today`/`yesterday` 在**插件侧**按本机本地日历解析
  (与 §1 的「读侧没有相对词」一致 —— 相对词是我们自己的便利,不是 Roam 的)。
- `--json` 输出 `{ ok, data: { date, path, created, updated, kept_local, roam_gone_kept } }`。
- 退出码:0 成功;2 参数错;3 插件未安装;4 执行失败(roam 未安装/未连接/当天无页)。

**需要改主程序一处**:`src/lib/cli/CliRunner.svelte:111` 目前无条件
`if (!payload.file) → exit 2`,任何**没有文件参数**的插件子命令都跑不通。
改为只在该 cli entry 声明了 `required` 的 `path` 参数时才要求 `file`,并补单测。
这是本设计对宿主的**唯一**改动。

## 7. 测试

**Rust 单测**

- 行内语法转换(移植 `syntax.ts` 的规则,逐条对齐 TS 单测用例)
- outline 解析 ↔ 序列化往返
- 合并:新增 / 更新 / 本地块保留 / Roam 侧删除保留 / 顺序锚点 / 幂等 / 本地文件不存在
- 日期解析(`yyyy-MM-dd` ↔ `MM-DD-YYYY`、`today`/`yesterday`、非法输入)
- 可执行发现(注入式,同 `claude-agent/backend/src/discover.rs` 的 `discover_with`)
- datalog 返回值解析:`order` 乱序、`children` 缺省、空结果

**Golden fixture(格式防漂移)**

`plugins-src/roam-import/backend/tests/fixtures/daily.note.md`:

- Rust 侧:给定固定的 Roam 页面 JSON + 固定的本地文件,合并产出必须与该 fixture 逐字节相等。
- TS 侧:同一份 fixture 用主程序 `parseOutline` + `serializeOutline` 往返,断言不变。

两侧共用一份文件 ⇒ 任一侧改了 `.note.md` 格式都会红。

**前端**:`strings.test.ts` 四语言键齐全。

**GUI**:按仓库惯例由用户实机验证,不做 UI 自动化。

## 8. 验收

1. `notemd roam-day --date <某天>` 在 vault 里生成/更新对应 `.note.md`,内容与 Roam 一致。
2. 手工在该文件里插一个本地块 → 再同步一次 → 本地块仍在原位,Roam 块被刷新。
3. 同一天连续同步两次,文件无变化(幂等)。
4. `roam` 未安装 / 未 connect / 当天无日记页 → 三种明确报错,vault 无写入。
5. 插件窗口三态状态显示与实际一致;勾选状态跨窗口重开保持。
