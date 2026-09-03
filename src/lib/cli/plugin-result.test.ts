import { describe, expect, it } from 'vitest'
import {
  decodePluginCliResult,
  finishForPluginCliResult,
  PLUGIN_CLI_RESULT_KEY,
} from './plugin-result'

describe('decodePluginCliResult', () => {
  it('preserves a structured report with a plugin-selected failure exit code', () => {
    const report = { blocked: 2, conflict: 1, items: [{ conversation_id: '20260903_120000' }] }
    expect(decodePluginCliResult({
      [PLUGIN_CLI_RESULT_KEY]: {
        exit_code: 4,
        message: 'migration did not finish cleanly',
        data: report,
      },
    })).toEqual({ exitCode: 4, message: 'migration did not finish cleanly', data: report })
  })

  it('does not reinterpret ordinary plugin objects that contain ok=false', () => {
    expect(decodePluginCliResult({ ok: false, reason: 'domain result' })).toBeNull()
  })

  it('rejects malformed or out-of-range envelopes', () => {
    expect(decodePluginCliResult({ [PLUGIN_CLI_RESULT_KEY]: { exit_code: 1.5 } })).toBeNull()
    expect(decodePluginCliResult({ [PLUGIN_CLI_RESULT_KEY]: { exit_code: 256 } })).toBeNull()
    expect(decodePluginCliResult({ [PLUGIN_CLI_RESULT_KEY]: { exit_code: 4, message: 42 } })).toBeNull()
  })

  it('prints a non-clean report as structured JSON and preserves exit code 4', () => {
    const report = { blocked: 1, errors: ['invalid transcript'] }
    const result = finishForPluginCliResult({
      [PLUGIN_CLI_RESULT_KEY]: {
        exit_code: 4,
        message: 'migration did not finish cleanly',
        data: report,
      },
    }, { json: true, quiet: false, pluginName: 'Meetings' })

    expect(result).toEqual({
      exit_code: 4,
      stdout: JSON.stringify({ ok: false, data: report }),
      stderr: [],
    })
  })

  it('keeps failure output visible in human quiet mode', () => {
    const report = { conflict: 1 }
    const result = finishForPluginCliResult({
      [PLUGIN_CLI_RESULT_KEY]: { exit_code: 4, message: 'conflict', data: report },
    }, { json: false, quiet: true, pluginName: 'Meetings' })

    expect(result.stdout).toBe(JSON.stringify(report))
    expect(result.stderr).toEqual(['✗ Meetings: conflict'])
  })
})
