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

/**
 * 已解析的身份,**同步**取。未预热时返回 null —— 调用方(⌘N 这类热路径)
 * 拿 null 就不签,绝不为了一个署名去阻塞两次 git 子进程。窗口由
 * `warmHumanActor()` 在启动时关掉。
 */
export function humanActorNow(): string | null {
  return cached
}

/** 后台预热缓存。不返回、不抛错——预热失败最多就是这一轮不签。 */
export function warmHumanActor(): void {
  void humanActor().catch(() => {})
}

/** 测试/重登 vault 后清缓存。 */
export function resetHumanActor(): void {
  cached = null
}
