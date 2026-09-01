export type Locale = 'en' | 'zh' | 'ja' | 'de'

export type MessageKey =
  | 'app.title'
  | 'app.value'
  | 'filter.projects'
  | 'common.loading'
  | 'common.refresh'
  | 'common.open'
  | 'common.cancel'
  | 'common.save'
  | 'common.optional'
  | 'common.error'
  | 'empty.wip'
  | 'empty.waiting'
  | 'empty.capture'
  | 'empty.dormant'
  | 'empty.closed'
  | 'empty.search'
  | 'section.wip'
  | 'section.waiting'
  | 'section.capture'
  | 'section.resurfaced'
  | 'section.placed'
  | 'section.repair'
  | 'count.wip'
  | 'count.waiting'
  | 'action.placeOne'
  | 'action.newTask'
  | 'action.newIdea'
  | 'action.settings'
  | 'action.hideCapture'
  | 'action.findPlaced'
  | 'action.hidePlaced'
  | 'action.place'
  | 'action.reopen'
  | 'action.relink'
  | 'action.openSource'
  | 'badge.proofed'
  | 'badge.task'
  | 'badge.agent'
  | 'badge.orphan'
  | 'badge.unsupported'
  | 'badge.dueNone'
  | 'badge.due'
  | 'badge.overdue'
  | 'badge.contextMissing'
  | 'metadata.label'
  | 'priority.P0'
  | 'priority.P1'
  | 'priority.P2'
  | 'priority.P3'
  | 'warning.wip'
  | 'warning.waiting'
  | 'warning.readOnly'
  | 'search.placeholder'
  | 'board.dragHelp'
  | 'create.title'
  | 'create.destination'
  | 'create.field'
  | 'create.placeholder'
  | 'create.save'
  | 'create.saveShortcut'
  | 'task.create.title'
  | 'task.create.destination'
  | 'task.create.field'
  | 'task.create.placeholder'
  | 'task.create.body'
  | 'task.create.bodyPlaceholder'
  | 'task.create.doneWhen'
  | 'task.create.doneWhenPlaceholder'
  | 'task.create.saveInbox'
  | 'task.create.saveCurrent'
  | 'task.create.saveShortcut'
  | 'sheet.title'
  | 'sheet.choose'
  | 'route.commit'
  | 'route.commit.detail'
  | 'route.wait'
  | 'route.wait.detail'
  | 'route.park'
  | 'route.park.detail'
  | 'route.settle'
  | 'route.settle.detail'
  | 'field.commitment'
  | 'field.commitment.placeholder'
  | 'field.nextAction'
  | 'field.nextAction.placeholder'
  | 'field.closeCondition'
  | 'field.closeCondition.placeholder'
  | 'field.waitingFor'
  | 'field.waitingFor.placeholder'
  | 'field.reviewAt'
  | 'field.wakeTrigger'
  | 'field.wakeTrigger.placeholder'
  | 'field.wakeTrigger.help'
  | 'field.exitKind'
  | 'field.exitKind.placeholder'
  | 'field.exitVia'
  | 'field.exitVia.placeholder'
  | 'field.reason'
  | 'field.reason.placeholder'
  | 'field.target'
  | 'field.target.placeholder'
  | 'field.result'
  | 'field.result.placeholder'
  | 'field.project'
  | 'field.priority'
  | 'field.due'
  | 'field.contexts'
  | 'field.contexts.placeholder'
  | 'settings.title'
  | 'settings.description'
  | 'settings.wipLimit'
  | 'settings.wipLimit.help'
  | 'settings.defaultPriority'
  | 'settings.defaultPriority.help'
  | 'settings.defaultDueDays'
  | 'settings.defaultDueDays.help'
  | 'settings.defaultContext'
  | 'settings.defaultContext.help'
  | 'settings.validation'
  | 'settings.save'
  | 'settings.saving'
  | 'settings.saveError'
  | 'field.project.placeholder'
  | 'field.project.selected'
  | 'field.project.existing'
  | 'field.project.add'
  | 'field.project.help'
  | 'field.project.target'
  | 'field.project.target.placeholder'
  | 'project.suggestion'
  | 'project.suggestion.detail'
  | 'field.article'
  | 'field.article.placeholder'
  | 'preset.commit.verify'
  | 'preset.commit.prototype'
  | 'preset.commit.plan'
  | 'preset.commit.deliver'
  | 'preset.next.evidence'
  | 'preset.next.experiment'
  | 'preset.next.draft'
  | 'preset.next.user'
  | 'preset.close.decision'
  | 'preset.close.prototype'
  | 'preset.close.used'
  | 'preset.close.metric'
  | 'preset.wait.person'
  | 'preset.wait.agent'
  | 'preset.wait.review'
  | 'preset.wait.evidence'
  | 'preset.date.tomorrow'
  | 'preset.date.days3'
  | 'preset.date.week1'
  | 'preset.date.weeks2'
  | 'preset.date.month1'
  | 'preset.wake.week'
  | 'preset.wake.month'
  | 'preset.wake.related'
  | 'preset.wake.repeat'
  | 'preset.wake.evidence'
  | 'preset.reason.value'
  | 'preset.reason.timing'
  | 'preset.reason.disproved'
  | 'preset.reason.better'
  | 'preset.result.accepted'
  | 'preset.result.source'
  | 'preset.result.delivered'
  | 'preset.result.recorded'
  | 'exit.done'
  | 'exit.stopped'
  | 'exit.transferred'
  | 'exit.compressed'
  | 'via.none'
  | 'via.delegateDone'
  | 'via.article'
  | 'via.delegateTransferred'
  | 'via.drop'
  | 'via.disproved'
  | 'via.ignore'
  | 'via.merge'
  | 'via.project'
  | 'via.buy'
  | 'via.publish'
  | 'via.principle'
  | 'via.automate'
  | 'relink.title'
  | 'relink.helpExact'
  | 'relink.helpManual'
  | 'relink.created'
  | 'relink.createdUnknown'
  | 'relink.noCandidates'
  | 'error.required'
  | 'error.ideaRequired'
  | 'error.taskRequired'
  | 'error.doneWhenRequired'
  | 'error.createTask'
  | 'error.createRefresh'
  | 'error.createRefreshCurrent'
  | 'error.createCurrent'
  | 'error.create'
  | 'error.load'
  | 'error.save'
  | 'error.open'
  | 'status.capture'
  | 'status.wip'
  | 'status.waiting'
  | 'status.dormant'
  | 'status.closed'
  | 'status.unsupported'

type Catalog = Record<MessageKey, string>

