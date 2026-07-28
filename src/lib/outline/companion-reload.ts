// src/lib/outline/companion-reload.ts
// 伴生 .note.md 被外部(别的 agent / 别的设备经 git 同步)改动后该怎么办。
// 纯决策、无 IO —— watcher 与写盘防线都用它,行为一致且可单测。
//
// 与主文档 file-watcher 的策略保持一致:hash 相同 = 自写回声,忽略;
// 干净就静默重载(最少打断);脏则绝不静默覆盖,交冲突横幅让人裁决。

export type CompanionReloadDecision = 'ignore' | 'reload' | 'conflict'

export function decideCompanionReload(args: {
  /** 盘上当前内容的 sha256 */
  diskHash: string
  /** 我们上次接受(读到或写出)的内容 hash;从未建立基线时为 null */
  lastHash: string | null
  /** 大纲树有未落盘的本地改动 */
  dirty: boolean
}): CompanionReloadDecision {
  if (args.lastHash != null && args.diskHash === args.lastHash) return 'ignore'
  return args.dirty ? 'conflict' : 'reload'
}
