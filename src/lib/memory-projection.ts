/**
 * Memory Protocol v2 reserves USER.md and MEMORY.md at the configured Vault
 * root as disposable projections. Detection is path-based; no legacy marker or
 * frontmatter compatibility is retained.
 */
export function isMemoryProjectionPath(path: string, vaultRoot: string | null): boolean {
  if (!vaultRoot) return false
  const normalizedPath = path.replace(/\\/g, '/').replace(/\/+$/, '')
  const normalizedRoot = vaultRoot.replace(/\\/g, '/').replace(/\/+$/, '')
  return normalizedPath === `${normalizedRoot}/USER.md`
    || normalizedPath === `${normalizedRoot}/MEMORY.md`
}

let configuredVaultRoot: string | null = null

export function setMemoryProjectionVaultRoot(root: string | null): void {
  configuredVaultRoot = root
}

export function isConfiguredMemoryProjectionPath(path: string): boolean {
  return isMemoryProjectionPath(path, configuredVaultRoot)
}