const en: Catalog = {
  'app.title': 'Next',
  'app.value': 'Give ideas and tasks a clear next step — or a place to rest.',
  'filter.projects': 'Projects',
  'common.loading': 'Loading…',
  'common.refresh': 'Refresh',
  'common.open': 'Open',
  'common.cancel': 'Cancel',
  'common.save': 'Place item',
  'common.optional': 'Optional',
  'common.error': 'Something went wrong',
  'empty.wip': 'Nothing is actively committed.',
  'empty.waiting': 'Nothing is waiting for review.',
  'empty.capture': 'No recent items need a decision.',
  'empty.dormant': 'Nothing needs to return to view.',
  'empty.closed': 'Completed items stay out of sight until you show or search them.',
  'empty.search': 'No placed items match.',
  'section.wip': 'In Progress',
  'section.waiting': 'Waiting',
  'section.capture': 'Inbox',
  'section.resurfaced': 'Back in view',
  'section.placed': 'Placed items',
  'section.repair': 'Needs attention',
  'count.wip': '{count}/{limit}',
  'count.waiting': '{count}',
  'action.placeOne': 'Place an item',
  'action.newTask': 'New Task',
  'action.newIdea': 'New Idea',
  'action.settings': 'Settings',
  'action.hideCapture': 'Hide items',
  'action.findPlaced': 'Show placed items',
  'action.hidePlaced': 'Hide placed items',
  'action.place': 'Place',
  'action.reopen': 'Reopen',
  'action.relink': 'Relink',
  'action.openSource': 'Open source',
  'badge.proofed': 'Proofed',
  'badge.task': 'Task',
  'badge.agent': 'Agent',
  'badge.orphan': 'Source missing',
  'badge.unsupported': 'Needs repair',
  'badge.dueNone': 'No due date',
  'badge.due': 'Due {date}',
  'badge.overdue': 'Overdue {date}',
  'badge.contextMissing': 'Context not clarified',
  'metadata.label': 'Priority, due date, and contexts',
  'priority.P0': 'P0 · Urgent',
  'priority.P1': 'P1 · High',
  'priority.P2': 'P2 · Normal',
  'priority.P3': 'P3 · Low',
  'warning.wip': 'WIP limit reached: {count} items are in progress (limit {limit}). Finish, wait, or move at least one to Later. This warning stays visible until In Progress is below the limit.',
  'warning.waiting': 'More than five items are waiting. Make sure each still has a real review responsibility.',
  'warning.readOnly': 'Next is read-only until its event document is repaired.',
  'search.placeholder': 'Search title, action, or destination',
  'board.dragHelp': 'Drag a card to another lane. Next asks only for the information that state needs.',
  'create.title': 'New Idea',
  'create.destination': 'Save to {path}',
  'create.field': 'Idea',
  'create.placeholder': 'Write down the thought before it disappears…',
  'create.save': 'Create Idea',
  'create.saveShortcut': '⌘↵ / Ctrl↵ to save',
  'task.create.title': 'New Task',
  'task.create.destination': 'Save to {path}',
  'task.create.field': 'Task',
  'task.create.placeholder': 'What needs to be done?',
  'task.create.body': 'Context',
  'task.create.bodyPlaceholder': 'The information needed to resume this task',
  'task.create.doneWhen': 'Done when',
  'task.create.doneWhenPlaceholder': 'What observable result means this is complete?',
  'task.create.saveInbox': 'Add to Inbox',
  'task.create.saveCurrent': 'Save and mark current',
  'task.create.saveShortcut': '⌘↵ / Ctrl↵ adds to Inbox',
  'sheet.title': 'Place “{title}”',
  'sheet.choose': 'What happens next?',
  'route.commit': 'Move forward now',
  'route.commit.detail': 'Take responsibility for a concrete next step.',
  'route.wait': 'Wait to review',
  'route.wait.detail': 'An external result still needs your acceptance.',
  'route.park': 'Look again later',
  'route.park.detail': 'Keep the memory without a current commitment.',
  'route.settle': 'End or move elsewhere',
  'route.settle.detail': 'Finish, stop, transfer, or turn it into a stable system.',
  'field.commitment': 'Commitment',
  'field.commitment.placeholder': 'What will you verify or deliver?',
  'field.nextAction': 'Next action',
  'field.nextAction.placeholder': 'A step you can start without thinking again',
  'field.closeCondition': 'Close condition',
  'field.closeCondition.placeholder': 'What result or evidence lets you stop?',
  'field.waitingFor': 'Waiting for',
  'field.waitingFor.placeholder': 'Person, result, evidence, or purchase trial',
  'field.reviewAt': 'Review on',
  'field.wakeTrigger': 'Wake trigger',
  'field.wakeTrigger.placeholder': 'A date or a concrete situation',
  'field.wakeTrigger.help': 'Dates resurface automatically. Situations stay searchable; Next does not detect them.',
  'field.exitKind': 'Outcome',
  'field.exitKind.placeholder': 'Choose an outcome',
  'field.exitVia': 'How',
  'field.exitVia.placeholder': 'Choose how',
  'field.reason': 'Reason',
  'field.reason.placeholder': 'Why is stopping the right decision?',
  'field.target': 'Destination',
  'field.target.placeholder': 'Project, person, product, link, rule, or automation',
  'field.result': 'Result',
  'field.result.placeholder': 'Optional result or evidence link',
  'field.project': 'Project',
  'field.priority': 'Priority',
  'field.due': 'Due date · Optional',
  'field.contexts': 'GTD context · Optional',
  'field.contexts.placeholder': '@computer, @phone (comma-separated)',
  'settings.title': 'Next Settings',
  'settings.description': 'These settings apply only to Next and to cards you create from now on.',
  'settings.wipLimit': 'In Progress limit',
  'settings.wipLimit.help': 'Show a persistent warning when In Progress reaches this number.',
  'settings.defaultPriority': 'Default priority',
  'settings.defaultPriority.help': 'Prefill the priority for new Ideas and Tasks.',
  'settings.defaultDueDays': 'Default due date',
  'settings.defaultDueDays.help': 'Days after creation; use 0 to leave new cards without a due date.',
  'settings.defaultContext': 'Default GTD context',
  'settings.defaultContext.help': 'Optional context prefilled for new cards, such as @computer or @phone.',
  'settings.validation': 'Enter whole numbers: the In Progress limit must be at least 1, and due days cannot be negative.',
  'settings.save': 'Save Settings',
  'settings.saving': 'Saving…',
  'settings.saveError': 'Next could not save these settings. Your edits are still here.',
  'field.project.placeholder': 'Choose or enter a project',
  'field.project.selected': 'Selected projects',
  'field.project.existing': 'Existing projects',
  'field.project.add': 'Add',
  'field.project.help': 'Select more than one, or create a new tag.',
  'field.project.target': 'Upgrade to which project?',
  'field.project.target.placeholder': 'Choose one project',
  'project.suggestion': 'Suggested · {project}',
  'project.suggestion.detail': 'Local match: {terms}. Click to confirm before placing.',
  'field.article': 'Published article',
  'field.article.placeholder': 'Article path or public URL',
  'preset.commit.verify': 'Verify whether “{title}” is worth continuing',
  'preset.commit.prototype': 'Build a usable prototype of “{title}”',
  'preset.commit.plan': 'Turn “{title}” into a reviewable plan',
  'preset.commit.deliver': 'Deliver the smallest useful result for “{title}”',
  'preset.next.evidence': 'Collect three key pieces of evidence',
  'preset.next.experiment': 'Run one minimal experiment',
  'preset.next.draft': 'Write the first version',
  'preset.next.user': 'Ask one real user to review it',
  'preset.close.decision': 'Reach a clear continue-or-stop decision',
  'preset.close.prototype': 'Complete a usable prototype',
  'preset.close.used': 'One person uses it and gives feedback',
  'preset.close.metric': 'Meet the chosen validation measure',
  'preset.wait.person': 'A person’s reply about “{title}”',
  'preset.wait.agent': 'An agent’s result for “{title}”',
  'preset.wait.review': 'Review or acceptance feedback on “{title}”',
  'preset.wait.evidence': 'Key evidence about “{title}”',
  'preset.date.tomorrow': 'Tomorrow',
  'preset.date.days3': 'In 3 days',
  'preset.date.week1': 'In 1 week',
  'preset.date.weeks2': 'In 2 weeks',
  'preset.date.month1': 'In 1 month',
  'preset.wake.week': 'Next week',
  'preset.wake.month': 'Next month',
  'preset.wake.related': 'When a related project starts',
  'preset.wake.repeat': 'When the same problem appears again',
  'preset.wake.evidence': 'When key evidence appears',
  'preset.reason.value': 'Not valuable enough',
  'preset.reason.timing': 'The timing is wrong',
  'preset.reason.disproved': 'The core assumption was disproved',
  'preset.reason.better': 'A better solution already exists',
  'preset.result.accepted': 'Completed and accepted',
  'preset.result.source': 'The source note is the result',
  'preset.result.delivered': 'Delivered',
  'preset.result.recorded': 'Evidence has been recorded',
  'exit.done': 'Done',
  'exit.stopped': 'Stopped',
  'exit.transferred': 'Moved elsewhere',
  'exit.compressed': 'Turned into a system',
  'via.none': 'Completed directly',
  'via.delegateDone': 'Delegated and accepted',
  'via.article': 'Organized and published as a complete article',
  'via.delegateTransferred': 'Responsibility transferred',
  'via.drop': 'Dropped',
  'via.disproved': 'Disproved',
  'via.ignore': 'Ignored',
  'via.merge': 'Merged',
  'via.project': 'Moved to project',
  'via.buy': 'Bought an existing solution',
  'via.publish': 'Published source material for others to continue',
  'via.principle': 'Turned into a principle',
  'via.automate': 'Automated',
  'relink.title': 'Relink source',
  'relink.helpExact': 'These files have the same creation time. Confirm the correct source; Next will not rename or edit it.',
  'relink.helpManual': 'No creation-time match was found. These are unclaimed idea files; check the path and time yourself.',
  'relink.created': 'Created',
  'relink.createdUnknown': 'unknown',
  'relink.noCandidates': 'No unclaimed idea files were found.',
  'error.required': 'Complete the required fields.',
  'error.ideaRequired': 'Write down the idea before saving.',
  'error.taskRequired': 'Name the task before saving.',
  'error.doneWhenRequired': 'Add a done condition before marking this task current.',
  'error.createTask': 'Next could not create this Task.',
  'error.createRefresh': 'The Task was saved, but Next could not refresh the Inbox.',
  'error.createRefreshCurrent': 'The Task was saved, but Inbox refresh failed, so it was not marked current.',
  'error.createCurrent': 'The Task is safe in Inbox, but could not be marked current.',
  'error.create': 'Next could not create this Idea.',
  'error.load': 'Next could not load your items.',
  'error.save': 'Next could not save this decision.',
  'error.open': 'The source file could not be opened.',
  'status.capture': 'Inbox',
  'status.wip': 'In Progress',
  'status.waiting': 'Waiting',
  'status.dormant': 'Someday',
  'status.closed': 'Done',
  'status.unsupported': 'Unsupported',
}

