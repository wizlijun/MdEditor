import { presetRange, type Preset } from '../insights/value'
import { localTzOffsetMinutes } from '../insights/model'
import { joinPath } from './cli-runner'
import type { CliFinishResult } from './share-cli'
import type { CliPayload } from './cli-runner'

export interface ReadingInsightsCliDeps {
  finish: (result: CliFinishResult) => Promise<void>
  resolveVault: () => Promise<string | null>
  generate: (from: string, to: string, vaultRoot: string) => Promise<{ filename: string; markdown: string }>
  mkdir: (path: string) => Promise<void>
  writeTextFile: (path: string, contents: string) => Promise<void>
  now?: () => number
  timezoneOffsetMinutes?: () => number
}

const PRESETS = ['today', 'yesterday', '7d', '30d', 'month'] as const

function validIsoDate(value: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false
  const date = new Date(`${value}T00:00:00Z`)
  return !Number.isNaN(date.getTime()) && date.toISOString().slice(0, 10) === value
}

export async function runReadingInsightsCli(
  payload: CliPayload,
  deps: ReadingInsightsCliDeps,
): Promise<void> {
  const fail = async (exitCode: number, code: string, message: string) => {
    await deps.finish({
      exit_code: exitCode,
      stdout: payload.global.json
        ? JSON.stringify({ ok: false, error: { code, message } })
        : undefined,
      stderr: payload.global.json ? [] : [`notemd: ${message}`],
    })
  }

  try {
    const vaultFlag = payload.flags.vault
    if (vaultFlag != null && (typeof vaultFlag !== 'string' || vaultFlag.length === 0)) {
      await fail(2, 'invalid_arguments', '--vault requires a non-empty path')
      return
    }
    const vaultRoot = typeof vaultFlag === 'string' && vaultFlag
      ? vaultFlag
      : await deps.resolveVault()
    if (!vaultRoot) {
      await fail(2, 'vault_required', 'no Vault configured. Pass --vault <path> or configure one in the app.')
      return
    }

    const fromFlag = payload.flags.from
    const toFlag = payload.flags.to
    const fromProvided = fromFlag != null
    const toProvided = toFlag != null
    if (fromProvided !== toProvided) {
      await fail(2, 'invalid_arguments', '--from and --to must be provided together')
      return
    }
    if (fromProvided && payload.flags.date != null) {
      await fail(2, 'invalid_arguments', '--date conflicts with --from/--to')
      return
    }

    let from: string
    let to: string
    if (fromProvided && toProvided) {
      from = fromFlag as string
      to = toFlag as string
      if (!validIsoDate(from) || !validIsoDate(to) || from > to) {
        await fail(2, 'invalid_date_range', '--from/--to must be valid YYYY-MM-DD dates with --from <= --to')
        return
      }
    } else {
      const dateFlag = payload.flags.date
      if (dateFlag != null && (typeof dateFlag !== 'string' || !PRESETS.includes(dateFlag as typeof PRESETS[number]))) {
        await fail(2, 'invalid_date_preset', `invalid --date preset '${String(dateFlag)}'. Valid: ${PRESETS.join(', ')}`)
        return
      }
      const preset = (dateFlag ?? 'yesterday') as Preset
      const range = presetRange(
        preset,
        (deps.now ?? Date.now)(),
        (deps.timezoneOffsetMinutes ?? localTzOffsetMinutes)(),
      )
      from = range.from
      to = range.to
    }

    const { filename, markdown } = await deps.generate(from, to, vaultRoot)
    if (payload.flags.stdout === true) {
      await deps.finish({
        exit_code: 0,
        stdout: payload.global.json
          ? JSON.stringify({ ok: true, data: { from, to, markdown } })
          : payload.global.quiet ? undefined : markdown,
        stderr: [],
      })
      return
    }

    const statDir = joinPath(vaultRoot, 'stat')
    await deps.mkdir(statDir)
    const path = joinPath(vaultRoot, filename)
    await deps.writeTextFile(path, markdown)
    await deps.finish({
      exit_code: 0,
      stdout: payload.global.json
        ? JSON.stringify({ ok: true, data: { from, to, path } })
        : payload.global.quiet ? undefined : `wrote ${path}`,
      stderr: [],
    })
  } catch (error) {
    await fail(1, 'reading_insights_failed', `reading-insights report failed: ${String(error)}`)
  }
}
