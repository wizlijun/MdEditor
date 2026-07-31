import { invoke } from '@tauri-apps/api/core'

export interface LogLine {
  ts: string
  source: string
  category: string
  level: string
  message: string
}

function stringifyArg(a: unknown): string {
  if (typeof a === 'string') return a
  if (a instanceof Error) return `${a.name}: ${a.message}`
  try { return JSON.stringify(a) } catch { return String(a) }
}

/** Tauri runtime's own IPC-transport chatter. These are emitted by
 *  tauri's core.js, not by app code. Forwarding them into the log bus is what
 *  turns a single orphaned-callback condition — after an IPC custom-protocol →
 *  postMessage fallback ("Load failed") strands a long-lived callback — into a
 *  tens-of-thousands-of-lines app.log flood: Rust keeps delivering to the dead
 *  callback id, tauri logs "Couldn't find callback id" for each, and the bridge
 *  would persist + re-broadcast every one. Still printed to the native console
 *  (original() runs first); we just don't persist/re-broadcast them. */
function isTauriTransportNoise(message: string): boolean {
  return (
    message.startsWith("[TAURI] Couldn't find callback id") ||
    message.includes('IPC custom protocol failed')
  )
}

let patched = false

/** Idempotent. Patches console.* to also forward into the backend log bus.
 *  HARD RULE: call the native console first, then report; swallow report
 *  failures — otherwise a reporting error logs, which re-enters here → loop. */
export function installConsoleBridge(): void {
  if (patched) return
  patched = true
  const map = [
    ['debug', 'debug'],
    ['info', 'info'],
    ['info', 'log'],
    ['warn', 'warn'],
    ['error', 'error'],
  ] as const
  for (const [level, method] of map) {
    const original = console[method].bind(console)
    console[method] = (...args: unknown[]) => {
      original(...args)
      const message = args.map(stringifyArg).join(' ')
      if (isTauriTransportNoise(message)) return
      void invoke('logs_append_frontend', { level, message }).catch(() => {})
    }
  }
}