const zh: Catalog = {
  'app.title': '下一步',
  'app.value': '让想法和任务都有明确的下一步，或一个安心的去处。',
  'filter.projects': '项目',
  'common.loading': '正在载入…',
  'common.refresh': '刷新',
  'common.open': '打开',
  'common.cancel': '取消',
  'common.save': '确认安放',
  'common.optional': '可选',
  'common.error': '出现了问题',
  'empty.wip': '手上没有正在承诺的事项。',
  'empty.waiting': '没有等待回收的结果。',
  'empty.capture': '最近没有需要安放的事项。',
  'empty.dormant': '没有需要再次浮现的事项。',
  'empty.closed': '已完成事项默认不常亮；显示或搜索时才会出现。',
  'empty.search': '没有匹配的已安放事项。',
  'section.wip': '进行中',
  'section.waiting': '等待',
  'section.capture': '收件箱',
  'section.resurfaced': '再次浮现',
  'section.placed': '已安放',
  'section.repair': '需要处理',
  'count.wip': '{count}/{limit}',
  'count.waiting': '{count}',
  'action.placeOne': '安放一个事项',
  'action.newTask': '新建任务',
  'action.newIdea': '新建 Idea',
  'action.settings': '设置',
  'action.hideCapture': '收起想法',
  'action.findPlaced': '显示已安放',
  'action.hidePlaced': '收起已安放',
  'action.place': '安放',
  'action.reopen': '重新考虑',
  'action.relink': '重新关联',
  'action.openSource': '打开原文',
  'badge.proofed': '已有论证',
  'badge.task': '任务',
  'badge.agent': 'Agent 添加',
  'badge.orphan': '原文失联',
  'badge.unsupported': '需要修复',
  'badge.dueNone': '无截止日期',
  'badge.due': '截止 {date}',
  'badge.overdue': '已逾期 {date}',
  'badge.contextMissing': '情境未明确',
  'metadata.label': '优先级、截止日期与情境',
  'priority.P0': 'P0 · 紧急',
  'priority.P1': 'P1 · 高',
  'priority.P2': 'P2 · 普通',
  'priority.P3': 'P3 · 低',
  'warning.wip': '已达到 WIP 上限：进行中有 {count} 项（上限 {limit}）。请先完成、转为等待或移到稍后至少一项；在数量降到上限以下前，此警告会一直显示。',
  'warning.waiting': '等待项已超过五个。请确认每一项仍有真实的回收责任。',
  'warning.readOnly': '事件文档修复前，Next 将保持只读。',
  'search.placeholder': '搜索标题、下一步或去向',
  'board.dragHelp': '把卡片拖到另一条泳道；Next 只会询问该状态真正需要的信息。',
  'create.title': '新建 Idea',
  'create.destination': '保存到 {path}',
  'create.field': 'Idea',
  'create.placeholder': '趁念头还在，把它写下来…',
  'create.save': '创建 Idea',
  'create.saveShortcut': '⌘↵ / Ctrl↵ 保存',
  'task.create.title': '新建任务',
  'task.create.destination': '保存到 {path}',
  'task.create.field': '任务',
  'task.create.placeholder': '现在需要完成什么？',
  'task.create.body': '补充说明',
  'task.create.bodyPlaceholder': '重新开始这项任务时需要的上下文',
  'task.create.doneWhen': '完成条件',
  'task.create.doneWhenPlaceholder': '出现什么可观察结果就算完成？',
  'task.create.saveInbox': '加入收件箱',
  'task.create.saveCurrent': '保存并标记为当前',
  'task.create.saveShortcut': '⌘↵ / Ctrl↵ 加入收件箱',
  'sheet.title': '安放“{title}”',
  'sheet.choose': '接下来怎么办？',
  'route.commit': '现在推进',
  'route.commit.detail': '承担一个具体下一步和关闭条件。',
  'route.wait': '等待回收',
  'route.wait.detail': '外部结果仍需要你检查或验收。',
  'route.park': '以后再看',
  'route.park.detail': '保留记忆，解除当前承诺。',
  'route.settle': '结束或已有去处',
  'route.settle.detail': '完成、停止、转移，或变成稳定机制。',
  'field.commitment': '承诺',
  'field.commitment.placeholder': '这次具体要验证或交付什么？',
  'field.nextAction': '下一步',
  'field.nextAction.placeholder': '重新打开时可以直接执行的动作',
  'field.closeCondition': '关闭条件',
  'field.closeCondition.placeholder': '出现什么结果或证据就可以停止？',
  'field.waitingFor': '等待什么',
  'field.waitingFor.placeholder': '人、结果、证据或购买试用',
  'field.reviewAt': '回收时间',
  'field.wakeTrigger': '唤醒条件',
  'field.wakeTrigger.placeholder': '一个日期或具体情境',
  'field.wakeTrigger.help': '日期到达后会自动浮现；情境只保留供搜索，Next 不会自动识别。',
  'field.exitKind': '结果',
  'field.exitKind.placeholder': '选择结果',
  'field.exitVia': '方式',
  'field.exitVia.placeholder': '选择方式',
  'field.reason': '理由',
  'field.reason.placeholder': '为什么现在停止是合理的？',
  'field.target': '去向',
  'field.target.placeholder': '项目、人员、产品、链接、规则或自动化',
  'field.result': '结果',
  'field.result.placeholder': '可选的结果或证据链接',
  'field.project': '项目',
  'field.priority': '优先级',
  'field.due': '截止日期 · 可选',
  'field.contexts': 'GTD 情境 · 可选',
  'field.contexts.placeholder': '@电脑、@电话（逗号分隔）',
  'settings.title': '下一步设置',
  'settings.description': '这些设置只作用于「下一步」插件，以及今后新建的卡片。',
  'settings.wipLimit': '进行中上限',
  'settings.wipLimit.help': '进行中数量达到此值后，持续显示醒目警告。',
  'settings.defaultPriority': '默认优先级',
  'settings.defaultPriority.help': '为新建 Idea 和任务预填优先级。',
  'settings.defaultDueDays': '默认截止天数',
  'settings.defaultDueDays.help': '从创建当天起计算；填 0 表示新卡片默认没有截止日期。',
  'settings.defaultContext': '默认 GTD 情境',
  'settings.defaultContext.help': '可选，为新卡片预填执行情境，例如 @电脑 或 @电话。',
  'settings.validation': '请输入整数：进行中上限至少为 1，默认截止天数不能小于 0。',
  'settings.save': '保存设置',
  'settings.saving': '正在保存…',
  'settings.saveError': '无法保存这些设置；你的修改仍保留在页面中。',
  'field.project.placeholder': '选择已有项目，或输入项目名称',
  'field.project.selected': '已选项目',
  'field.project.existing': '已有项目',
  'field.project.add': '添加',
  'field.project.help': '可多选；也可以创建一个新的项目 Tag。',
  'field.project.target': '升级到哪个项目？',
  'field.project.target.placeholder': '选择一个项目',
  'project.suggestion': '建议 · {project}',
  'project.suggestion.detail': '本地匹配：{terms}。点击确认后再安放。',
  'field.article': '完整文章',
  'field.article.placeholder': '文章路径或公开链接',
  'preset.commit.verify': '验证“{title}”是否值得继续',
  'preset.commit.prototype': '为“{title}”做一个可用原型',
  'preset.commit.plan': '把“{title}”写成可评审方案',
  'preset.commit.deliver': '交付“{title}”的最小可用结果',
  'preset.next.evidence': '收集三个关键证据',
  'preset.next.experiment': '做一个最小实验',
  'preset.next.draft': '写出第一版',
  'preset.next.user': '找一位真实使用者确认',
  'preset.close.decision': '得到明确继续或停止结论',
  'preset.close.prototype': '完成一个可用原型',
  'preset.close.used': '有人实际使用并给出反馈',
  'preset.close.metric': '达到预设验证指标',
  'preset.wait.person': '关于“{title}”的他人回复',
  'preset.wait.agent': '关于“{title}”的 Agent 结果',
  'preset.wait.review': '关于“{title}”的评审或验收反馈',
  'preset.wait.evidence': '关于“{title}”的关键证据',
  'preset.date.tomorrow': '明天',
  'preset.date.days3': '3 天后',
  'preset.date.week1': '1 周后',
  'preset.date.weeks2': '2 周后',
  'preset.date.month1': '1 个月后',
  'preset.wake.week': '下周',
  'preset.wake.month': '下个月',
  'preset.wake.related': '相关项目启动时',
  'preset.wake.repeat': '再次遇到同类问题时',
  'preset.wake.evidence': '获得关键证据时',
  'preset.reason.value': '价值不足',
  'preset.reason.timing': '时机不对',
  'preset.reason.disproved': '核心假设已被否定',
  'preset.reason.better': '已有更好的方案',
  'preset.result.accepted': '已完成并验收',
  'preset.result.source': '原文即结果',
  'preset.result.delivered': '已交付',
  'preset.result.recorded': '已有证据记录',
  'exit.done': '完成',
  'exit.stopped': '停止',
  'exit.transferred': '转到别处',
  'exit.compressed': '变成机制',
  'via.none': '直接完成',
  'via.delegateDone': '委托并已验收',
  'via.article': '整理并发布为完整文章',
  'via.delegateTransferred': '责任已移交',
  'via.drop': '放弃',
  'via.disproved': '证伪',
  'via.ignore': '忽略',
  'via.merge': '合并',
  'via.project': '升级为项目',
  'via.buy': '购买已有方案',
  'via.publish': '公开素材供他人接力',
  'via.principle': '沉淀为原则',
  'via.automate': '自动化',
  'relink.title': '重新关联原文',
  'relink.helpExact': '这些文件的创建时间相同，请确认正确原文。Next 不会重命名或修改它。',
  'relink.helpManual': '没有找到相同创建时间。这些只是尚未被认领的 idea，请自行核对路径与时间。',
  'relink.created': '创建时间',
  'relink.createdUnknown': '未知',
  'relink.noCandidates': '没有找到尚未被认领的 idea 文件。',
  'error.required': '请补全必填内容。',
  'error.ideaRequired': '请先写下 Idea。',
  'error.taskRequired': '请先写下任务。',
  'error.doneWhenRequired': '标记为当前之前，请填写完成条件。',
  'error.createTask': 'Next 无法创建这个任务。',
  'error.createRefresh': '任务已保存，但 Next 暂时无法刷新收件箱。',
  'error.createRefreshCurrent': '任务已保存，但收件箱刷新失败，尚未标记为当前。',
  'error.createCurrent': '任务已保存在收件箱，但无法标记为当前。',
  'error.create': 'Next 无法创建这个 Idea。',
  'error.load': 'Next 无法载入你的事项。',
  'error.save': 'Next 无法保存这次安放。',
  'error.open': '无法打开原文。',
  'status.capture': '收件箱',
  'status.wip': '进行中',
  'status.waiting': '等待',
  'status.dormant': '稍后',
  'status.closed': '已完成',
  'status.unsupported': '无法识别',
}

