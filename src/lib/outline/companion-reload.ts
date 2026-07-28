// src/lib/outline/companion-reload.ts
// 伴生 .note.md 被外部(别的 agent / 别的设备经 git 同步)改动后该怎么办。
// 纯决策、无 IO —— 可单测,行为一目了然。
//
// 与主文档 file-watcher 的策略一致:自写回声忽略;干净就静默重载(最少打断);
// 脏则绝不静默覆盖,交冲突横幅让人裁决。

export type CompanionReloadDecision = 'ignore' | 'reload' | 'conflict'

export interface CompanionReloadInput {
  /** 盘上当前内容的 sha256 */
  diskHash: string
  /** 我们上次接受(读到或写出)的**磁盘原始字节** hash;从未建立基线时 null */
  lastHash: string | null
  /** 内存树当前序列化后的 hash;算不出时 null */
  ourHash: string | null
  /**
   * 上次接受的内容经「解析→序列化」后的**规范形** hash;算不出时 null。
   *
   * 为什么不能直接拿 `ourHash` 和 `lastHash` 比:`lastHash` 是磁盘原始字节,而
   * `ourHash` 来自解析后再序列化。手写的笔记(4 空格缩进、条目间空行、结尾无换行、
   * 没有 front-matter……)经这一趟必然与原始字节不同 —— 若拿两者比,这类文件会
   * **永远**被判成脏,外部改动永远走冲突横幅、永远等不到静默重载。规范形基线才是
   * 同一坐标系下的比较对象。
   */
  canonicalHash: string | null
  /**
   * intent-save 已激活(用户确有写笔记的意图)。未激活时树纯属从主文档派生,
   * 重载不会丢任何人写的东西 —— 无基线可比时用它兜底,好过一律当脏。
   */
  armed: boolean
  /** 最后的兜底脏标志(派生同步也会置真,故只在算不出 hash 时才用) */
  dirtyFlag: boolean
}

export function decideCompanionReload(args: CompanionReloadInput): CompanionReloadDecision {
  // 自己刚写下去的那一份
  if (args.lastHash != null && args.diskHash === args.lastHash) return 'ignore'
  // 内存树序列化后与盘上完全一致:没有要载入的新东西,也没有会被覆盖的东西
  if (args.ourHash != null && args.ourHash === args.diskHash) return 'ignore'

  const dirty = args.ourHash != null && args.canonicalHash != null
    ? args.ourHash !== args.canonicalHash
    : args.dirtyFlag && args.armed
  return dirty ? 'conflict' : 'reload'
}
