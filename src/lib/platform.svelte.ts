import { platform as tauriPlatform } from '@tauri-apps/plugin-os'

export type Platform = 'macos' | 'ios' | 'unknown'
export type FormFactor = 'desktop' | 'tablet' | 'phone'

let cached: Promise<Platform> | null = null

export function platform(): Promise<Platform> {
  if (cached !== null) return cached
  const result = Promise.resolve(tauriPlatform() as unknown as string).then(
    (raw: string): Platform => (raw === 'macos' || raw === 'ios' ? raw : 'unknown'),
  )
  cached = result
  return result
}

export const isIOS = async () => (await platform()) === 'ios'
export const isMacOS = async () => (await platform()) === 'macos'

/** Re-exported for convenience; the implementation is dependency-free so that
 *  `editor-kit/` can import it without pulling in `@tauri-apps/plugin-os`. */
export { isApplePlatformSync } from './platform-sync'

/** test-only escape hatch */
export function _resetCacheForTests() { cached = null }

/** Reactive form-factor signal. Initialized by `initFormFactor()` in main.ts. */
export const formFactor = $state<{ value: FormFactor }>({ value: 'desktop' })

let initialized = false
export async function initFormFactor(): Promise<void> {
  if (initialized) return
  initialized = true
  const p = await platform()
  const compute = () => {
    if (p !== 'ios') return 'desktop' as FormFactor
    return window.innerWidth < 768 ? 'phone' : 'tablet'
  }
  formFactor.value = compute()
  if (p === 'ios') {
    // Listener intentionally never removed — formFactor is a process-lifetime
    // singleton. Do not call initFormFactor more than once (see idempotency guard).
    window.addEventListener('resize', () => {
      formFactor.value = compute()
    })
  }
}

export function _resetInitForTests() { initialized = false }
