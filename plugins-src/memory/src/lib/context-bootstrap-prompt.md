# 任务：基于我的 note.md Vault 初始化 Role / Scope，并提议分配现有记忆

你是一个外部 Agent。请根据我当前 note.md Vault 中可验证的长期证据，建立一份保守、稳定、互相隔离的 Role / Scope 候选，并在我确认 Registry 后，为现有 Memory Claim 提交 Role / Scope 重新分配提案。

这不是人格分析，也不是让你取得长期记忆治理权。你可以自动执行读取、分析、候选校验、重分配预览和 proposal；Role / Scope 定义生效及最终批量应用必须由我在 note.md 的「记忆 → 身份与场景」界面确认。

---

## 一、不可突破的权限边界

1. 只读取完成任务所需的 Vault 内容和以下 `notemd memory` 命令输出。
2. 不直接修改 `.notemd/memory/**`、`MEMORY.md`、`USER.md` 或任何 Memory ledger、Registry、projection 文件。
3. 不调用或编造以下能力：
   - `notemd memory context-registry replace/apply/import`
   - `notemd memory reassign apply`
   - 任何 `approve`、`--yes`、`--force` 或冒充 `human:*` 的参数
4. `--recorded-by` 必须是你自己的稳定 Agent ID，例如 `codex/gpt-5`、`claude/sonnet-4`；绝不能写成 `human:*`。
5. 不读取、复制或输出密码、token、API key、私钥、门禁码、证件号、银行卡号、完整住址等凭据或高风险隐私。发现后只计数为「已排除敏感项」，不要复述内容。
6. 不把推断当事实。证据不足、互相冲突或可能跨安全域的内容必须进入「待我判断」，不得强行归类。
7. 不把当前对话临时要求写成长期 Role / Scope，也不推断「本次会话当前采用哪个 Role / Scope」。当前会话上下文由 Context Manifest 单独决定。

如果你没有 Vault 文件读取或本地命令执行能力，输出「当前 Agent 无法读取 Vault 或执行 notemd CLI」，然后停止，不要凭本次对话虚构结果。

---

## 二、Role 与 Scope 的严格定义

### Role：我的行为身份

Role 描述「我以什么身份思考、判断或协作」，例如开发者、顾问、父亲。Role 不是客户、项目、目录、工具或某一个人的名字。

- ID 使用稳定的小写英文：`role:<slug>`。
- 首次初始化通常保持在 3–7 个清晰 Role；证据不足时宁可少建。
- `label` 是可读名称；改文案不应导致未来更换 ID。
- `guidance` 只写该身份下 Agent 应如何协助我。
- `avoid_error` 写该身份最需要防止的串用或误判。
- 只有我明确表达过，或至少两份相互独立、由我产生的材料持续支持，才能建议 active Role。

### Scope：事实可以被使用的隔离场景

Scope 描述「这条记忆属于哪个安全边界或工作空间」，例如家庭、客户 A、客户 A 的项目 X、自有产品。

- Realm 是硬安全域，ID 使用 `realm:<slug>`，没有 `parent_id`。
- Space 是 Realm 内的细分空间，ID 使用 `space:<realm>/<slug>`，必须设置同安全域的 `parent_id`。
- 同一 Realm 及其 Space 必须共用完全相同的 `security_domain`。
- 不同客户、家庭与工作客户默认属于不同 Realm，不能因为都由我参与就合并。
- 项目名称、客户名称和家庭场景属于 Scope，不属于 Role。
- 不创建宽泛的 `global` 或「所有客户」Scope 来省事。确实跨场景安全的个人偏好才可保留在既有通用 Scope。
- 不把客户、家人或第三方的事实提升成「关于我的全局事实」。

### Claim 分配约束

- 每条 Claim 至少要有一个明确 Role 与一个明确 Scope；无法判断的保留在现有 unclassified / 通用项并列入人工复核。
- 一条 Claim 不得跨越两个不同 `security_domain`。若同一句内容似乎适用于多个 Realm，列为「需要拆分」，不要用一次重新分配扩大可见范围。
- `--set-role` 与 `--set-space` 表示完整替换，不是追加；生成命令前必须列出替换后的完整集合。
- 从私有/客户 Realm 移到更宽 Scope、跨 Realm、降低隔离或扩大适用 Role，一律作为高风险单项，不能混入普通批次。

---

## 三、证据标准

优先级从高到低：

1. 我明确写下或确认的身份、边界、决定和长期做法。
2. `notemd memory list --status current --json` 返回的已批准 Claim。
3. 带 `generated.by: human:*` 或等价人工署名的 Vault 文档。
4. 多份材料中反复出现、但仍需要我确认的稳定模式。

以下内容不能单独作为创建依据：

- `MEMORY.md` / `USER.md` 的投影文本；它们只可辅助定位，不能替代结构化 Claim。
- 仅由 Agent 生成、没有人工确认或其他独立证据的内容。
- 单次会议、当天待办、临时会话、草稿猜测、情绪或心理画像。
- 目录名本身。目录只能作为线索，必须结合文件内容或已批准 Claim。

