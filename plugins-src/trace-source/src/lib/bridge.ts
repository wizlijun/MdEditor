// src/lib/bridge.ts — typed accessor for the host-injected `window.notemd`
// fetch-RPC bridge (see src-tauri/src/plugin_runtime/windows.rs bridge_script).
//
// A plugin window has ZERO Tauri IPC; every host effect goes through
// `notemd.request(method, params)`, which POSTs to `plugin://<id>/__rpc__` and
// resolves with the method's `result` (or throws on `error`). This is the
// subset of host methods this plugin actually consumes.

/** The bridge surface the host injects as an initialization script. */
export interface NotemdBridge {
  pluginId: string
  /** BCP-ish locale code the host resolved from settings: 'en' | 'zh' | 'ja' | 'de'. */
  locale: string
  /** Active UI theme id (unused by this plugin; color-scheme handles appearance). */
  theme: string
  /** Call a host method; resolves with its result, rejects with an Error on RPC error. */
  request(method: string, params?: unknown): Promise<any>
  /** Subscribe to host→UI pushes (seed / tray-activate / theme-changed). */
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
 * `host.vault.remove` — deletes ONE file (vault-relative path). A directory is
 * refused; a path that doesn't exist resolves as success (idempotent); a
 * symlink is removed as the link itself, never followed.
 */
export function vaultRemove(path: string): Promise<{ ok: true }> {
  return bridge().request('host.vault.remove', { path })
}

/** `host.editor.open` — opens a vault-relative path in the main window's editor. */
export function editorOpen(path: string): Promise<unknown> {
  return bridge().request('host.editor.open', { path })
}

/**
 * `host.agent.status` → `{state:'done',record}` / `{state:'running',steps,last}`
 * / `{state:'lost'}`. Interpretation lives in `delegate.ts`. `task` is required
 * on purpose: the agent's own handler DEFAULTS a missing task to another
 * plugin's task and would silently report on the wrong run directory.
 */
export function agentStatus(task: string, runId: string): Promise<unknown> {
  return bridge().request('host.agent.status', { task, run_id: runId })
}

/**
 * The tray reminder the agent plugin pushes when a run reaches a terminal
 * state. ALL FOUR fields are required — claude-agent deserializes this into a
 * struct with no defaults and fails the whole call on a missing key.
 * `open_path` / `expect_file` must be ABSOLUTE.
 *
 * The reminder is sent by the agent plugin, NOT by this window, and that is
 * the point: this window is torn down when it closes, the resident agent
 * plugin is not — the run and its notification outlive us.
 */
export interface AgentNotify {
  title_ok: string
  title_fail: string
  open_path: string
  expect_file: string
}

export interface AgentRunParams {
  /** Task id under `.notemd/agent-tasks/` — `trace-source` for this plugin. */
  task: string
  /** The delegation text, appended to the task template's own prompt. */
  prompt?: string
  notify: AgentNotify
  /** Which agent should run it. Omitted = whatever the host would pick. */
  harness?: string
}

/**
 * `host.agent.run` → `{ run_id }`. When no agent plugin is installed or it
 * cannot be activated, the rejection's message is prefixed `agent_unavailable:`
 * — there is no dedicated error code, so the prefix is the only signal.
 */
export function agentRun(params: AgentRunParams): Promise<{ run_id: string }> {
  return bridge().request('host.agent.run', params)
}

/**
 * `host.agent.providers` → every installed agent plus the harness behind it.
 * Feeds the `by X ▾` picker beside the delegate button — the host answers from
 * one place so the control is the same on every surface.
 */
export function agentProviders(): Promise<AgentProviders> {
  return bridge().request('host.agent.providers', {})
}
