/**
 * USER.md and MEMORY.md become controlled projections only after the Memory
 * workflow has marked them. Requiring both the reserved basename and a control
 * signal avoids making an unrelated file with the same name read-only.
 */
export function isManagedMemoryProjection(path: string, content: string): boolean {
  const name = path.replace(/\\/g, '/').split('/').pop()?.toUpperCase()
  if (name !== 'USER.MD' && name !== 'MEMORY.MD') return false

  const hasControlMarker = content.includes('<!-- notemd-memory-control -->')
  const hasManagedFrontmatter = /^managed:\s*$[\s\S]*?^\s+by:\s*notemd\.memory\s*$/m.test(content)
  const hasReadOnlyNotice = /GENERATED\s*\/\s*READ-ONLY/i.test(content)
  return hasControlMarker || hasManagedFrontmatter || hasReadOnlyNotice
}
