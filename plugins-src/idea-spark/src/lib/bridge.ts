// src/lib/bridge.ts — typed accessor for the host-injected `window.notemd`
// fetch-RPC bridge (see src-tauri/src/plugin_runtime/windows.rs bridge_script).
//
// A plugin window has ZERO Tauri IPC; every host effect goes through
// `notemd.request(method, params)`, which POSTs to `plugin://<id>/__rpc__` and
// resolves with the method's `result` (or throws on `error`).

/** The bridge surface the host injects as an initialization script. */
export interface NotemdBridge {
  pluginId: string
  /** BCP-ish locale code the host resolved from settings: 'en' | 'zh' | 'ja' | 'de'. */
  locale: string
  /** Active UI theme id (unused by this plugin; color-scheme handles appearance). */
  theme: string
  /** Call a host method; resolves with its result, rejects with an Error on RPC error. */
  request(method: string, params?: unknown): Promise<any>
  /** Subscribe to host→UI pushes (unused by this plugin; present for completeness). */
  onMessage(cb: (payload: unknown) => void): void
}

import type { AgentProviders } from './agent-picker/types'
declare global {
  interface Window {
    notemd: NotemdBridge
  }
}

/** The injected bridge. Throws if accessed outside a host plugin window. */
export function bridge(): NotemdBridge {
  const b = window.notemd
  if (!b) throw new Error('window.notemd bridge missing (not running inside a plugin window)')
  return b
}

// ── host method result shapes (subset this plugin consumes) ──────────────────

export interface VaultInfo {
  root: string | null
  wiki_dir: string | null
  daily_dir: string | null
}

/** `host.vault.info` → root + configured wiki/daily dir names. */
export function vaultInfo(): Promise<VaultInfo> {
  return bridge().request('host.vault.info')
}

/** `host.vault.read` → file content (vault-relative path). */
export function vaultRead(path: string): Promise<{ content: string }> {
  return bridge().request('host.vault.read', { path })
}

/** `host.vault.write` — writes text, creating parent dirs (vault-relative path). */
export function vaultWrite(path: string, content: string): Promise<{ ok: true }> {
  return bridge().request('host.vault.write', { path, content })
}

/** `host.vault.exists` → whether a vault-relative path exists. */
export function vaultExists(path: string): Promise<{ exists: boolean }> {
  return bridge().request('host.vault.exists', { path })
}

/** `host.vault.list` → directory entries (vault-relative path). */
export function vaultList(path: string): Promise<{ entries: { name: string; is_dir: boolean }[] }> {
  return bridge().request('host.vault.list', { path })
}

/**
 * `host.vault.remove` — deletes ONE file (vault-relative path).
 *
 * Host semantics worth knowing at the call site: a directory is refused (only
 * `remove_file` is ever called), a path that doesn't exist resolves as success
 * (idempotent, so a retry after a partial failure is safe), and a symlink is
 * removed as the link itself — never followed to its target.
 */
export function vaultRemove(path: string): Promise<{ ok: true }> {
  return bridge().request('host.vault.remove', { path })
}

/**
 * The tray reminder claude-agent pushes when a run reaches a terminal state.
 *
 * ALL FOUR fields are required — claude-agent deserializes this into a struct
 * with no defaults and fails the whole `run-task` call on a missing key (see
 * `NotifySpec` in plugins-src/claude-agent/backend/src/plugin.rs). `open_path`
 * and `expect_file` must be ABSOLUTE: the first is handed to the host's
 * reminder registry, the second is `is_file()`-checked to decide whether a
 * `success` record actually delivered anything.
 *
 * The reminder is sent by claude-agent, NOT by this plugin, and that is the
 * whole point: a plugin with an open window is torn down when that window
 * closes, which would take its reminder with it. claude-agent is resident, so
 * the run — and the notification — outlive this window. (Hence also: no
 * `notify` capability in our manifest. The tray registry does not deduplicate,
 * so a second push from here would show the user two reminders for one run.)
 */
export interface AgentNotify {
  title_ok: string
  title_fail: string
  open_path: string
  expect_file: string
}

export interface AgentRunParams {
  /** Task id under `.notemd/agent-tasks/` — `idea-proof` for this plugin. */
  task: string
  /** Extra prompt text, appended to the task template's own prompt. */
  prompt?: string
  /** ABSOLUTE path of the file the run is about. claude-agent `canonicalize`s
   *  it, so the file MUST already exist — flush the editor to disk first.
   *  Optional in the run-task schema; directive runs are self-contained and
   *  omit it. */
  note_path?: string
  notify: AgentNotify
  /** Which agent should run it. Omitted = whatever the host would pick. */
  harness?: string
}

/**
 * `host.agent.run` → `{ run_id }`. The host relays this verbatim to the
 * resident `notemd.claude-agent` plugin (capability `agent`).
 *
 * When claude-agent is not installed or cannot be activated, the rejection's
 * message is prefixed `agent_unavailable:` — there is no dedicated error code
 * (it arrives as the generic -32000), so the prefix is the only thing that
 * distinguishes "no agent" from "the run was refused".
 */
export function agentRun(params: AgentRunParams): Promise<{ run_id: string }> {
  return bridge().request('host.agent.run', params)
}

/**
 * `host.agent.providers` → every installed agent plus the harness behind it.
 *
 * Feeds the `by X ▾` picker beside the delegate button. The host answers from
 * one place so the control is the same here as in a sidecar note or the ebook
 * queue; this window does not get to invent its own idea of what an agent is.
 */
export function agentProviders(): Promise<AgentProviders> {
  return bridge().request('host.agent.providers', {})
}

/**
 * `host.agent.status` → `{state:'done',record}` / `{state:'running',steps,last}`
 * / `{state:'lost'}`. Interpretation lives in `agent-client.ts`.
 *
 * `task` is a required argument of this wrapper on purpose: claude-agent's own
 * handler DEFAULTS a missing `task` to `answer-note-question`, so an omitted
 * one doesn't fail — it silently reports on the wrong task's run directory.
 */
export function agentStatus(task: string, runId: string): Promise<unknown> {
  return bridge().request('host.agent.status', { task, run_id: runId })
}

/**
 * `host.vault.rename` — moves a file within the vault (both ends vault-relative).
 *
 * NEVER clobbers: an existing `to` rejects with an "exists" error and leaves
 * both ends untouched (the host does the check atomically), so a caller may
 * treat rejection as "that name is spoken for" without racing anything.
 * Missing parent directories of `to` are created.
 */
export function vaultRename(from: string, to: string): Promise<{ ok: true }> {
  return bridge().request('host.vault.rename', { from, to })
}
