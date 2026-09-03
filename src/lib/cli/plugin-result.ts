export const PLUGIN_CLI_RESULT_KEY = '__notemd_cli_result'

export interface PluginCliResult {
  exitCode: number
  data: unknown
  message?: string
}

export interface PluginCliFinish {
  exit_code: number
  stdout?: string
  stderr: string[]
}

/**
 * Decode the opt-in result envelope used by plugins that need to return
 * structured data together with a non-zero CLI exit code. Ordinary plugin
 * results remain untouched, including objects that happen to contain `ok`.
 */
export function decodePluginCliResult(value: unknown): PluginCliResult | null {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return null
  const envelope = (value as Record<string, unknown>)[PLUGIN_CLI_RESULT_KEY]
  if (envelope == null || typeof envelope !== 'object' || Array.isArray(envelope)) return null

  const fields = envelope as Record<string, unknown>
  const exitCode = fields.exit_code
  if (!Number.isInteger(exitCode) || (exitCode as number) < 0 || (exitCode as number) > 255) {
    return null
  }
  const message = fields.message
  if (message != null && typeof message !== 'string') return null

  return {
    exitCode: exitCode as number,
    data: fields.data ?? {},
    ...(typeof message === 'string' && message.length > 0 ? { message } : {}),
  }
}

export function finishForPluginCliResult(
  value: unknown,
  options: { json: boolean; quiet: boolean; pluginName: string },
): PluginCliFinish {
  const pluginResult = decodePluginCliResult(value)
  const data = pluginResult?.data ?? value
  const exitCode = pluginResult?.exitCode ?? 0
  const path = data != null && typeof data === 'object'
    ? (data as Record<string, unknown>).path
    : undefined
  const stdout = options.json
    ? JSON.stringify({ ok: exitCode === 0, data: data ?? {} })
    : options.quiet && exitCode === 0
      ? undefined
      : typeof path === 'string'
        ? path
        : JSON.stringify(data ?? {})
  const stderr = exitCode === 0 || options.json
    ? []
    : [`✗ ${options.pluginName}: ${pluginResult?.message ?? `plugin requested exit code ${exitCode}`}`]
  return { exit_code: exitCode, stdout, stderr }
}
