// 把一个平台条目并进已存在的 updater manifest(latest.json)。
//
// 背景:发版是**分两台机器、分两次**完成的 —— macOS 侧 `release.sh` 先建
// GitHub Release 并上传只含 `darwin-*` 的 latest.json;Windows 侧随后在**同一个
// tag** 下补上自己的安装包,并把 `windows-*` 条目并进那份 latest.json。
//
// 合并必须是「只增不减」:任何一次合并都不允许弄丢已经在线的平台条目,否则
// 另一平台的用户会突然收到「没有可用更新」甚至更新报错。这里的校验就是为了
// 让这种事在脚本层面发生不了,而不是靠人记得。
//
// 平台键名取自 tauri-plugin-updater 的 `updater_os()`/`updater_arch()`
// (src/updater.rs):`darwin|windows|linux` + `-` + `x86_64|aarch64|i686|armv7`。
// 客户端按 `{os}-{arch}-{installer}` → `{os}-{arch}` 的顺序找,所以我们写不带
// installer 后缀的通用键即可。

/** updater 认得的平台键。写错一个字客户端就是静默「无更新」,所以显式白名单。 */
export const KNOWN_PLATFORMS = [
  'darwin-x86_64',
  'darwin-aarch64',
  'windows-x86_64',
  'windows-aarch64',
  'windows-i686',
  'linux-x86_64',
  'linux-aarch64',
]

export class MergeError extends Error {}

/**
 * @param {object} manifest  已在线的 latest.json(已 JSON.parse)
 * @param {object} entry     {platform, url, signature, version}
 * @returns {object} 新的 manifest(不改动入参)
 */
export function mergePlatform(manifest, entry) {
  const { platform, url, signature, version } = entry

  if (!manifest || typeof manifest !== 'object' || Array.isArray(manifest)) {
    throw new MergeError('existing manifest is not a JSON object')
  }
  if (!manifest.platforms || typeof manifest.platforms !== 'object') {
    throw new MergeError('existing manifest has no `platforms` object')
  }
  if (!KNOWN_PLATFORMS.includes(platform)) {
    throw new MergeError(
      `unknown platform key ${JSON.stringify(platform)}; expected one of ${KNOWN_PLATFORMS.join(', ')}`,
    )
  }
  // 版本必须对得上。版本错位是最危险的一种错误:manifest 说自己是 6.808.3,
  // 里面却挂着 6.808.2 的 Windows 包 —— 客户端会下载并安装一个"降级/错版"
  // 的安装器,而且签名是对的、验签会通过,没有任何一层会拦住它。
  if (version && manifest.version !== version) {
    throw new MergeError(
      `version mismatch: manifest says ${manifest.version}, this build is ${version}. ` +
        `Windows 必须构建与 mac 同一个 tag 的代码 —— 先 git checkout v${manifest.version}。`,
    )
  }
  if (!signature || !signature.trim()) {
    throw new MergeError(
      `empty signature for ${platform}. 多半是 TAURI_SIGNING_PRIVATE_KEY_PASSWORD 没被设成空串,` +
        `导致签名步骤没真正跑(见 docs/windows-agent-brief.md §1)。`,
    )
  }
  if (!/^https:\/\//.test(url || '')) {
    throw new MergeError(`url must be an https URL, got ${JSON.stringify(url)}`)
  }
  // url 必须指向本 manifest 对应的 tag,否则会把用户送去下别的版本。
  if (manifest.version && !url.includes(`/v${manifest.version}/`)) {
    throw new MergeError(
      `url does not point at tag v${manifest.version}: ${url}`,
    )
  }

  const merged = {
    ...manifest,
    platforms: { ...manifest.platforms, [platform]: { signature, url } },
  }

  // 只增不减的硬校验:合并后原有的每个平台键都必须还在,且内容未被动过。
  for (const [k, v] of Object.entries(manifest.platforms)) {
    if (k === platform) continue
    if (JSON.stringify(merged.platforms[k]) !== JSON.stringify(v)) {
      throw new MergeError(`merge would alter existing platform ${k} — refusing`)
    }
  }
  return merged
}

/** 供 CLI 与测试共用的可读摘要。 */
export function describe(manifest) {
  return Object.keys(manifest.platforms).sort().join(', ')
}
