---
name: decision-log-extract
description: "Use when turning a day's raw content (voice transcripts, daily notes, chat/meeting logs) into decision-log candidate JSON (vault/diary/YYYY-MM-DD-decision.json) — surfacing new decisions the user made, verdict suggestions for due/triggered open decisions, and progress-driven updates to open decisions. Standalone: contains the full contract; no external references needed."
---

# Decision — Daily Decision Pipeline (standalone)

## What this is

Turn a day's raw content plus the user's current **open decisions** into ONE candidate JSON for the "Decision" note.md plugin. The plugin reads this file and surfaces the items for the user to confirm. You produce three kinds of entries and **output pure JSON, nothing else**:

1. **`new_candidates`** — new decisions the user made in the content.
2. **`closures`** — verdict suggestions for open decisions that are **due or triggered**.
3. **`edit_decisions`** — progress / breakthrough / resolved / abandoned updates for open decisions the content speaks to (not yet due).

**Core principle: you nominate/suggest, the user decides.** Never invent the user's prediction, confidence, verdict, or conclusion. You surface signals; the human ratifies them.

## Inputs

You are given:

- `date` — today, `YYYY-MM-DD`.
- `content` — the day's material (transcripts / diary / chat / meeting / progress notes). Ideally each line carries a speaker, a time, and a conversation id (`conv_id`).
- `open_decisions` — the user's current undecided decisions (read from `vault/decision/open.decision.note.md` front-matter `decisions[]`). Each entry has: `id`, `title`, `prediction`, `check-date` (YYYY-MM-DD), and optional `triggers` (`[{ if, source }]`). You use these to build `closures` and `edit_decisions`.
- `rejected` — items the user previously marked **inaccurate** (read from `vault/decision/_rejected.json`, shape `{ "rejected": [{ type: "candidate"|"closure"|"edit", decision_id?, title?, quote?, kind?, summary?, rejected_at }] }`). You must NOT re-surface these (see rule 6). If the file is absent, treat it as empty.

## Hard rules (violating any = failure)

1. **Never fabricate.**
   - `quoted` = the user actually voiced an expectation → quote it verbatim, then structure it.
   - `nominated` = it sounds like a decision but no expectation was stated → write `prediction` as a **question** (or `null`). Never an invented assertion.
   - For verdicts and updates likewise: report what the content shows; do not declare success/failure yourself.
2. **Rather omit than pad.** Only real decisions (a choice + a falsifiable expectation) and real developments. Chit-chat, facts, reminders, and to-dos are NOT decisions. If there is nothing, return empty arrays (`[]`). That is normal and correct.
3. **Unsure → leave empty / omit.** No default-value guessing. `confidence` stays `null` unless the user's own words reveal certainty. `state` is omitted entirely if the content does not reveal it. Optional fields are omitted, not filled with placeholders.
4. **One open decision → at most one array per day.** If it is due or triggered → `closures`. Otherwise, if the content shows a development → `edit_decisions`. Otherwise leave it alone. The same `decision_id` must never appear in both `closures` and `edit_decisions`.
5. **Do not over-close.** Only `resolved` / `abandoned` developments get terminal actions (`close-*` / `drop`). Mere progress or a breakthrough → `note`, keep the decision open.
6. **Respect rejections.** Do NOT re-surface anything the user marked inaccurate in `rejected`: skip a `new_candidate` whose decision is the same as a rejected `candidate` (matching title / quote / gist); skip a `closure` or `edit_decision` for a `decision_id` the user rejected for that kind of suggestion. When it's clearly the same thing they rejected, omit it. (A genuinely new, different development on that decision may still be surfaced.)
7. **Output must be `JSON.parse`-able** — a single JSON object, no prose, no markdown code fences, no preamble or trailing text.

## Output object

```json
{
  "date": "2026-07-22",
  "generated_by": "openclaw",
  "new_candidates": [],
  "closures": [],
  "edit_decisions": []
}
```

- `date` — today (`YYYY-MM-DD`), matches the target filename `vault/diary/<date>-decision.json`.
- `generated_by` — the producing pipeline/agent name (e.g. `"openclaw"`); if unknown use `"agent"`.
- Every item in every array has `status: "pending"`.

### Shared value types

- **Evidence**: `{ "conv_id"?: string, "quote": string, "time"?: string }` — `quote` is verbatim source text.
- **Confidence**: a probability number `0-1` or `null` — five anchors 0.55/0.65/0.75/0.85/0.95 (legacy `"low"|"medium"|"high"` still accepted, mapped to 0.6/0.75/0.9). Map the user's wording to the nearest anchor: e.g. "大概/勉强" → 0.55, "六成" → 0.65, "挺有把握" → 0.75, "很有把握" → 0.85, "几乎确定/一定" → 0.95. No wording of certainty → `null`.
- **Trigger**: `{ "if": string, "source"?: string }` — a natural-language re-open condition ("if X happens, reconsider"). `source` is the signal channel (e.g. `"openclaw"`).
- **State**: `{ "time"?: "HH:MM", "speech_rate"?: "slow"|"normal"|"fast", "calendar_density"?: "low"|"medium"|"high" }` — a passive snapshot of the user's state when deciding. Omit the whole object if not revealed.