const ja: Catalog = {
  'app.title': '次の一歩',
  'app.value': 'アイデアとタスクに、明確な次の一歩か安心できる置き場所を。',
  'filter.projects': 'プロジェクト',
  'common.loading': '読み込み中…',
  'common.refresh': '更新',
  'common.open': '開く',
  'common.cancel': 'キャンセル',
  'common.save': '配置を確定',
  'common.optional': '任意',
  'common.error': '問題が発生しました',
  'empty.wip': '現在引き受けている項目はありません。',
  'empty.waiting': '確認待ちの項目はありません。',
  'empty.capture': '最近、判断が必要な項目はありません。',
  'empty.dormant': '再表示が必要な項目はありません。',
  'empty.closed': '完了した項目は、表示または検索するまで隠れます。',
  'empty.search': '一致する配置済み項目はありません。',
  'section.wip': '進行中',
  'section.waiting': '待機中',
  'section.capture': '受信箱',
  'section.resurfaced': '再び表示',
  'section.placed': '配置済み',
  'section.repair': '確認が必要',
  'count.wip': '{count}/{limit}',
  'count.waiting': '{count}',
  'action.placeOne': '項目を置く',
  'action.newTask': '新規タスク',
  'action.newIdea': '新規 Idea',
  'action.settings': '設定',
  'action.hideCapture': '項目を隠す',
  'action.findPlaced': '配置済みを表示',
  'action.hidePlaced': '配置済みを隠す',
  'action.place': '置く',
  'action.reopen': '再検討',
  'action.relink': '再リンク',
  'action.openSource': '原文を開く',
  'badge.proofed': '検証済み',
  'badge.task': 'タスク',
  'badge.agent': 'Agent が追加',
  'badge.orphan': '原文が見つかりません',
  'badge.unsupported': '修復が必要',
  'badge.dueNone': '期限なし',
  'badge.due': '期限 {date}',
  'badge.overdue': '期限超過 {date}',
  'badge.contextMissing': 'コンテキスト未整理',
  'metadata.label': '優先度、期限、コンテキスト',
  'priority.P0': 'P0 · 緊急',
  'priority.P1': 'P1 · 高',
  'priority.P2': 'P2 · 通常',
  'priority.P3': 'P3 · 低',
  'warning.wip': 'WIP 上限に達しました：{count} 件が進行中です（上限 {limit}）。少なくとも一件を完了、待機、または後回しにしてください。進行中の件数が上限未満になるまで、この警告は表示され続けます。',
  'warning.waiting': '確認待ちが五つを超えています。すべてに確認責任が残っているか確かめてください。',
  'warning.readOnly': 'イベント文書を修復するまで Next は読み取り専用です。',
  'search.placeholder': 'タイトル、次の行動、移動先を検索',
  'board.dragHelp': 'カードを別のレーンへドラッグできます。Next はその状態に必要な情報だけを尋ねます。',
  'create.title': '新規 Idea',
  'create.destination': '{path} に保存',
  'create.field': 'Idea',
  'create.placeholder': '消える前に思いつきを書き留める…',
  'create.save': 'Idea を作成',
  'create.saveShortcut': '⌘↵ / Ctrl↵ で保存',
  'task.create.title': '新規タスク',
  'task.create.destination': '{path} に保存',
  'task.create.field': 'タスク',
  'task.create.placeholder': '何を完了する必要がありますか？',
  'task.create.body': '補足',
  'task.create.bodyPlaceholder': 'このタスクを再開するために必要な情報',
  'task.create.doneWhen': '完了条件',
  'task.create.doneWhenPlaceholder': 'どんな確認可能な結果で完了としますか？',
  'task.create.saveInbox': '受信箱に追加',
  'task.create.saveCurrent': '保存して進行中にする',
  'task.create.saveShortcut': '⌘↵ / Ctrl↵ で受信箱に追加',
  'sheet.title': '「{title}」の置き場所',
  'sheet.choose': '次はどうしますか？',
  'route.commit': '今進める',
  'route.commit.detail': '具体的な次の行動と終了条件を引き受けます。',
  'route.wait': '確認を待つ',
  'route.wait.detail': '外部の結果をまだ確認する必要があります。',
  'route.park': 'あとで見る',
  'route.park.detail': '記憶を残し、今の約束から外します。',
  'route.settle': '終える、または移す',
  'route.settle.detail': '完了、中止、移管、仕組み化を選びます。',
  'field.commitment': '約束',
  'field.commitment.placeholder': '何を検証、または届けますか？',
  'field.nextAction': '次の行動',
  'field.nextAction.placeholder': '考え直さず始められる具体的な一歩',
  'field.closeCondition': '終了条件',
  'field.closeCondition.placeholder': 'どんな結果や証拠があれば止められますか？',
  'field.waitingFor': '待っているもの',
  'field.waitingFor.placeholder': '人、結果、証拠、購入した製品の試用',
  'field.reviewAt': '確認日',
  'field.wakeTrigger': '再開条件',
  'field.wakeTrigger.placeholder': '日付、または具体的な状況',
  'field.wakeTrigger.help': '日付は自動で再表示されます。状況は検索用に残りますが、Next は自動検出しません。',
  'field.exitKind': '結果',
  'field.exitKind.placeholder': '結果を選択',
  'field.exitVia': '方法',
  'field.exitVia.placeholder': '方法を選択',
  'field.reason': '理由',
  'field.reason.placeholder': '今やめるのが適切なのはなぜですか？',
  'field.target': '移動先',
  'field.target.placeholder': 'プロジェクト、人、製品、リンク、原則、自動化',
  'field.result': '成果',
  'field.result.placeholder': '任意の成果または証拠へのリンク',
  'field.project': 'プロジェクト',
  'field.priority': '優先度',
  'field.due': '期限 · 任意',
  'field.contexts': 'GTD コンテキスト · 任意',
  'field.contexts.placeholder': '@computer, @phone（カンマ区切り）',
  'settings.title': '次の一歩の設定',
  'settings.description': 'これらの設定は「次の一歩」と、今後作成するカードだけに適用されます。',
  'settings.wipLimit': '進行中の上限',
  'settings.wipLimit.help': '進行中がこの件数に達すると、目立つ警告を常に表示します。',
  'settings.defaultPriority': '既定の優先度',
  'settings.defaultPriority.help': '新しいアイデアとタスクの優先度を事前入力します。',
  'settings.defaultDueDays': '既定の期限日数',
  'settings.defaultDueDays.help': '作成日からの日数です。0 にすると期限を設定しません。',
  'settings.defaultContext': '既定の GTD コンテキスト',
  'settings.defaultContext.help': '任意。@パソコンや @電話など、新しいカードに事前入力します。',
  'settings.validation': '整数を入力してください。進行中の上限は 1 以上、期限日数は 0 以上です。',
  'settings.save': '設定を保存',
  'settings.saving': '保存中…',
  'settings.saveError': '設定を保存できませんでした。編集内容はこのページに残っています。',
  'field.project.placeholder': '既存のプロジェクトを選択するか入力',
  'field.project.selected': '選択済みのプロジェクト',
  'field.project.existing': '既存のプロジェクト',
  'field.project.add': '追加',
  'field.project.help': '複数選択、または新しいプロジェクトタグを作成できます。',
  'field.project.target': 'どのプロジェクトに昇格しますか？',
  'field.project.target.placeholder': 'プロジェクトを選択',
  'project.suggestion': '候補 · {project}',
  'project.suggestion.detail': 'ローカル一致：{terms}。クリックして確認後に配置します。',
  'field.article': '公開済み記事',
  'field.article.placeholder': '記事のパスまたは公開 URL',
  'preset.commit.verify': '「{title}」を続ける価値があるか検証する',
  'preset.commit.prototype': '「{title}」の使える試作を作る',
  'preset.commit.plan': '「{title}」をレビュー可能な案にする',
  'preset.commit.deliver': '「{title}」の最小限の成果を届ける',
  'preset.next.evidence': '重要な証拠を三つ集める',
  'preset.next.experiment': '最小限の実験を一つ行う',
  'preset.next.draft': '初版を書く',
  'preset.next.user': '実際の利用者一人に確認する',
  'preset.close.decision': '続行か中止かを明確に決める',
  'preset.close.prototype': '使える試作を完成する',
  'preset.close.used': '一人が実際に使い、感想を返す',
  'preset.close.metric': '決めた検証指標を満たす',
  'preset.wait.person': '「{title}」についての相手からの返事',
  'preset.wait.agent': '「{title}」についての Agent の結果',
  'preset.wait.review': '「{title}」についてのレビューまたは検収結果',
  'preset.wait.evidence': '「{title}」についての重要な証拠',
  'preset.date.tomorrow': '明日',
  'preset.date.days3': '3日後',
  'preset.date.week1': '1週間後',
  'preset.date.weeks2': '2週間後',
  'preset.date.month1': '1か月後',
  'preset.wake.week': '来週',
  'preset.wake.month': '来月',
  'preset.wake.related': '関連プロジェクトが始まった時',
  'preset.wake.repeat': '同じ問題が再び起きた時',
  'preset.wake.evidence': '重要な証拠が得られた時',
  'preset.reason.value': '価値が十分ではない',
  'preset.reason.timing': '今は時期が違う',
  'preset.reason.disproved': '中心となる仮説が否定された',
  'preset.reason.better': 'より良い解決策がすでにある',
  'preset.result.accepted': '完了して検収済み',
  'preset.result.source': '原文が成果',
  'preset.result.delivered': '納品済み',
  'preset.result.recorded': '証拠を記録済み',
  'exit.done': '完了',
  'exit.stopped': '中止',
  'exit.transferred': '別の場所へ移動',
  'exit.compressed': '仕組みに変換',
  'via.none': '直接完了',
  'via.delegateDone': '委託して検収済み',
  'via.article': '完全な記事にまとめて公開',
  'via.delegateTransferred': '責任を移管',
  'via.drop': '取り下げ',
  'via.disproved': '反証',
  'via.ignore': '無視',
  'via.merge': '統合',
  'via.project': 'プロジェクトへ移動',
  'via.buy': '既存製品を購入',
  'via.publish': '引き継ぎ用の素材を公開',
  'via.principle': '原則に変換',
  'via.automate': '自動化',
  'relink.title': '原文を再リンク',
  'relink.helpExact': '作成時刻が同じファイルです。正しい原文か確認してください。Next は名前も内容も変更しません。',
  'relink.helpManual': '同じ作成時刻の候補がありません。未使用の idea ファイルなので、パスと時刻を自分で確認してください。',
  'relink.created': '作成時刻',
  'relink.createdUnknown': '不明',
  'relink.noCandidates': '未使用の idea ファイルがありません。',
  'error.required': '必須項目を入力してください。',
  'error.ideaRequired': '保存する前に Idea を入力してください。',
  'error.taskRequired': '保存する前にタスクを入力してください。',
  'error.doneWhenRequired': '進行中にする前に完了条件を入力してください。',
  'error.createTask': 'Next はこのタスクを作成できませんでした。',
  'error.createRefresh': 'タスクは保存されましたが、Next は受信箱を更新できませんでした。',
  'error.createRefreshCurrent': 'タスクは保存されましたが、受信箱の更新に失敗したため進行中にはなっていません。',
  'error.createCurrent': 'タスクは受信箱に保存されましたが、進行中にできませんでした。',
  'error.create': 'Next はこの Idea を作成できませんでした。',
  'error.load': 'Next は項目を読み込めませんでした。',
  'error.save': 'Next はこの判断を保存できませんでした。',
  'error.open': '原文を開けませんでした。',
  'status.capture': '受信箱',
  'status.wip': '進行中',
  'status.waiting': '待機中',
  'status.dormant': 'いつか',
  'status.closed': '完了',
  'status.unsupported': '未対応',
}

