// prompts.ts — 「委托给 agent 的提示词」在 vault 里的落点,以及设置面板列出的
// 可编辑项。
//
// 提示词是**文件**,不是设置项:每个 task 模板的 `CLAUDE.md` 就是交给 agent 的
// 那段话(task.json 里的 `prompt` 只是一句调用语,真正的协议全写在 CLAUDE.md)。
// 所以「编辑提示词」= 用标准 md 编辑器打开那个文件——插件不再存第二份副本,也
// 就没有哪一份才算数的问题。
//
// 用户改完就一直有效:播种(task-template.ts / trace-template.ts)只在文件缺失
// 时写,存在即跳过,永不覆盖。

/** 所有 task 模板的根目录(vault 相对)。 */
export const TASKS_DIR = '.notemd/agent-tasks'

/** 一个 task 模板的提示词文件(vault 相对):`.notemd/agent-tasks/<id>/CLAUDE.md`。 */
export function promptPathFor(taskId: string): string {
  return `${TASKS_DIR}/${taskId}/CLAUDE.md`
}

/** 设置面板里的一行:点它就在主编辑器打开 `promptPathFor(taskId)`。 */
export interface PromptEntry {
  taskId: string
  /** 面向用户的名字:主任务用译文,指令用 `/名字`。 */
  label: string
}

/** promptEntries 需要的指令信息(directives.ts 的 DirectiveEntry 的子集)。 */
export interface PromptDirective {
  taskId: string
  display: string
}

/**
 * 可编辑的提示词清单:本插件的主任务(委托按钮跑的那个)排第一,其后是输入面
 * 发现到的每个 `/指令`——它们同样是「委托给 agent」,同样是一个 CLAUDE.md。
 *
 * 按 taskId 去重:主任务将来若也带上 `directive`,它会同时出现在两处,而同一个
 * 文件在同一张清单里列两行,只会让人以为有两份提示词。
 */
export function promptEntries(
  mainTaskId: string,
  mainLabel: string,
  directives: readonly PromptDirective[],
): PromptEntry[] {
  const out: PromptEntry[] = [{ taskId: mainTaskId, label: mainLabel }]
  const seen = new Set([mainTaskId])
  for (const d of directives) {
    if (seen.has(d.taskId)) continue
    seen.add(d.taskId)
    out.push({ taskId: d.taskId, label: `/${d.display}` })
  }
  return out
}
