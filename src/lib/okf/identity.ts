// 本机人类身份(OKF §7 的 `human:<id>`)。
// 值由后端 `notemd_okf_human_id` 从 vault 的 git 身份/系统用户名推出(见
// src-tauri/src/okf/mod.rs),每个会话只取一次。取不到就退回 `human:local` ——
// 身份拿不准绝不能挡住"人工确认"这个动作本身。
import { actor } from './actor'

let cached: string | null = null

export async function humanActor(): Promise<string> {
  if (cached) return cached
  try {
    const [{ invoke }, { sotvaultStore }] = await Promise.all([
      import('@tauri-apps/api/core'),
      import('../sotvault.svelte'),
    ])
    const id = await invoke<string>('notemd_okf_human_id', { vaultPath: sotvaultStore.vaultRoot })
    cached = actor.human(id?.trim() ? id.trim() : 'local')
  } catch {
    cached = actor.human('local')
  }
  return cached
}

/** 测试/重登 vault 后清缓存。 */
export function resetHumanActor(): void {
  cached = null
}
