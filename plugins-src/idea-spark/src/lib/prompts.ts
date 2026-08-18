// prompts.ts — 「委托给 agent 的提示词」在 vault 里的落点。
//
// 提示词是**文件**,不是设置项:task 模板的 `CLAUDE.md` 就是交给 agent 的
// 那段话(task.json 里的 `prompt` 只是一句调用语,真正的协议全写在 CLAUDE.md)。
// 所以「编辑提示词」= 用标准 md 编辑器打开那个文件——插件不再存第二份副本,也
// 就没有哪一份才算数的问题。
//
// 用户改完就一直有效:播种(task-template.ts)只在文件缺失时写,存在即跳过,
// 永不覆盖。

/** 所有 task 模板的根目录(vault 相对)。 */
export const TASKS_DIR = '.notemd/agent-tasks'

/** 一个 task 模板的提示词文件(vault 相对):`.notemd/agent-tasks/<id>/CLAUDE.md`。 */
export function promptPathFor(taskId: string): string {
  return `${TASKS_DIR}/${taskId}/CLAUDE.md`
}