### A. `new_candidates` — new decisions found in the content

Each item:

| field | rule |
| --- | --- |
| `id` | required. `cand-<date>-<NN>` with a two-digit per-day sequence, e.g. `cand-2026-07-22-01`, `-02`. You assign these in order; no randomness or LLM needed. |
| `title` | required. One short line, the user's phrasing. |
| `prediction_source` | required. `"quoted"` or `"nominated"`. |
| `quote` | **required iff `quoted`.** The user's exact words that voiced the expectation. Never rewritten. Omit for `nominated`. |
| `prediction` | `quoted`: the expectation structured into one falsifiable statement (what + expected evidence). `nominated`: a **question** (e.g. "你是预期 X 吗?") or `null`. **Never** an invented assertion for `nominated`. |
| `confidence` | a probability number `0-1` (five-star anchors: 0.55/0.65/0.75/0.85/0.95; legacy `low`/`medium`/`high` still accepted, mapped to 0.6/0.75/0.9) or `null`. Only non-null when the user's own words reveal how sure they are. This is a suggestion the user overwrites when signing. |
| `check_date` | `YYYY-MM-DD` or `null`. Fill only if the user mentioned a timeframe (compute the date from it); else `null`. |
| `premortem_hint` | optional string. Only when the user themselves voiced a failure risk for this decision; phrase it as the likely reason it failed. Never invent one. |
| `alternatives` | optional string array. Only options the user actually mentioned weighing and not choosing. Never invent. |
| `triggers` | optional array of Trigger. Add only if the user mentioned a re-open condition. Omit if none. |
| `state` | optional State snapshot. Omit entirely if the content doesn't reveal it. |
| `source` | optional Evidence pointing back to the origin (with `conv_id` if present) so the user can verify. |
| `status` | always `"pending"`. |

What counts: a choice plus a falsifiable expectation = a decision. Reminders/to-dos ("记得续域名") and facts ("供应商宕机了") are not decisions on their own.

### B. `closures` — verdict suggestions for due/triggered open decisions

Generate **only** for an open decision whose `check-date <= today` (`reason: "due"`) or one of whose `triggers` fired in today's content (`reason: "trigger"`). Never nag decisions that are neither due nor triggered.

| field | rule |
| --- | --- |
| `decision_id` | required. The `id` from `open_decisions`. |
| `reason` | required. `"due"` or `"trigger"`. |
| `suggested_outcome` | optional. `"hit"` / `"partial"` / `"miss"` — your read from the evidence. Omit if genuinely unsure (the user renders the final verdict). |
| `evidence` | optional array of Evidence supporting the suggestion, verbatim quotes. |
| `status` | always `"pending"`. |

### C. `edit_decisions` — content-driven updates to open decisions (not yet due)

For an open decision that did **not** go into `closures`, but which the content speaks to with a development:

