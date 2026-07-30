# Claude Agent

A general **headless runner**, not a chat window. It runs `claude -p` inside a
task template that lives in your vault, streams the run into a window, and
writes one record per run. What the run actually *does* is defined by the
template — the plugin only starts it, watches it, and records it.

设计文档:`docs/superpowers/specs/2026-07-30-claude-headless-agent-plugin-design.md`

## Prerequisites

- **Claude Code installed and logged in.** Verify with `claude --version` and
  `claude -p "reply ok" --output-format json`.
- A vault configured in note.md.

The plugin looks for the `claude` binary in three tiers: `NOTEMD_CLAUDE_BIN` →
your login shell's `command -v claude` → the usual install locations
(`~/.claude/local/claude`, `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`).
A GUI app inherits a lean PATH, so if the window says *claude executable not
found*, set `NOTEMD_CLAUDE_BIN` to its absolute path.

For a machine with no interactive login, `claude setup-token` produces a long-
lived OAuth token; the plugin passes `CLAUDE_CODE_OAUTH_TOKEN` through to the
child process if it's set in the environment.

## Task templates

```
<vault>/.notemd/agent-tasks/<id>/
├── task.json             # what to run
├── CLAUDE.md             # the task's instructions (auto-loaded)
├── .claude/
│   ├── settings.json     # permission allowlist, with ${VAULT} placeholders
│   └── skills/…          # optional, auto-discovered
└── .mcp.json             # optional, auto-discovered
```

Plain text, git-tracked, yours to edit. Running one outside note.md with
`cd <task> && claude -p …` is exactly equivalent.

`task.json`:

| Field | Required | Meaning |
|---|---|---|
| `name` | ✅ | Display name in the window |
| `description` | | One line under the name |
| `prompt` | | The template's own prompt (part ① below) |
| `max_turns` | | → `--max-turns`; omitted if unset |
| `timeout_seconds` | | Default 1800; the run is killed past this |
| `model` | | → `--model`; omitted if unset |

### The prompt is three parts, in this fixed order

1. `task.json`'s `prompt`
2. what you typed in the window this run (or `-p` on the CLI)
3. the current-document context, when the checkbox is on:
   ```
   ## 当前文档
   路径:<absolute path>
   选中内容:
   <selection>
   ```

Empty parts are dropped. Write templates expecting exactly this order.

### Working directory

The run's cwd is the **task template directory**. That matters: Claude Code
walks *up* looking for `CLAUDE.md`, so it loads both your vault's conventions
and the task's instructions, and it discovers `.claude/skills` and `.mcp.json`
next to the template. `--bare` is deliberately never passed — it would skip all
of that.

Your notes are read and written by absolute path, gated by the allowlist.

### `${VAULT}` in settings.json

`.claude/settings.json` stays portable by writing `${VAULT}` instead of a
machine path:

```json
{ "permissions": { "allow": ["Read(${VAULT}/**)", "Write(${VAULT}/**/*.note.md)"] } }
```

Before each run the plugin substitutes the real vault root into
`.claude/settings.local.json` — Claude Code's own local override layer. The
template is never rewritten, so it survives moving between machines.

Headless runs have nobody to click "approve", so anything not on the allowlist
silently doesn't happen. Grant deliberately: the built-in `answer-note-question`
template withholds write access to source `.md` files, so its "never modify the
source" rule is enforced by permissions rather than by the prompt alone.

## Built-in templates

- **`selfcheck`** — reports which CLAUDE.md files loaded, which skills are
  visible, the cwd and vault root, and the granted permissions. Run this first;
  it's the acceptance test for the whole setup.
- **`answer-note-question`** — answers the open questions captured in sidecar
  `.note.md` files (`type:: question`, `status:: open`), following the protocol
  in `docs/superpowers/specs/2026-07-27-annotation-qa-loop-design.md`: reply as
  a `✦` child node, flip `status::` to `answered`, and **never** to `closed` —
  only a person closes a question.

Both are seeded on first activation and **never overwritten**; once you edit
one, it's yours.

## CLI

```
notemd agent <task> [-p "extra prompt"] [--wait]
```

Detached by default: it returns a run id immediately and the work continues in
a separate process. That's not a preference — the host runs CLI subcommands in a
throwaway headless instance capped at 300 seconds that would kill the child at
exit, and answering a batch of questions routinely runs longer. Detaching is what makes `notemd agent`
usable from cron.

`--wait` blocks and returns the result instead, subject to that 300-second cap.

## Concurrency

One run per task at a time; different tasks run in parallel. The lock is a file
under `.notemd/agent-runs/<task>/`; if a process dies holding it, the next run
sees a dead pid and reclaims it.

## Run records

`.notemd/agent-runs/<task>/runs/<runId>.json` holds the outcome: status, exit
code, turn count, session id, the result text (8 KB tail) and, on failure, the
last 2 KB of stderr. The full event stream is **not** persisted — it exists for
the window to watch live. The whole `agent-runs/` tree is added to the vault's
`.gitignore`.

Hitting a rate limit isn't retried automatically; it shows up as the run's
result text, and you decide when to run again.

## Usage boundary

`claude -p` draws on your Claude subscription, which Anthropic permits for
**your own automation**. Don't wrap this into a multi-user service — that
violates the subscription terms. (The Agent SDK is a separate path that requires
an API key; this plugin deliberately doesn't use it.)