const de: Catalog = {
  'app.title': 'Nächster Schritt',
  'app.value': 'Gib Ideen und Aufgaben einen klaren nächsten Schritt – oder einen ruhigen Ort.',
  'filter.projects': 'Projekte',
  'common.loading': 'Wird geladen…',
  'common.refresh': 'Aktualisieren',
  'common.open': 'Öffnen',
  'common.cancel': 'Abbrechen',
  'common.save': 'Ablage bestätigen',
  'common.optional': 'Optional',
  'common.error': 'Ein Fehler ist aufgetreten',
  'empty.wip': 'Kein Eintrag ist derzeit verbindlich aktiv.',
  'empty.waiting': 'Nichts wartet auf eine Prüfung.',
  'empty.capture': 'Kein neuer Eintrag braucht eine Entscheidung.',
  'empty.dormant': 'Kein Eintrag muss wieder in den Blick kommen.',
  'empty.closed': 'Erledigte Einträge bleiben verborgen, bis du sie anzeigst oder suchst.',
  'empty.search': 'Kein passender abgelegter Eintrag.',
  'section.wip': 'In Arbeit',
  'section.waiting': 'Warten',
  'section.capture': 'Eingang',
  'section.resurfaced': 'Wieder im Blick',
  'section.placed': 'Abgelegte Einträge',
  'section.repair': 'Prüfung nötig',
  'count.wip': '{count}/{limit}',
  'count.waiting': '{count}',
  'action.placeOne': 'Einen Eintrag ablegen',
  'action.newTask': 'Neue Aufgabe',
  'action.newIdea': 'Neue Idee',
  'action.settings': 'Einstellungen',
  'action.hideCapture': 'Einträge ausblenden',
  'action.findPlaced': 'Abgelegte Einträge anzeigen',
  'action.hidePlaced': 'Abgelegte Einträge ausblenden',
  'action.place': 'Ablegen',
  'action.reopen': 'Neu prüfen',
  'action.relink': 'Neu verknüpfen',
  'action.openSource': 'Quelle öffnen',
  'badge.proofed': 'Geprüft',
  'badge.task': 'Aufgabe',
  'badge.agent': 'Von Agent hinzugefügt',
  'badge.orphan': 'Quelle fehlt',
  'badge.unsupported': 'Reparatur nötig',
  'badge.dueNone': 'Kein Fälligkeitsdatum',
  'badge.due': 'Fällig {date}',
  'badge.overdue': 'Überfällig {date}',
  'badge.contextMissing': 'Kontext ungeklärt',
  'metadata.label': 'Priorität, Fälligkeit und Kontexte',
  'priority.P0': 'P0 · Dringend',
  'priority.P1': 'P1 · Hoch',
  'priority.P2': 'P2 · Normal',
  'priority.P3': 'P3 · Niedrig',
  'warning.wip': 'WIP-Limit erreicht: {count} Einträge sind in Bearbeitung (Limit {limit}). Schließe mindestens einen ab, stelle ihn auf Warten oder verschiebe ihn auf Später. Diese Warnung bleibt sichtbar, bis die Anzahl unter dem Limit liegt.',
  'warning.waiting': 'Mehr als fünf Punkte warten. Prüfe, ob für jeden noch echte Abnahmeverantwortung besteht.',
  'warning.readOnly': 'Next bleibt schreibgeschützt, bis das Ereignisdokument repariert ist.',
  'search.placeholder': 'Titel, nächsten Schritt oder Ziel suchen',
  'board.dragHelp': 'Ziehe eine Karte in eine andere Bahn. Next fragt nur nach den Angaben, die dieser Zustand braucht.',
  'create.title': 'Neue Idee',
  'create.destination': 'Speichern unter {path}',
  'create.field': 'Idee',
  'create.placeholder': 'Halte den Gedanken fest, bevor er verschwindet…',
  'create.save': 'Idee erstellen',
  'create.saveShortcut': '⌘↵ / Ctrl↵ zum Speichern',
  'task.create.title': 'Neue Aufgabe',
  'task.create.destination': 'Speichern unter {path}',
  'task.create.field': 'Aufgabe',
  'task.create.placeholder': 'Was muss erledigt werden?',
  'task.create.body': 'Kontext',
  'task.create.bodyPlaceholder': 'Informationen, die du zum Fortsetzen brauchst',
  'task.create.doneWhen': 'Erledigt, wenn',
  'task.create.doneWhenPlaceholder': 'Welches beobachtbare Ergebnis bedeutet fertig?',
  'task.create.saveInbox': 'Zum Eingang hinzufügen',
  'task.create.saveCurrent': 'Speichern und als aktuell markieren',
  'task.create.saveShortcut': '⌘↵ / Ctrl↵ fügt zum Eingang hinzu',
  'sheet.title': '„{title}“ ablegen',
  'sheet.choose': 'Was geschieht als Nächstes?',
  'route.commit': 'Jetzt weiterführen',
  'route.commit.detail': 'Verantwortung für einen konkreten nächsten Schritt übernehmen.',
  'route.wait': 'Auf Prüfung warten',
  'route.wait.detail': 'Ein externes Ergebnis muss noch abgenommen werden.',
  'route.park': 'Später ansehen',
  'route.park.detail': 'Die Erinnerung behalten, ohne aktuelle Verpflichtung.',
  'route.settle': 'Beenden oder weitergeben',
  'route.settle.detail': 'Abschließen, stoppen, übertragen oder in ein System überführen.',
  'field.commitment': 'Verpflichtung',
  'field.commitment.placeholder': 'Was wirst du prüfen oder liefern?',
  'field.nextAction': 'Nächster Schritt',
  'field.nextAction.placeholder': 'Ein Schritt, der ohne erneutes Nachdenken beginnt',
  'field.closeCondition': 'Abschlussbedingung',
  'field.closeCondition.placeholder': 'Welches Ergebnis oder Indiz erlaubt den Abschluss?',
  'field.waitingFor': 'Warten auf',
  'field.waitingFor.placeholder': 'Person, Ergebnis, Nachweis oder Produkttest',
  'field.reviewAt': 'Prüfen am',
  'field.wakeTrigger': 'Auslöser',
  'field.wakeTrigger.placeholder': 'Ein Datum oder eine konkrete Situation',
  'field.wakeTrigger.help': 'Daten erscheinen automatisch wieder. Situationen bleiben suchbar; Next erkennt sie nicht automatisch.',
  'field.exitKind': 'Ergebnis',
  'field.exitKind.placeholder': 'Ergebnis wählen',
  'field.exitVia': 'Art',
  'field.exitVia.placeholder': 'Art wählen',
  'field.reason': 'Grund',
  'field.reason.placeholder': 'Warum ist das Beenden jetzt richtig?',
  'field.target': 'Ziel',
  'field.target.placeholder': 'Projekt, Person, Produkt, Link, Regel oder Automatisierung',
  'field.result': 'Resultat',
  'field.result.placeholder': 'Optionales Resultat oder Link zum Nachweis',
  'field.project': 'Projekt',
  'field.priority': 'Priorität',
  'field.due': 'Fälligkeitsdatum · Optional',
  'field.contexts': 'GTD-Kontext · Optional',
  'field.contexts.placeholder': '@computer, @phone (kommagetrennt)',
  'settings.title': 'Einstellungen für Nächster Schritt',
  'settings.description': 'Diese Einstellungen gelten nur für Nächster Schritt und künftig erstellte Karten.',
  'settings.wipLimit': 'Limit für laufende Arbeit',
  'settings.wipLimit.help': 'Zeigt dauerhaft eine deutliche Warnung, sobald In Bearbeitung dieses Limit erreicht.',
  'settings.defaultPriority': 'Standardpriorität',
  'settings.defaultPriority.help': 'Füllt die Priorität für neue Ideen und Aufgaben vorab aus.',
  'settings.defaultDueDays': 'Standardfälligkeit',
  'settings.defaultDueDays.help': 'Tage nach der Erstellung; 0 lässt neue Karten ohne Fälligkeitsdatum.',
  'settings.defaultContext': 'Standard-GTD-Kontext',
  'settings.defaultContext.help': 'Optionaler Kontext für neue Karten, zum Beispiel @Computer oder @Telefon.',
  'settings.validation': 'Ganze Zahlen eingeben: Das Limit muss mindestens 1 sein, die Fälligkeitstage mindestens 0.',
  'settings.save': 'Einstellungen speichern',
  'settings.saving': 'Wird gespeichert…',
  'settings.saveError': 'Die Einstellungen konnten nicht gespeichert werden. Ihre Eingaben bleiben erhalten.',
  'field.project.placeholder': 'Projekt auswählen oder eingeben',
  'field.project.selected': 'Ausgewählte Projekte',
  'field.project.existing': 'Vorhandene Projekte',
  'field.project.add': 'Hinzufügen',
  'field.project.help': 'Mehrfachauswahl oder ein neues Projekt-Tag erstellen.',
  'field.project.target': 'In welches Projekt übernehmen?',
  'field.project.target.placeholder': 'Ein Projekt auswählen',
  'project.suggestion': 'Vorschlag · {project}',
  'project.suggestion.detail': 'Lokaler Treffer: {terms}. Vor dem Ablegen bestätigen.',
  'field.article': 'Veröffentlichter Artikel',
  'field.article.placeholder': 'Artikelpfad oder öffentliche URL',
  'preset.commit.verify': 'Prüfen, ob „{title}“ weiterverfolgt werden soll',
  'preset.commit.prototype': 'Einen brauchbaren Prototyp für „{title}“ bauen',
  'preset.commit.plan': '„{title}“ als prüfbaren Plan ausarbeiten',
  'preset.commit.deliver': 'Das kleinste brauchbare Ergebnis für „{title}“ liefern',
  'preset.next.evidence': 'Drei wichtige Belege sammeln',
  'preset.next.experiment': 'Ein minimales Experiment durchführen',
  'preset.next.draft': 'Die erste Version schreiben',
  'preset.next.user': 'Eine echte Nutzerin oder einen Nutzer fragen',
  'preset.close.decision': 'Klar über Fortsetzen oder Stoppen entscheiden',
  'preset.close.prototype': 'Einen brauchbaren Prototyp fertigstellen',
  'preset.close.used': 'Eine Person nutzt es und gibt Rückmeldung',
  'preset.close.metric': 'Das gewählte Prüfkriterium erreichen',
  'preset.wait.person': 'Antwort einer Person zu „{title}“',
  'preset.wait.agent': 'Ergebnis eines Agents zu „{title}“',
  'preset.wait.review': 'Prüf- oder Abnahmefeedback zu „{title}“',
  'preset.wait.evidence': 'Wichtiger Beleg zu „{title}“',
  'preset.date.tomorrow': 'Morgen',
  'preset.date.days3': 'In 3 Tagen',
  'preset.date.week1': 'In 1 Woche',
  'preset.date.weeks2': 'In 2 Wochen',
  'preset.date.month1': 'In 1 Monat',
  'preset.wake.week': 'Nächste Woche',
  'preset.wake.month': 'Nächsten Monat',
  'preset.wake.related': 'Wenn ein verwandtes Projekt beginnt',
  'preset.wake.repeat': 'Wenn dasselbe Problem erneut auftritt',
  'preset.wake.evidence': 'Wenn ein wichtiger Beleg vorliegt',
  'preset.reason.value': 'Nicht wertvoll genug',
  'preset.reason.timing': 'Der Zeitpunkt ist falsch',
  'preset.reason.disproved': 'Die Kernannahme wurde widerlegt',
  'preset.reason.better': 'Es gibt bereits eine bessere Lösung',
  'preset.result.accepted': 'Fertig und abgenommen',
  'preset.result.source': 'Die Quellnotiz ist das Ergebnis',
  'preset.result.delivered': 'Geliefert',
  'preset.result.recorded': 'Nachweis wurde festgehalten',
  'exit.done': 'Erledigt',
  'exit.stopped': 'Gestoppt',
  'exit.transferred': 'Weitergegeben',
  'exit.compressed': 'In ein System überführt',
  'via.none': 'Direkt abgeschlossen',
  'via.delegateDone': 'Delegiert und abgenommen',
  'via.article': 'Als vollständigen Artikel ausgearbeitet und veröffentlicht',
  'via.delegateTransferred': 'Verantwortung übertragen',
  'via.drop': 'Verworfen',
  'via.disproved': 'Widerlegt',
  'via.ignore': 'Ignoriert',
  'via.merge': 'Zusammengeführt',
  'via.project': 'In ein Projekt übertragen',
  'via.buy': 'Bestehende Lösung gekauft',
  'via.publish': 'Material zur Weiterarbeit veröffentlicht',
  'via.principle': 'In einen Grundsatz überführt',
  'via.automate': 'Automatisiert',
  'relink.title': 'Quelle neu verknüpfen',
  'relink.helpExact': 'Diese Dateien haben denselben Erstellungszeitpunkt. Bestätige die richtige Quelle; Next ändert sie nicht.',
  'relink.helpManual': 'Kein gleicher Erstellungszeitpunkt gefunden. Prüfe Pfad und Zeitpunkt dieser unbelegten Ideen selbst.',
  'relink.created': 'Erstellt',
  'relink.createdUnknown': 'unbekannt',
  'relink.noCandidates': 'Keine unbelegten Ideendateien gefunden.',
  'error.required': 'Fülle die erforderlichen Felder aus.',
  'error.ideaRequired': 'Schreibe die Idee vor dem Speichern auf.',
  'error.taskRequired': 'Benenne die Aufgabe vor dem Speichern.',
  'error.doneWhenRequired': 'Ergänze ein Abschlusskriterium, bevor du sie als aktuell markierst.',
  'error.createTask': 'Next konnte diese Aufgabe nicht erstellen.',
  'error.createRefresh': 'Die Aufgabe wurde gespeichert, aber Next konnte den Eingang nicht aktualisieren.',
  'error.createRefreshCurrent': 'Die Aufgabe wurde gespeichert, aber wegen der fehlgeschlagenen Aktualisierung nicht als aktuell markiert.',
  'error.createCurrent': 'Die Aufgabe liegt sicher im Eingang, konnte aber nicht als aktuell markiert werden.',
  'error.create': 'Next konnte diese Idee nicht erstellen.',
  'error.load': 'Next konnte deine Einträge nicht laden.',
  'error.save': 'Next konnte diese Entscheidung nicht speichern.',
  'error.open': 'Die Quelldatei konnte nicht geöffnet werden.',
  'status.capture': 'Eingang',
  'status.wip': 'In Arbeit',
  'status.waiting': 'Warten',
  'status.dormant': 'Irgendwann',
  'status.closed': 'Erledigt',
  'status.unsupported': 'Nicht unterstützt',
}

export const CATALOGS: Record<Locale, Catalog> = { en, zh, ja, de }
export const LOCALES: Locale[] = ['en', 'zh', 'ja', 'de']

let active: Locale = 'en'

function isLocale(value: unknown): value is Locale {
  return value === 'en' || value === 'zh' || value === 'ja' || value === 'de'
}

export function setLocale(code: string | undefined): void {
  const base = code?.split('-')[0]
  active = isLocale(base) ? base : 'en'
}

export function t(key: MessageKey, values: Record<string, string | number> = {}): string {
  const template = CATALOGS[active]?.[key] ?? en[key] ?? key
  return template.replace(/\{(\w+)\}/g, (_, name: string) => String(values[name] ?? `{${name}}`))
}
