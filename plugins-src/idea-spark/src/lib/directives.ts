// 指令 = task 模板:.notemd/agent-tasks/<id>/task.json 里 directive 非空的模板,
// 在输入面以 `/名字` 调用。发现走 vault RPC,全容错(参考 state-io.ts 的 parseState
// 与 agent-run-core task.rs「task.json 坏了就跳过」的先例)——一个坏模板不拉黑
// 整张指令表。
const TASKS_DIR = '.notemd/agent-tasks'

export interface DirectiveEntry {
  taskId: string
  names: string[]
  display: string
  description: string
}

export interface DirectiveIo {
  list(path: string): Promise<{ entries: Array<{ name: string; is_dir: boolean }> }>
  read(path: string): Promise<{ content: string }>
}

/** `/溯源 只查论文\n> 引文` → { name:'溯源', rest:'只查论文\n> 引文' };非指令输入 → null */
export function parseDirectiveInput(text: string): { name: string; rest: string } | null {
  const t = text.trimStart()
  if (!t.startsWith('/')) return null
  const m = /^\/(\S+)([\s\S]*)$/.exec(t)
  if (!m) return null
  return { name: m[1], rest: m[2].trim() }
}

export async function discoverDirectives(io: DirectiveIo): Promise<DirectiveEntry[]> {
  let names: string[]
  try {
    const { entries } = await io.list(TASKS_DIR)
    names = entries.filter((e) => e.is_dir).map((e) => e.name)
  } catch {
    return []
  }
  const out: DirectiveEntry[] = []
  for (const id of names) {
    try {
      const { content } = await io.read(`${TASKS_DIR}/${id}/task.json`)
      const t = JSON.parse(content) as { directive?: unknown; description?: unknown }
      const directive = Array.isArray(t.directive)
        ? t.directive.filter((n): n is string => typeof n === 'string' && n !== '')
        : []
      if (directive.length === 0) continue
      out.push({
        taskId: id,
        names: directive,
        display: directive[0],
        description: typeof t.description === 'string' ? t.description : '',
      })
    } catch {
      /* 坏模板跳过 */
    }
  }
  return out
}

export function matchDirective(entries: DirectiveEntry[], name: string): DirectiveEntry | null {
  return entries.find((e) => e.names.includes(name)) ?? null
}