| field | rule |
| --- | --- |
| `decision_id` | required. The `id` from `open_decisions`. |
| `kind` | required. `"progress"` (advanced, not concluded) / `"breakthrough"` (major step, still in progress) / `"resolved"` (expectation met or clearly not, and it's over) / `"abandoned"` (user decided to stop / shelve it). |
| `summary` | required. One factual sentence describing what happened (no exaggeration). |
| `suggested_action` | required. See the mapping below. |
| `new_check_date` | `YYYY-MM-DD` only when `suggested_action == "adjust-check-date"`; otherwise `null`. |
| `evidence` | optional array of Evidence, verbatim quotes. |
| `status` | always `"pending"`. |

**`kind` → `suggested_action` (do not over-close):**

| content signal | kind | suggested_action |
| --- | --- | --- |
| moved forward, not concluded | `progress` | `note` |
| timeline clearly shifted (earlier/later) | `progress` | `adjust-check-date` (+ `new_check_date`) |
| major breakthrough, still ongoing | `breakthrough` | `note` |
| the expectation was **met** | `resolved` | `close-hit` (partly met → `close-partial`) |
| the expectation **clearly failed** and it's over | `resolved` | `close-miss` |
| user decided **not to do it / shelved it** | `abandoned` | `drop` |

## Self-check before you output

- [ ] Every `quoted` candidate carries an unedited `quote`; every `nominated` `prediction` is a question or `null` (no invented assertion).
- [ ] `confidence` is non-null only when the user's words reveal it; `state` is omitted when unknown.
- [ ] `closures` contains only due/triggered decisions; `edit_decisions` only not-yet-due decisions with a real development.
- [ ] No `decision_id` appears in both `closures` and `edit_decisions`.
- [ ] Only `resolved`/`abandoned` use terminal actions (`close-*`/`drop`); progress/breakthrough use `note` (or `adjust-check-date`).
- [ ] `new_check_date` appears only with `adjust-check-date`.
- [ ] Every `status` is `"pending"`; the three arrays are `[]` when there's nothing.
- [ ] Nothing in the output re-surfaces something listed in `rejected`.
- [ ] Output is a single pure JSON object, `JSON.parse`-able, no fences or commentary.

## Worked example

**Input**

```
date: 2026-07-22
content:
  [08:12 cv_1 user] "「决策」插件我觉得两周内能发出可用版本。"    # new decision, voiced expectation
  [11:00 cv_1 user] "也许该把每周例会砍了。"                    # new decision, no expectation
  [14:20 cv_2 user] "记得给设计发反馈。"                        # to-do → skip
  [16:00 cv_2 user] "上周那个迁移昨天到期,一直稳,没回滚。"      # open A, DUE today → closure
  [17:30 cv_2 user] "多语言今天把 i18n 框架跑通了,大进展。"      # open B, not due, breakthrough → edit
  [19:00 cv_2 user] "上新定价那个方案我们决定不搞了。"            # open C, not due, abandoned → edit
open_decisions:
  - { id: "2026-07-08-01", title: "上线迁移方案", prediction: "上线后不回滚", check-date: "2026-07-22" }
  - { id: "2026-07-15-01", title: "做多语言支持", prediction: "Q3 上线三语", check-date: "2026-09-30" }
  - { id: "2026-07-05-02", title: "上新定价", prediction: "月流水+10%", check-date: "2026-08-01" }
```

**Output**

```json
{
  "date": "2026-07-22",
  "generated_by": "openclaw",
  "new_candidates": [
    {
      "id": "cand-2026-07-22-01",
      "title": "先做「决策」MVP",
      "prediction_source": "quoted",
      "quote": "「决策」插件我觉得两周内能发出可用版本",
      "prediction": "两周内发出可用 MVP",
      "confidence": null,
      "check_date": "2026-08-05",
      "source": { "conv_id": "cv_1", "quote": "我觉得两周内能发出可用版本", "time": "08:12" },
      "status": "pending"
    },
    {
      "id": "cand-2026-07-22-02",
      "title": "砍掉每周例会",
      "prediction_source": "nominated",
      "prediction": "砍掉每周例会后会更好吗?",
      "confidence": null,
      "check_date": null,
      "source": { "conv_id": "cv_1", "quote": "也许该把每周例会砍了", "time": "11:00" },
      "status": "pending"
    }
  ],
  "closures": [
    {
      "decision_id": "2026-07-08-01",
      "reason": "due",
      "suggested_outcome": "hit",
      "evidence": [{ "conv_id": "cv_2", "quote": "上周那个迁移昨天到期,一直稳,没回滚" }],
      "status": "pending"
    }
  ],
  "edit_decisions": [
    {
      "decision_id": "2026-07-15-01",
      "kind": "breakthrough",
      "summary": "i18n 框架跑通,多语言取得关键进展",
      "suggested_action": "note",
      "new_check_date": null,
      "evidence": [{ "conv_id": "cv_2", "quote": "多语言今天把 i18n 框架跑通了,大进展" }],
      "status": "pending"
    },
    {
      "decision_id": "2026-07-05-02",
      "kind": "abandoned",
      "summary": "定价方案决定不做了",
      "suggested_action": "drop",
      "new_check_date": null,
      "evidence": [{ "conv_id": "cv_2", "quote": "上新定价那个方案我们决定不搞了" }],
      "status": "pending"
    }
  ]
}
```

Reasoning shown by the example:

- "两周内能发" is a voiced expectation → `new_candidates` / `quoted`; `check_date` computed from "两周内"; `confidence: null` (no certainty word).
- "砍周会" is a new decision with no stated expectation → `nominated`, `prediction` is a question, not an assertion.
- "给设计发反馈" is a to-do → skipped.
- Migration is **due today** → `closures` (reason `due`, suggested `hit`); it is NOT also placed in `edit_decisions`.
- Multilingual is **not due but a breakthrough** → `edit_decisions` with `note` (kept open — a breakthrough does not close it).
- Pricing is **not due but abandoned** → `edit_decisions` with `drop`.

## Common mistakes

- A confident `prediction` for a `nominated` item — it must be a question or `null`.
- Filling `confidence` because a plan "feels" medium — only the user's words set it.
- The same `decision_id` in both `closures` and `edit_decisions`.
- Closing (`close-*` / `drop`) on mere progress — only `resolved` / `abandoned` get terminal actions.
- Inventing a `state` snapshot — omit it when the content doesn't reveal it.
- Wrapping the JSON in ````json` fences or adding any commentary.
