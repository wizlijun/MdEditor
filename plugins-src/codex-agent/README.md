# Codex Agent for note.md

`notemd.codex-agent` connects the local Codex CLI to note.md's shared agent
workspace. It can run vault task templates from the plugin window, serve the
host's standard agent-provider commands, or detach long jobs from the
`notemd codex-agent` CLI.

## Requirements

- note.md 6.817.4 or later
- Codex CLI installed and authenticated (`codex --version`, `codex login status`)
- Existing Codex CLI authentication, or an invocation-scoped `CODEX_API_KEY`

Set `NOTEMD_CODEX_BIN` only when the CLI cannot be discovered from the login
shell or common install locations.

## Run model

Each run executes roughly this command, with the complete prompt sent on stdin:

```sh
codex -a never exec --json --ephemeral --skip-git-repo-check \
  -C <vault> --sandbox <mode> -m <effective-model> -
```

The process working directory is the Vault root. If a task does not explicitly
pin a model, the plugin asks Codex's local app-server for the effective model at
that Vault (including trusted project configuration and the Codex catalog
default), then passes that model to the run and records it in OKF provenance.
Model selection therefore stays in Codex; note.md does not maintain a separate
model setting.

The plugin maps Codex JSONL events into note.md's shared live stream, task
progress, history and artifact records. CLI runs detach by default so they can
outlive note.md's temporary headless command process. Poll a returned run id
through the provider's `run-status` command when a caller needs completion.

## Permissions

`policy.json` supports Codex's native `read-only`, `workspace-write` and
`danger-full-access` sandbox modes. Runs use approval policy `never`, so they do
not stall waiting for an invisible terminal prompt.

The plugin keeps the user's Codex configuration enabled so harness-provided
skills, MCP servers and other tools remain available. Those external tools may
have their own permissions outside Codex's filesystem sandbox; review that
configuration before treating a sandbox label as a complete security boundary.

For `workspace-write`, the Vault is the primary workspace. This is a
vault-level boundary, not an exact-file allowlist. Task instructions still
constrain a note-scoped run to the requested `.note.md`, but that instruction
is not a stronger OS sandbox.

Codex non-interactive mode: <https://learn.chatgpt.com/docs/non-interactive-mode>

## Task files

Tasks live under `.notemd/agent-tasks/<id>/` and are shared with other note.md
agent providers. Codex refreshes only its own `CODEX.md`; shared `task.json`,
`AGENTS.md`, `policy.json` and precheck scripts are created only when missing,
so installing multiple providers does not create a rewrite loop.

Derived run data lives under `.notemd/agent-runs/` and is gitignored. Markdown
artifacts remain ordinary vault files and receive OKF provenance when needed.
