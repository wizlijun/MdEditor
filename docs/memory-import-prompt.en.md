# Import-memory prompt (English)

The Memory window's **复制导入记忆Prompt** button copies the Chinese prompt at
`plugins-src/memory/src/lib/import-prompt.md`. That file is the source of truth
for what ships in the app; this document is the English rendering of the same
prompt, for notemd.net and the English README.

Keep the two in step: if one changes, change the other in the same commit.

---

## Copy everything below into another AI assistant

# Task: export what you remember about me as note.md memory claims

You are another AI assistant I use. On this machine I keep a vault managed by
note.md, and that vault is the shared long-term memory for every assistant I
work with. Export **the entries about me in your own memory system** as
commands note.md can import directly.

You are only **proposing**. Every entry arrives in my vault as pending and
takes effect only after I confirm it myself in note.md's Memory window. So err
towards listing one entry too many rather than deciding on my behalf to drop it.

---

## Step 1 · List them faithfully

1. Open your persistent memory (memory / long-term memory / custom
   instructions / user profile) and list **every** entry about me, one by one.
   Do not sample, do not merge, do not polish.
2. If you have no memory feature, or it is empty, answer "no memory entries to
   export" and stop. **Do not** improvise from this conversation.
3. If you cannot tell whether something is genuinely stored in your memory or
   was just picked up in this conversation, mark it uncertain and put it in the
   "for me to confirm" list at the end. Do not turn it into a command.

## Step 2 · Drop what should not be kept

**Must be dropped:**

- One-off, transient things: a single task, today's to-dos, session state,
  small talk.
- Facts whose subject is not me: colleagues, family, clients, other people's
  projects — even when they came up in my conversations.
- Anything you inferred from conversation but I never explicitly said or
  confirmed, including psychological profiles, personality judgements and
  background guesses. Put uncertain provenance in "For me to confirm"; do not
  generate a command for it.
- **Any credential**: passwords, API keys, tokens, private keys, government ID
  or card numbers, full home address, door codes. These **never** appear in the
  output, even if I told you once. Drop the whole entry and say only "dropped N
  entries containing credentials" at the end.

**Keep**: identity, stable preferences, behavioural boundaries, decisions I
have already made, long-term commitments, settled practices, and background
facts I explicitly asked you to remember.

## Step 3 · Split into atomic claims

One claim = one thing I could agree or disagree with on its own.

- "I use TypeScript, I hate any, keep PRs small" → three claims.
- Each one reads standalone: no "the above", "he", "this approach".
- Conditional claims must carry the condition: "in production code, use X",
  not "use X".
- Write in the language I normally use with you. Do not translate.
- One line, no line breaks.

## Step 4 · Label each claim

**scope and category** (which projection it lands in):

| scope | category | for |
|---|---|---|
| user | owner | who I am (name, handles, identity anchors) |
| user | identity | role, profession, affiliation, long-term position |
| user | preferences | taste, style, tools, language preferences |
| user | work-style | how I work and collaborate |
| user | boundaries | what I will not accept |
| memory | decisions | calls I have already made |
| memory | constraints | long-standing constraints and limits |
| memory | practices | practices I stick to |
| memory | context | background facts worth remembering long-term |

Use `other` when unsure.

**claim-kind** (pick one):
identity / preference / boundary / decision / belief /
observation / commitment / practice / material-fact / quotation

**The rest** follow this table. Do not improvise:

| flag | values | rule |
|---|---|---|
| polarity | positive / negative / neutral | "do this" → positive; "never do this" → negative; plain fact → neutral |
| risk-class | informational / behavioral / action-sensitive | boundary → action-sensitive; decision, practice, commitment → behavioral; everything else → informational |
| trust-tier | identity / stable-preference / contextual | identity kinds → identity; preference, boundary, practice → stable-preference; everything else → contextual |
| salience | normal / pinned | pinned only where I said "always" or "never forget this" |
| sensitivity | normal / private | health, family, finances, identity details → private |
| basis | owner-stated | export only entries I said or explicitly confirmed; inferred entries must not become commands |
| space | global or project/name | claims that only hold inside one project use project/name |
| purpose | planning / writing / information-answer / projection / sync | comma-separated, several allowed. Default to planning,writing,information-answer; style-only claims may use writing alone. Never external-action |
| provider-policy | deny / prompt / allow | private sensitivity → deny; otherwise prompt |

`--guidance` says how an assistant should use the claim; `--avoid-error` names
the specific mistake it exists to prevent. Both are required whenever polarity
is negative or claim-kind is boundary or practice.

Only when the stored memory contains a trustworthy effective time, add a full
RFC 3339 UTC value such as `--valid-from "2026-09-02T00:00:00Z"` to a claim
with a shelf life. Omit it when the exact time is unknown; do not guess.

## Step 5 · Output

Give me an overview table first, then **one** bash block holding one
`notemd memory propose create` per claim. Template:

```bash
notemd memory propose create \
  --request-id "your-name-memory-export/v1:todays-date:short-english-slug" \
  --recorded-by "product/model" \
  --scope user --category preferences --claim-kind preference \
  --text "In TypeScript I keep strict mode on and never fall back to any." \
  --basis owner-stated \
  --polarity negative --salience normal --sensitivity normal \
  --trust-tier stable-preference --risk-class informational \
  --space global --purpose "planning,writing,information-answer" --provider-policy prompt \
  --guidance "Default to strict TS; give a concrete type instead of widening." \
  --avoid-error "Reaching for any or @ts-ignore to silence a type error."
```

Hard requirements:

- `--recorded-by` is **your own** product and model, e.g. chatgpt/gpt-5 or
  gemini/2.5-pro. **Never** write a `human:` prefix — that prefix is reserved
  for me, and it is the only thing separating what I thought from what an AI
  wrote.
- `--request-id` is unique and re-runnable: exporting the same memory again
  must produce the same id, so a second run does not create a duplicate. Use
  lowercase hyphenated English for the slug.
- Quote every value. Text must not contain double quotes, backticks, dollar
  signs or backslashes; use single quotes if you need quotation marks.
- No `--vault`, no `sudo`, no loops or scripts, no `&&` chaining, and nothing
  executable outside that one block.
- Remind me to review every command in the bash block before running it. Do not
  claim that generated commands have already run or are safe.
- Do not call `notemd memory approve` / `reject` / `delete`. Those only accept
  my own action inside note.md and will refuse anything you write.

Close with two short sections:

1. **For me to confirm** — entries you were not sure were really in memory.
2. **One-line tally** — N exported, M dropped (K of them credentials).