每个候选 Role / Scope 都要附证据清单：Vault 相对路径、标题或 heading、证据日期、支持的语义。只做短摘要，不大段复制原文。没有证据清单的候选不得进入 Registry JSON。

---

## 四、执行流程

### 阶段 A：只读盘点

先逐条运行并解析：

```bash
notemd memory context-registry show --json
notemd memory list --status current --json
notemd memory check --json
```

随后用你的只读搜索/文件工具检查 Vault。跳过版本控制目录、依赖、构建产物、二进制文件和 `.notemd/memory/**` 权威内部文件。不要仅因关键词命中就建立 Role / Scope。

输出：

1. 现有 Registry 摘要；
2. 候选 Role 表；
3. 候选 Scope 树；
4. 每项证据与置信度；
5. 待我判断、需要拆分和已排除敏感项的数量。

### 阶段 B：生成并校验完整 Registry 候选

候选必须保留现有 Registry 中的全部条目、稳定 ID、状态和 redirect；首次初始化只允许新增，不自动归档、重命名、合并或删除既有项。

生成一份完整 JSON，结构严格为：

```json
{
  "roles": [
    {
      "id": "role:developer",
      "label": "开发者",
      "description": "仅写有证据支持的身份说明",
      "aliases": [],
      "status": "active",
      "guidance": "在该身份下如何协助我",
      "avoid_error": "必须避免的跨身份误用"
    }
  ],
  "scopes": [
    {
      "id": "realm:product/notemd",
      "label": "note.md 产品",
      "description": "仅写有证据支持的场景说明",
      "aliases": [],
      "status": "active",
      "kind": "realm",
      "security_domain": "product/notemd"
    },
    {
      "id": "space:product/notemd/release",
      "label": "发布",
      "description": "note.md Realm 内的发布工作",
      "aliases": [],
      "status": "active",
      "kind": "space",
      "security_domain": "product/notemd",
      "parent_id": "realm:product/notemd"
    }
  ]
}
```

把候选写到 Vault 外的临时文件，然后执行：

```bash
notemd memory context-registry validate --file <临时候选文件> --json
```

如果校验失败，根据 `errors` 修改后重试，最多三次。不要为了通过校验删除现有项或放宽安全域。最终展示完整 JSON、校验结果和与现有 Registry 的逐项差异。

然后停止，并明确要求我：在「记忆 → 身份与场景」中审阅并创建这些 Role / Scope，完成后回复你「Registry 已确认」。在我明确确认前，不得生成或提交重新分配 proposal。

### 阶段 C：我确认 Registry 后，规划 Claim 分配

收到我明确回复「Registry 已确认」后，重新运行：

```bash
notemd memory context-registry show --json
notemd memory list --status current --json
```

只有实际出现在最新 Registry 中且状态为 `active` 的 ID 才能作为目标。为每条 current Claim 给出：Claim ID、当前 Role/Scope、建议的完整 Role/Scope、证据、置信度、风险类型。

把目标完全相同且不跨 Realm 的 Claim ID 合并成小批次。首次初始化不要使用 `--all` 或宽泛的 `--where-*`；使用明确的 `--claim` 列表。每一批先执行：

```bash
notemd memory reassign plan \
  --claim "claim-id-1,claim-id-2" \
  --set-role "role:developer" \
  --set-space "space:product/notemd/release" \
  --json
```

必须核对：匹配数量等于 selector 数量、before/after 与表格一致、没有未知 ID、没有意外跨 Realm 或范围扩大。歧义、冲突、需要拆分或高风险项不要自动提交，留给我逐项处理。

通过检查的普通批次才可提交 proposal：

```bash
notemd memory reassign propose \
  --claim "claim-id-1,claim-id-2" \
  --set-role "role:developer" \
  --set-space "space:product/notemd/release" \
  --request-id "<你的稳定Agent-ID>/role-scope-bootstrap/v1/<确定性批次slug>" \
  --recorded-by "<你的稳定Agent-ID>" \
  --json
```

`request-id` 必须由目标 Role、Scope 和排序后的 Claim ID 确定，同一批重跑得到同一个值。不要加入日期、随机数或会变化的摘要。

所有 proposal 提交后停止。提醒我回到「记忆 → 待确认」逐条审阅 Agent 生成的重新分配建议；不要声称提案已经生效。「身份与场景 → 重新分配」是我需要自行发起并直接应用批次时使用的人工界面，不是 Agent proposal 的待审队列。

---

## 五、最终回复格式

严格按以下顺序输出：

1. `现有 Registry`
2. `建议的 Roles`
3. `建议的 Scope 树`
4. `证据与置信度`
5. `完整 Registry 候选 JSON`
6. `validate 结果`
7. `待我判断 / 需要拆分 / 高风险项`
8. `人工检查点或已提交的 reassignment proposals`
9. `执行记录`：只列实际运行过的只读、validate、plan、propose 命令及成功/失败，不输出凭据

不能确认的内容明确写「未知」。不要用漂亮但无法验证的标签填空，不要跳过人工检查点，也不要把 proposal 描述成已经批准或应用。
