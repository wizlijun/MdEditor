// src/lib/strings.ts — self-contained i18n for the ebook-import plugin.
//
// A plugin window can't import the host's i18n store, so this mirrors its
// shape (src/lib/i18n/store.svelte.ts) in miniature: a MessageKey union, one
// catalog per locale, and a `t()` that falls back to English. Language is
// chosen from `notemd.locale` at startup via `setLocale`; see App.svelte.

export type Locale = 'en' | 'zh' | 'ja' | 'de'

export type MessageKey =
  | 'title'
  | 'drop.hint'
  | 'drop.pick'
  | 'ocr.label'
  | 'ocr.onlyPdf'
  | 'ocr.provider.wechat'
  | 'ocr.provider.baidu'
  | 'settings.toggle'
  | 'settings.root'
  | 'settings.wechatUrl'
  | 'settings.baiduKey'
  | 'settings.baiduSecret'
  | 'settings.calibre.found'
  | 'settings.calibre.missing'
  | 'settings.calibre.pick'
  | 'settings.calibre.install'
  | 'settings.save'
  | 'queue.empty'
  | 'status.pending'
  | 'status.running'
  | 'status.done'
  | 'status.failed'
  | 'status.cancelled'
  | 'action.openInEditor'
  | 'action.cancel'
  | 'action.clear'
  | 'action.start'
  | 'action.running'
  | 'action.aiRead'
  | 'action.aiReread'
  | 'library.title'
  | 'library.empty'
  | 'library.noMatch'
  | 'library.search'
  | 'library.refresh'
  | 'library.summaryOn'
  | 'library.unread'
  | 'settings.prompt'
  | 'settings.promptHint'
  | 'err.promptMissing'
  // The agent picker beside the AI-read button. Same keys and wording as every
  // other surface that offers to run something with an agent.
  | 'agentPicker.by'
  | 'agentPicker.model'
  | 'agentPicker.unknown'
  | 'agentPicker.notInstalled'
  | 'agentPicker.broken'
  | 'action.viewSummary'
  | 'ai.queued'
  | 'ai.running'
  | 'ai.failed'
  | 'log.toggle'
  | 'stage.convert'
  | 'stage.extract'
  | 'stage.markdown'
  | 'stage.ocr'
  | 'stage.finalize'
  | 'dialog.ebooksFilter'
  | 'dialog.pickBooks'
  | 'dialog.pickCalibre'
  | 'err.noVault'
  | 'err.calibreMissing'
  | 'err.calibreTimeout'
  | 'err.calibreFailed'
  | 'err.badRoot'
  | 'err.noTitle'
  | 'err.ocrOnlyPdf'
  | 'err.ocrEmpty'
  | 'err.ocrUnreachable'
  | 'err.baiduFailed'
  | 'err.baiduCredentials'
  | 'err.baiduAuth'
  | 'settings.baiduHint'
  | 'ocr.baiduCost'
  | 'err.unsupportedType'
  | 'log.ocrStart'
  | 'log.converting'
  | 'log.pageFailed'
  | 'log.failedPages'
  | 'log.baiduToken'
  | 'log.baiduSubmit'
  | 'log.baiduDownload'
  | 'log.baiduStatus'
  | 'log.baiduStatus.pending'
  | 'log.baiduStatus.running'
  | 'log.baiduStatus.success'
  | 'log.baiduStatus.failed'
  | 'settings.ocrProvider'
  | 'topic.importTitle'
  | 'topic.required'
  | 'topic.manage'
  | 'topic.emptySetup'
  | 'topic.current'
  | 'topic.bookCount'
  | 'topic.agentDesign'
  | 'topic.agentRunning'
  | 'topic.agentScopeInventory'
  | 'topic.agentScopeVault'
  | 'topic.agentProviderMissing'
  | 'topic.agentProviderUnavailable'
  | 'topic.unclassifiedCount'
  | 'topic.chooseForBook'
  | 'topic.choose'
  | 'topic.proposalTitle'
  | 'topic.proposalHint'
  | 'topic.assignmentCount'
  | 'topic.applyProposal'
  | 'topic.filter'
  | 'topic.all'
  | 'topic.unclassified'
  | 'topic.manager.title'
  | 'topic.manager.hint'
  | 'topic.manager.close'
  | 'topic.manager.newTopic'
  | 'topic.manager.sort'
  | 'topic.manager.up'
  | 'topic.manager.down'
  | 'topic.manager.id'
  | 'topic.manager.idLocked'
  | 'topic.manager.label'
  | 'topic.manager.labelPlaceholder'
  | 'topic.manager.description'
  | 'topic.manager.descriptionPlaceholder'
  | 'topic.manager.index'
  | 'topic.manager.indexPlaceholder'
  | 'topic.manager.vocabulary'
  | 'topic.manager.addTerm'
  | 'topic.manager.term'
  | 'topic.manager.termDescription'
  | 'topic.manager.removeTerm'
  | 'topic.manager.migrate'
  | 'topic.manager.chooseOther'
  | 'topic.manager.delete'
  | 'topic.manager.add'
  | 'topic.manager.fix'
  | 'topic.manager.saving'
  | 'topic.validation.required'
  | 'topic.validation.tooFew'
  | 'topic.validation.tooMany'
  | 'topic.validation.invalidId'
  | 'topic.validation.invalidIndex'
  | 'topic.validation.duplicateId'
  | 'topic.validation.duplicateLabel'
  | 'topic.validation.duplicateIndex'
  | 'topic.validation.duplicateTerm'

type Catalog = Record<MessageKey, string>

const en: Catalog = {
  title: 'Ebook Import',
  'drop.hint': 'Drag epub / pdf / docx files here',
  'drop.pick': 'Add files…',
  'ocr.label': 'OCR (scanned PDF)',
  'ocr.onlyPdf': 'OCR only applies to PDF',
  'ocr.provider.wechat': 'WeChat OCR',
  'ocr.provider.baidu': 'Baidu Unlimited-OCR',
  'settings.toggle': 'Settings',
  'settings.root': 'Ebooks root',
  'settings.wechatUrl': 'WeChat OCR URL',
  'settings.baiduKey': 'Baidu API key',
  'settings.baiduHint':
    'Baidu Cloud console → AI → OCR / Document parsing → Applications → create one; its API Key and Secret Key go below. Billed per page, roughly ¥0.1 (CNY) each.',
  'ocr.baiduCost': 'Paid service — roughly ¥0.1 (CNY) per page',
  'settings.baiduSecret': 'Baidu secret key',
  'settings.calibre.found': 'Calibre found: {path} ({version})',
  'settings.calibre.missing': 'Calibre not found',
  'settings.calibre.pick': 'Choose…',
  'settings.calibre.install': 'Install Calibre',
  'settings.save': 'Save',
  'queue.empty': 'Nothing queued yet — add files, choose whether to OCR, then press Start.',
  'status.pending': 'Pending',
  'status.running': 'Running',
  'status.done': 'Done',
  'status.failed': 'Failed',
  'status.cancelled': 'Cancelled',
  'action.openInEditor': 'Open in editor',
  'action.cancel': 'Cancel',
  'action.clear': 'Clear finished',
  'action.start': 'Start import',
  'action.running': 'Importing…',
  'action.aiRead': 'AI read first',
  'action.aiReread': 'Read again',
  'library.title': 'Library',
  'library.empty': 'No books yet — import one and it shows up here.',
  'library.noMatch': 'No book matches that name.',
  'library.search': 'Search by title',
  'library.refresh': 'Refresh',
  'library.summaryOn': 'Digest {date}',
  'library.unread': 'Not read yet',
  'settings.prompt': 'AI reading prompt',
  'settings.promptHint':
    'The prompt is a plain file in your vault. Edit it and every AI read after that follows your version.',
  'err.promptMissing':
    "The prompt file isn't in the vault yet — run one AI read and the template lands there.",
  'agentPicker.by': 'by {name}',
  'agentPicker.model': 'model {model}',
  'agentPicker.unknown': 'harness unknown',
  'agentPicker.notInstalled': 'not installed',
  'agentPicker.broken': 'found, but it will not start',
  'action.viewSummary': 'View digest',
  'ai.queued': 'Waiting for AI…',
  'ai.running': 'AI reading… {elapsed}',
  'ai.failed': 'AI reading failed',
  'log.toggle': 'Log',
  'stage.convert': 'Converting',
  'stage.extract': 'Extracting',
  'stage.markdown': 'Converting to Markdown',
  'stage.ocr': 'Running OCR',
  'stage.finalize': 'Finalizing',
  'dialog.ebooksFilter': 'Ebooks',
  'dialog.pickBooks': 'Choose ebook files',
  'dialog.pickCalibre': 'Choose the ebook-convert binary',
  'err.noVault': 'No vault configured — open or create a vault first.',
  'err.calibreMissing': 'Calibre not found — install it or pick its path in Settings.',
  'err.calibreTimeout': 'Calibre conversion timed out — try a smaller file or check Settings.',
  'err.calibreFailed': 'Calibre conversion failed — see the log for details.',
  'err.badRoot': 'The ebooks root must be a path inside the vault — check Settings.',
  'err.noTitle': "Couldn't determine a title for this book.",
  'err.ocrOnlyPdf': 'OCR only works on PDF files.',
  'err.ocrEmpty': 'OCR produced no text — the scan may be blank or unreadable.',
  'err.ocrUnreachable': "Couldn't reach the OCR service — check your network or Settings.",
  'err.baiduFailed': 'Baidu OCR failed — see the log for details.',
  'err.baiduCredentials':
    "Baidu didn't recognize the credentials. Use an application's API Key and Secret Key from the OCR console — not an Access Key pair — and check the two fields aren't swapped.",
  'err.baiduAuth': 'Baidu sign-in failed — check the API key and secret key in Settings.',
  'err.unsupportedType': 'Unsupported file type — only epub, pdf, and docx are supported.',
  'log.ocrStart': 'OCR: {file}',
  'log.converting': 'Converting {file} with Calibre…',
  'log.pageFailed': 'Page {page} failed: {reason}',
  'log.failedPages': 'Pages that failed OCR: {pages}',
  'log.baiduToken': 'Baidu OCR: requesting an access token…',
  'log.baiduSubmit': 'Baidu OCR: uploading the document…',
  'log.baiduDownload': 'Baidu OCR: downloading the markdown…',
  'log.baiduStatus': 'Baidu OCR: {status}',
  'log.baiduStatus.pending': 'queued',
  'log.baiduStatus.running': 'processing',
  'log.baiduStatus.success': 'done',
  'log.baiduStatus.failed': 'failed',
  'settings.ocrProvider': 'OCR service',
  'topic.importTitle': 'Import topic',
  'topic.required': 'Every new book must belong to one topic',
  'topic.manage': 'Manage topics',
  'topic.emptySetup': 'No topics yet. Create 1–5 topics before importing books.',
  'topic.current': 'Current import topic',
  'topic.bookCount': '{count} books',
  'topic.agentDesign': 'Design topics with AI',
  'topic.agentRunning': 'AI is designing…',
  'topic.agentScopeInventory': 'Reads only the topic inventory',
  'topic.agentScopeVault': 'Read-only task · may read the whole Vault',
  'topic.agentProviderMissing': 'Unavailable: install Claude, Codex, or DeepSeek Agent.',
  'topic.agentProviderUnavailable': 'Unavailable: the selected Agent is not ready.',
  'topic.unclassifiedCount': '{count} existing books need a topic',
  'topic.chooseForBook': 'Choose a topic for {name}',
  'topic.choose': 'Choose a topic…',
  'topic.proposalTitle': 'AI topic proposal',
  'topic.proposalHint': 'Review the proposed topics and book assignments before applying them.',
  'topic.assignmentCount': '{count} assigned books',
  'topic.applyProposal': 'Apply proposal',
  'topic.filter': 'Filter library by topic',
  'topic.all': 'All topics',
  'topic.unclassified': 'Unclassified',
  'topic.manager.title': 'Manage book topics',
  'topic.manager.hint': 'This order is also used on the import screen.',
  'topic.manager.close': 'Close',
  'topic.manager.newTopic': 'New topic {number}',
  'topic.manager.sort': 'Topic order',
  'topic.manager.up': 'Move up',
  'topic.manager.down': 'Move down',
  'topic.manager.id': 'Stable ID',
  'topic.manager.idLocked': 'Cannot be changed after creation',
  'topic.manager.label': 'Topic name',
  'topic.manager.labelPlaceholder': 'For example: Software Engineering',
  'topic.manager.description': 'Domain description',
  'topic.manager.descriptionPlaceholder': 'Define what belongs here so people and agents classify consistently',
  'topic.manager.index': 'Index file',
  'topic.manager.indexPlaceholder': 'Software Engineering.index.md',
  'topic.manager.vocabulary': 'Related vocabulary and descriptions',
  'topic.manager.addTerm': 'Add term',
  'topic.manager.term': 'Term',
  'topic.manager.termDescription': 'Describe what this term means in the domain',
  'topic.manager.removeTerm': 'Remove',
  'topic.manager.migrate': 'Migrate books to',
  'topic.manager.chooseOther': 'Choose another topic…',
  'topic.manager.delete': 'Delete topic',
  'topic.manager.add': 'Add topic',
  'topic.manager.fix': 'Complete or correct the topic information',
  'topic.manager.saving': 'Saving…',
  'topic.validation.required': 'This field is required',
  'topic.validation.tooFew': 'At least two entries are required',
  'topic.validation.tooMany': 'At most five topics are allowed',
  'topic.validation.invalidId': 'Use lowercase letters, digits, and single hyphens only',
  'topic.validation.invalidIndex': 'Use one safe .index.md filename in the library root',
  'topic.validation.duplicateId': 'Topic IDs must be unique',
  'topic.validation.duplicateLabel': 'Topic names must be unique',
  'topic.validation.duplicateIndex': 'Index filenames must be unique',
  'topic.validation.duplicateTerm': 'Terms must be unique within a topic',
}

const zh: Catalog = {
  title: '导入电子书',
  'drop.hint': '将 epub / pdf / docx 文件拖到这里',
  'drop.pick': '添加文件…',
  'ocr.label': 'OCR(扫描版 PDF)',
  'ocr.onlyPdf': 'OCR 仅对 PDF 生效',
  'ocr.provider.wechat': '微信OCR',
  'ocr.provider.baidu': '百度 Unlimited-OCR',
  'settings.toggle': '设置',
  'settings.root': '电子书根目录',
  'settings.wechatUrl': '微信 OCR 地址',
  'settings.baiduKey': '百度 API Key',
  'settings.baiduHint':
    '在百度智能云控制台 → 人工智能 → 文字识别/文档解析 → 应用列表 → 创建应用,把该应用的 API Key 和 Secret Key 填在下面。按页计费,约 ¥0.1/页。',
  'ocr.baiduCost': '按页付费,约 ¥0.1/页',
  'settings.baiduSecret': '百度 Secret Key',
  'settings.calibre.found': '已找到 Calibre：{path}（{version}）',
  'settings.calibre.missing': '未找到 Calibre',
  'settings.calibre.pick': '选择…',
  'settings.calibre.install': '安装 Calibre',
  'settings.save': '保存',
  'queue.empty': '任务列表为空——先添加文件、选好是否 OCR,再点「开始导入」。',
  'status.pending': '等待中',
  'status.running': '进行中',
  'status.done': '已完成',
  'status.failed': '失败',
  'status.cancelled': '已取消',
  'action.openInEditor': '在编辑器打开',
  'action.cancel': '取消',
  'action.clear': '清除已完成',
  'action.start': '开始导入',
  'action.running': '导入中…',
  'action.aiRead': 'AI 先读',
  'action.aiReread': '重读',
  'library.title': '书库',
  'library.empty': '还没有书——导入一本,它就会出现在这里。',
  'library.noMatch': '没有匹配这个书名的书。',
  'library.search': '按书名搜索',
  'library.refresh': '刷新',
  'library.summaryOn': '{date} 摘要',
  'library.unread': '尚未读过',
  'settings.prompt': 'AI 阅读提示词',
  'settings.promptHint': '提示词就是 vault 里的一个纯文本文件。改了它,之后每一次 AI 阅读都按你的版本来。',
  'err.promptMissing': '提示词文件还没落到 vault——先跑一次「AI 先读」,模板会自动生成。',
  'agentPicker.by': '由 {name} 执行',
  'agentPicker.model': '模型 {model}',
  'agentPicker.unknown': '运行环境未知',
  'agentPicker.notInstalled': '未安装',
  'agentPicker.broken': '装了,但起不来',
  'action.viewSummary': '查看摘要',
  'ai.queued': '排队等待 AI 阅读…',
  'ai.running': 'AI 阅读中… {elapsed}',
  'ai.failed': 'AI 阅读失败',
  'log.toggle': '日志',
  'stage.convert': '转换中',
  'stage.extract': '解包中',
  'stage.markdown': '生成 Markdown 中',
  'stage.ocr': 'OCR 识别中',
  'stage.finalize': '整理中',
  'dialog.ebooksFilter': '电子书',
  'dialog.pickBooks': '选择电子书文件',
  'dialog.pickCalibre': '选择 ebook-convert 可执行文件',
  'err.noVault': '未配置 vault——请先打开或创建一个 vault。',
  'err.calibreMissing': '未找到 Calibre——请安装,或在设置中指定路径。',
  'err.calibreTimeout': 'Calibre 转换超时——请尝试更小的文件,或检查设置。',
  'err.calibreFailed': 'Calibre 转换失败——详情见日志。',
  'err.badRoot': '电子书根目录必须是 vault 内的路径——请检查设置。',
  'err.noTitle': '无法确定这本书的标题。',
  'err.ocrOnlyPdf': 'OCR 仅支持 PDF 文件。',
  'err.ocrEmpty': 'OCR 未识别出任何文字——扫描件可能是空白或无法识别。',
  'err.ocrUnreachable': '无法连接 OCR 服务——请检查网络或设置。',
  'err.baiduFailed': '百度 OCR 失败——详情见日志。',
  'err.baiduCredentials':
    '百度不认这组凭据。请填 OCR 控制台中「应用」的 API Key 和 Secret Key(不是「安全认证」里的 Access Key),并确认两栏没有填反。',
  'err.baiduAuth': '百度鉴权失败——请检查设置里的 API Key 和 Secret Key。',
  'err.unsupportedType': '不支持的文件类型——仅支持 epub、pdf、docx。',
  'log.ocrStart': 'OCR:{file}',
  'log.converting': '正在用 Calibre 转换 {file}…',
  'log.pageFailed': '第 {page} 页失败:{reason}',
  'log.failedPages': 'OCR 失败的页码:{pages}',
  'log.baiduToken': '百度 OCR:正在获取访问令牌…',
  'log.baiduSubmit': '百度 OCR:正在上传文档…',
  'log.baiduDownload': '百度 OCR:正在下载识别结果…',
  'log.baiduStatus': '百度 OCR:{status}',
  'log.baiduStatus.pending': '排队中',
  'log.baiduStatus.running': '识别中',
  'log.baiduStatus.success': '已完成',
  'log.baiduStatus.failed': '失败',
  'settings.ocrProvider': 'OCR 服务',
  'topic.importTitle': '导入主题',
  'topic.required': '每本新书必须归入一个主题',
  'topic.manage': '管理主题',
  'topic.emptySetup': '尚未设置主题。创建 1–5 个主题后即可导入书籍。',
  'topic.current': '当前导入主题',
  'topic.bookCount': '{count} 本',
  'topic.agentDesign': 'AI 根据书库设计主题',
  'topic.agentRunning': 'AI 正在设计…',
  'topic.agentScopeInventory': '仅可读取主题 inventory',
  'topic.agentScopeVault': '只读任务 · 可能读取整个 Vault',
  'topic.agentProviderMissing': '不可用：请安装 Claude、Codex 或 DeepSeek Agent。',
  'topic.agentProviderUnavailable': '不可用：所选 Agent 当前未就绪。',
  'topic.unclassifiedCount': '有 {count} 本旧书尚未分类',
  'topic.chooseForBook': '为《{name}》选择主题',
  'topic.choose': '选择主题…',
  'topic.proposalTitle': 'AI 主题方案',
  'topic.proposalHint': '应用前请确认主题以及每个主题包含的书籍数量。',
  'topic.assignmentCount': '归入 {count} 本',
  'topic.applyProposal': '应用方案',
  'topic.filter': '按主题筛选书库',
  'topic.all': '全部主题',
  'topic.unclassified': '未分类',
  'topic.manager.title': '管理书籍主题',
  'topic.manager.hint': '主题顺序也会用于导入界面的展示顺序。',
  'topic.manager.close': '关闭',
  'topic.manager.newTopic': '新主题 {number}',
  'topic.manager.sort': '主题排序',
  'topic.manager.up': '上移',
  'topic.manager.down': '下移',
  'topic.manager.id': '稳定 ID',
  'topic.manager.idLocked': '创建后不可修改',
  'topic.manager.label': '主题名称',
  'topic.manager.labelPlaceholder': '例如：软件工程',
  'topic.manager.description': '领域说明',
  'topic.manager.descriptionPlaceholder': '说明主题边界，以便用户和 Agent 一致分类',
  'topic.manager.index': '索引文件',
  'topic.manager.indexPlaceholder': '软件工程.index.md',
  'topic.manager.vocabulary': '相关词汇与描述',
  'topic.manager.addTerm': '添加词汇',
  'topic.manager.term': '词汇',
  'topic.manager.termDescription': '描述这个词在领域中的含义',
  'topic.manager.removeTerm': '删除',
  'topic.manager.migrate': '删除前迁移到',
  'topic.manager.chooseOther': '选择其他主题…',
  'topic.manager.delete': '删除主题',
  'topic.manager.add': '添加主题',
  'topic.manager.fix': '请补全或修正主题信息',
  'topic.manager.saving': '保存中…',
  'topic.validation.required': '此项不能为空',
  'topic.validation.tooFew': '至少需要两项',
  'topic.validation.tooMany': '最多只能有 5 个主题',
  'topic.validation.invalidId': '只能使用小写字母、数字和单个连字符',
  'topic.validation.invalidIndex': '请输入书库根目录下安全的 .index.md 文件名',
  'topic.validation.duplicateId': '主题 ID 不能重复',
  'topic.validation.duplicateLabel': '主题名称不能重复',
  'topic.validation.duplicateIndex': '索引文件名不能重复',
  'topic.validation.duplicateTerm': '同一主题内的词汇不能重复',
}

const ja: Catalog = {
  title: '電子書籍を取り込む',
  'drop.hint': 'epub・pdf・docx ファイルをここにドロップ',
  'drop.pick': 'ファイルを追加…',
  'ocr.label': 'OCR(スキャン PDF)',
  'ocr.onlyPdf': 'OCR は PDF にのみ適用されます',
  'ocr.provider.wechat': 'WeChat OCR',
  'ocr.provider.baidu': 'Baidu Unlimited-OCR',
  'settings.toggle': '設定',
  'settings.root': '電子書籍のルート',
  'settings.wechatUrl': 'WeChat OCR の URL',
  'settings.baiduKey': 'Baidu API キー',
  'settings.baiduHint':
    'Baidu Cloud コンソール → 人工知能 → 文字認識/文書解析 → アプリケーション一覧 → アプリを作成し、その API Key と Secret Key を下に入力します。ページ単位の課金で、1 ページ約 0.1 元です。',
  'ocr.baiduCost': '有料サービス — 1 ページ約 0.1 元',
  'settings.baiduSecret': 'Baidu シークレットキー',
  'settings.calibre.found': 'Calibre が見つかりました：{path}（{version}）',
  'settings.calibre.missing': 'Calibre が見つかりません',
  'settings.calibre.pick': '選択…',
  'settings.calibre.install': 'Calibre をインストール',
  'settings.save': '保存',
  'queue.empty': 'まだ何もありません。ファイルを追加し、OCR の有無を選んでから「開始」を押してください。',
  'status.pending': '待機中',
  'status.running': '実行中',
  'status.done': '完了',
  'status.failed': '失敗',
  'status.cancelled': 'キャンセル済み',
  'action.openInEditor': 'エディタで開く',
  'action.cancel': 'キャンセル',
  'action.clear': '完了分を消去',
  'action.start': '取り込みを開始',
  'action.running': '取り込み中…',
  'action.aiRead': 'AI に先に読ませる',
  'action.aiReread': '読み直す',
  'library.title': '蔵書',
  'library.empty': 'まだ本がありません。取り込むとここに並びます。',
  'library.noMatch': 'その書名に一致する本はありません。',
  'library.search': '書名で検索',
  'library.refresh': '再読み込み',
  'library.summaryOn': '{date} の要約',
  'library.unread': '未読',
  'settings.prompt': 'AI リーディングのプロンプト',
  'settings.promptHint':
    'プロンプトは vault 内のただのテキストファイルです。書き換えれば、以降の AI リーディングはすべてその内容に従います。',
  'err.promptMissing':
    'プロンプトファイルはまだ vault にありません。AI リーディングを一度実行すると、テンプレートが作成されます。',
  'agentPicker.by': '実行:{name}',
  'agentPicker.model': 'モデル {model}',
  'agentPicker.unknown': '実行環境不明',
  'agentPicker.notInstalled': '未インストール',
  'agentPicker.broken': 'インストール済みですが起動できません',
  'action.viewSummary': '要約を見る',
  'ai.queued': 'AI リーディング待機中…',
  'ai.running': 'AI リーディング中… {elapsed}',
  'ai.failed': 'AI リーディングに失敗しました',
  'log.toggle': 'ログ',
  'stage.convert': '変換中',
  'stage.extract': '展開中',
  'stage.markdown': 'Markdown に変換中',
  'stage.ocr': 'OCR 実行中',
  'stage.finalize': '仕上げ中',
  'dialog.ebooksFilter': '電子書籍',
  'dialog.pickBooks': '電子書籍ファイルを選択',
  'dialog.pickCalibre': 'ebook-convert 実行ファイルを選択',
  'err.noVault': 'vault が未設定です。まず vault を開くか作成してください。',
  'err.calibreMissing': 'Calibre が見つかりません。インストールするか、設定でパスを指定してください。',
  'err.calibreTimeout': 'Calibre の変換がタイムアウトしました。ファイルサイズを小さくするか、設定を確認してください。',
  'err.calibreFailed': 'Calibre の変換に失敗しました。詳細はログを確認してください。',
  'err.badRoot': '電子書籍のルートは vault 内のパスである必要があります。設定を確認してください。',
  'err.noTitle': 'この書籍のタイトルを特定できませんでした。',
  'err.ocrOnlyPdf': 'OCR は PDF ファイルにのみ対応しています。',
  'err.ocrEmpty': 'OCR でテキストを認識できませんでした。スキャンが空白か読み取れない可能性があります。',
  'err.ocrUnreachable': 'OCR サービスに接続できません。ネットワークまたは設定を確認してください。',
  'err.baiduFailed': 'Baidu OCR が失敗しました。詳細はログを確認してください。',
  'err.baiduCredentials':
    'Baidu が認証情報を受け付けませんでした。OCR コンソールの「アプリケーション」の API Key と Secret Key(「セキュリティ認証」の Access Key ではありません)を入力し、2 つの欄が入れ替わっていないか確認してください。',
  'err.baiduAuth': 'Baidu の認証に失敗しました。設定の API キーとシークレットキーを確認してください。',
  'err.unsupportedType': 'サポートされていないファイル形式です。epub、pdf、docx のみ対応しています。',
  'log.ocrStart': 'OCR:{file}',
  'log.converting': 'Calibre で {file} を変換中…',
  'log.pageFailed': '{page} ページ目が失敗:{reason}',
  'log.failedPages': 'OCR に失敗したページ:{pages}',
  'log.baiduToken': 'Baidu OCR:アクセストークンを取得中…',
  'log.baiduSubmit': 'Baidu OCR:ドキュメントをアップロード中…',
  'log.baiduDownload': 'Baidu OCR:結果をダウンロード中…',
  'log.baiduStatus': 'Baidu OCR:{status}',
  'log.baiduStatus.pending': '待機中',
  'log.baiduStatus.running': '処理中',
  'log.baiduStatus.success': '完了',
  'log.baiduStatus.failed': '失敗',
  'settings.ocrProvider': 'OCR サービス',
  'topic.importTitle': '取り込みテーマ',
  'topic.required': '新しい本には必ずテーマを1つ指定します',
  'topic.manage': 'テーマを管理',
  'topic.emptySetup': 'テーマがありません。取り込み前に1〜5個作成してください。',
  'topic.current': '現在の取り込みテーマ',
  'topic.bookCount': '{count} 冊',
  'topic.agentDesign': 'AIでテーマを設計',
  'topic.agentRunning': 'AIが設計中…',
  'topic.agentScopeInventory': 'トピック inventory のみ読み取り可',
  'topic.agentScopeVault': '読み取り専用タスク · Vault 全体を読み取る場合があります',
  'topic.agentProviderMissing': '利用不可：Claude、Codex、または DeepSeek Agent をインストールしてください。',
  'topic.agentProviderUnavailable': '利用不可：選択した Agent は現在使用できません。',
  'topic.unclassifiedCount': '既存の {count} 冊が未分類です',
  'topic.chooseForBook': '「{name}」のテーマを選択',
  'topic.choose': 'テーマを選択…',
  'topic.proposalTitle': 'AIテーマ案',
  'topic.proposalHint': '適用前にテーマと本の割り当てを確認してください。',
  'topic.assignmentCount': '{count} 冊を割り当て',
  'topic.applyProposal': '案を適用',
  'topic.filter': 'テーマで蔵書を絞り込む',
  'topic.all': 'すべてのテーマ',
  'topic.unclassified': '未分類',
  'topic.manager.title': '書籍テーマを管理',
  'topic.manager.hint': 'この順序は取り込み画面にも使われます。',
  'topic.manager.close': '閉じる',
  'topic.manager.newTopic': '新しいテーマ {number}',
  'topic.manager.sort': 'テーマの順序',
  'topic.manager.up': '上へ',
  'topic.manager.down': '下へ',
  'topic.manager.id': '固定 ID',
  'topic.manager.idLocked': '作成後は変更できません',
  'topic.manager.label': 'テーマ名',
  'topic.manager.labelPlaceholder': '例：ソフトウェア工学',
  'topic.manager.description': '分野の説明',
  'topic.manager.descriptionPlaceholder': '人と Agent が一貫して分類できるよう境界を説明します',
  'topic.manager.index': '索引ファイル',
  'topic.manager.indexPlaceholder': 'ソフトウェア工学.index.md',
  'topic.manager.vocabulary': '関連語彙と説明',
  'topic.manager.addTerm': '語彙を追加',
  'topic.manager.term': '語彙',
  'topic.manager.termDescription': 'この分野での意味を説明',
  'topic.manager.removeTerm': '削除',
  'topic.manager.migrate': '削除前の移動先',
  'topic.manager.chooseOther': '別のテーマを選択…',
  'topic.manager.delete': 'テーマを削除',
  'topic.manager.add': 'テーマを追加',
  'topic.manager.fix': 'テーマ情報を入力または修正してください',
  'topic.manager.saving': '保存中…',
  'topic.validation.required': '必須項目です',
  'topic.validation.tooFew': '2項目以上必要です',
  'topic.validation.tooMany': 'テーマは最大5個です',
  'topic.validation.invalidId': '小文字、数字、単一ハイフンのみ使用できます',
  'topic.validation.invalidIndex': 'ルート直下の安全な .index.md ファイル名を指定してください',
  'topic.validation.duplicateId': 'テーマ ID は重複できません',
  'topic.validation.duplicateLabel': 'テーマ名は重複できません',
  'topic.validation.duplicateIndex': '索引ファイル名は重複できません',
  'topic.validation.duplicateTerm': 'テーマ内の語彙は重複できません',
}

const de: Catalog = {
  title: 'E-Books importieren',
  'drop.hint': 'epub-, pdf- oder docx-Dateien hierher ziehen',
  'drop.pick': 'Dateien hinzufügen…',
  'ocr.label': 'OCR (gescanntes PDF)',
  'ocr.onlyPdf': 'OCR gilt nur für PDF',
  'ocr.provider.wechat': 'WeChat-OCR',
  'ocr.provider.baidu': 'Baidu Unlimited-OCR',
  'settings.toggle': 'Einstellungen',
  'settings.root': 'E-Book-Stammverzeichnis',
  'settings.wechatUrl': 'WeChat-OCR-URL',
  'settings.baiduKey': 'Baidu-API-Schlüssel',
  'settings.baiduHint':
    'Baidu-Cloud-Konsole → KI → Texterkennung/Dokumentanalyse → Anwendungen → eine anlegen; deren API Key und Secret Key gehören hier hinein. Abrechnung pro Seite, etwa ¥0,1 (CNY) je Seite.',
  'ocr.baiduCost': 'Kostenpflichtig — etwa ¥0,1 (CNY) pro Seite',
  'settings.baiduSecret': 'Baidu-Geheimschlüssel',
  'settings.calibre.found': 'Calibre gefunden: {path} ({version})',
  'settings.calibre.missing': 'Calibre nicht gefunden',
  'settings.calibre.pick': 'Auswählen…',
  'settings.calibre.install': 'Calibre installieren',
  'settings.save': 'Speichern',
  'queue.empty': 'Noch nichts in der Liste — Dateien hinzufügen, OCR wählen, dann auf Start drücken.',
  'status.pending': 'Ausstehend',
  'status.running': 'Läuft',
  'status.done': 'Fertig',
  'status.failed': 'Fehlgeschlagen',
  'status.cancelled': 'Abgebrochen',
  'action.openInEditor': 'Im Editor öffnen',
  'action.cancel': 'Abbrechen',
  'action.clear': 'Fertige entfernen',
  'action.start': 'Import starten',
  'action.running': 'Importiert…',
  'action.aiRead': 'Zuerst KI lesen lassen',
  'action.aiReread': 'Erneut lesen',
  'library.title': 'Bibliothek',
  'library.empty': 'Noch keine Bücher — importiere eines, dann steht es hier.',
  'library.noMatch': 'Kein Buch passt zu diesem Titel.',
  'library.search': 'Nach Titel suchen',
  'library.refresh': 'Aktualisieren',
  'library.summaryOn': 'Zusammenfassung vom {date}',
  'library.unread': 'Noch nicht gelesen',
  'settings.prompt': 'Prompt für die KI-Lektüre',
  'settings.promptHint':
    'Der Prompt ist eine einfache Datei in deinem Vault. Änderst du sie, folgt jede weitere KI-Lektüre deiner Fassung.',
  'err.promptMissing':
    'Die Prompt-Datei liegt noch nicht im Vault — führe eine KI-Lektüre aus, dann wird die Vorlage angelegt.',
  'agentPicker.by': 'via {name}',
  'agentPicker.model': 'Modell {model}',
  'agentPicker.unknown': 'Umgebung unbekannt',
  'agentPicker.notInstalled': 'nicht installiert',
  'agentPicker.broken': 'vorhanden, startet aber nicht',
  'action.viewSummary': 'Zusammenfassung öffnen',
  'ai.queued': 'Wartet auf KI…',
  'ai.running': 'KI liest… {elapsed}',
  'ai.failed': 'KI-Lektüre fehlgeschlagen',
  'log.toggle': 'Protokoll',
  'stage.convert': 'Wird konvertiert',
  'stage.extract': 'Wird entpackt',
  'stage.markdown': 'Wird in Markdown umgewandelt',
  'stage.ocr': 'OCR läuft',
  'stage.finalize': 'Wird abgeschlossen',
  'dialog.ebooksFilter': 'E-Books',
  'dialog.pickBooks': 'E-Book-Dateien auswählen',
  'dialog.pickCalibre': 'Wählen Sie die ebook-convert-Binärdatei',
  'err.noVault': 'Kein Vault konfiguriert — bitte zuerst einen Vault öffnen oder erstellen.',
  'err.calibreMissing': 'Calibre nicht gefunden — installieren oder den Pfad in den Einstellungen angeben.',
  'err.calibreTimeout': 'Zeitüberschreitung bei der Calibre-Konvertierung — kleinere Datei versuchen oder Einstellungen prüfen.',
  'err.calibreFailed': 'Calibre-Konvertierung fehlgeschlagen — Details im Protokoll.',
  'err.badRoot': 'Das E-Book-Stammverzeichnis muss ein Pfad innerhalb des Vaults sein — Einstellungen prüfen.',
  'err.noTitle': 'Für dieses Buch konnte kein Titel ermittelt werden.',
  'err.ocrOnlyPdf': 'OCR funktioniert nur mit PDF-Dateien.',
  'err.ocrEmpty': 'OCR hat keinen Text erkannt — der Scan ist möglicherweise leer oder unlesbar.',
  'err.ocrUnreachable': 'OCR-Dienst nicht erreichbar — Netzwerk oder Einstellungen prüfen.',
  'err.baiduFailed': 'Baidu-OCR fehlgeschlagen — Details im Protokoll.',
  'err.baiduCredentials':
    'Baidu hat die Zugangsdaten nicht erkannt. Verwende API Key und Secret Key einer Anwendung aus der OCR-Konsole — kein Access-Key-Paar — und prüfe, ob die beiden Felder vertauscht sind.',
  'err.baiduAuth': 'Baidu-Anmeldung fehlgeschlagen — API-Key und Secret Key in den Einstellungen prüfen.',
  'err.unsupportedType': 'Nicht unterstützter Dateityp — nur epub, pdf und docx werden unterstützt.',
  'log.ocrStart': 'OCR läuft: {file}',
  'log.converting': 'Konvertiere {file} mit Calibre…',
  'log.pageFailed': 'Seite {page} fehlgeschlagen: {reason}',
  'log.failedPages': 'Seiten mit OCR-Fehler: {pages}',
  'log.baiduToken': 'Baidu OCR: Zugriffstoken wird angefordert…',
  'log.baiduSubmit': 'Baidu OCR: Dokument wird hochgeladen…',
  'log.baiduDownload': 'Baidu OCR: Markdown wird heruntergeladen…',
  'log.baiduStatus': 'Baidu-OCR: {status}',
  'log.baiduStatus.pending': 'in der Warteschlange',
  'log.baiduStatus.running': 'wird verarbeitet',
  'log.baiduStatus.success': 'fertig',
  'log.baiduStatus.failed': 'fehlgeschlagen',
  'settings.ocrProvider': 'OCR-Dienst',
  'topic.importTitle': 'Importthema',
  'topic.required': 'Jedes neue Buch benötigt genau ein Thema',
  'topic.manage': 'Themen verwalten',
  'topic.emptySetup': 'Noch keine Themen. Vor dem Import 1–5 Themen anlegen.',
  'topic.current': 'Aktuelles Importthema',
  'topic.bookCount': '{count} Bücher',
  'topic.agentDesign': 'Themen mit KI entwerfen',
  'topic.agentRunning': 'KI entwirft…',
  'topic.agentScopeInventory': 'Liest nur das Themen-Inventory',
  'topic.agentScopeVault': 'Schreibgeschützte Aufgabe · kann den gesamten Vault lesen',
  'topic.agentProviderMissing': 'Nicht verfügbar: Claude, Codex oder DeepSeek Agent installieren.',
  'topic.agentProviderUnavailable': 'Nicht verfügbar: Der ausgewählte Agent ist nicht bereit.',
  'topic.unclassifiedCount': '{count} vorhandene Bücher sind nicht klassifiziert',
  'topic.chooseForBook': 'Thema für „{name}“ auswählen',
  'topic.choose': 'Thema auswählen…',
  'topic.proposalTitle': 'KI-Themenvorschlag',
  'topic.proposalHint': 'Bitte Themen und Buchzuordnungen vor dem Anwenden prüfen.',
  'topic.assignmentCount': '{count} Bücher zugeordnet',
  'topic.applyProposal': 'Vorschlag anwenden',
  'topic.filter': 'Bibliothek nach Thema filtern',
  'topic.all': 'Alle Themen',
  'topic.unclassified': 'Nicht klassifiziert',
  'topic.manager.title': 'Buchthemen verwalten',
  'topic.manager.hint': 'Diese Reihenfolge gilt auch im Importfenster.',
  'topic.manager.close': 'Schließen',
  'topic.manager.newTopic': 'Neues Thema {number}',
  'topic.manager.sort': 'Themenreihenfolge',
  'topic.manager.up': 'Nach oben',
  'topic.manager.down': 'Nach unten',
  'topic.manager.id': 'Stabile ID',
  'topic.manager.idLocked': 'Nach dem Anlegen nicht änderbar',
  'topic.manager.label': 'Themenname',
  'topic.manager.labelPlaceholder': 'Zum Beispiel: Softwareentwicklung',
  'topic.manager.description': 'Fachgebietsbeschreibung',
  'topic.manager.descriptionPlaceholder': 'Grenzen für eine einheitliche Zuordnung durch Menschen und Agents beschreiben',
  'topic.manager.index': 'Indexdatei',
  'topic.manager.indexPlaceholder': 'Softwareentwicklung.index.md',
  'topic.manager.vocabulary': 'Verwandte Begriffe und Beschreibungen',
  'topic.manager.addTerm': 'Begriff hinzufügen',
  'topic.manager.term': 'Begriff',
  'topic.manager.termDescription': 'Bedeutung des Begriffs im Fachgebiet',
  'topic.manager.removeTerm': 'Entfernen',
  'topic.manager.migrate': 'Bücher verschieben nach',
  'topic.manager.chooseOther': 'Anderes Thema auswählen…',
  'topic.manager.delete': 'Thema löschen',
  'topic.manager.add': 'Thema hinzufügen',
  'topic.manager.fix': 'Themenangaben vervollständigen oder korrigieren',
  'topic.manager.saving': 'Speichert…',
  'topic.validation.required': 'Dieses Feld ist erforderlich',
  'topic.validation.tooFew': 'Mindestens zwei Einträge sind erforderlich',
  'topic.validation.tooMany': 'Höchstens fünf Themen sind erlaubt',
  'topic.validation.invalidId': 'Nur Kleinbuchstaben, Ziffern und einzelne Bindestriche verwenden',
  'topic.validation.invalidIndex': 'Einen sicheren .index.md-Dateinamen im Bibliotheksstamm verwenden',
  'topic.validation.duplicateId': 'Themen-IDs müssen eindeutig sein',
  'topic.validation.duplicateLabel': 'Themennamen müssen eindeutig sein',
  'topic.validation.duplicateIndex': 'Indexdateinamen müssen eindeutig sein',
  'topic.validation.duplicateTerm': 'Begriffe müssen innerhalb eines Themas eindeutig sein',
}

const registry: Record<Locale, Catalog> = { en, zh, ja, de }

let active: Locale = 'en'

function isLocale(v: unknown): v is Locale {
  return v === 'en' || v === 'zh' || v === 'ja' || v === 'de'
}

/**
 * Sets the active locale from `notemd.locale`. Accepts a region suffix
 * (`zh-CN` → `zh`); unknown/absent falls back to English.
 */
export function setLocale(code: string | undefined): void {
  const base = code?.split('-')[0]
  active = isLocale(base) ? base : 'en'
}

/**
 * Translates `key` for the active locale, filling `{name}` placeholders from
 * `params`. Falls back to the English catalog for a missing key, then to the
 * raw key.
 */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const catalog = registry[active] ?? en
  let s = catalog[key] ?? en[key] ?? key
  if (params) {
    s = s.replace(/\{(\w+)\}/g, (m, name) => (name in params ? String(params[name]) : m))
  }
  return s
}

// Exported for tests only (catalog completeness / placeholder parity checks).
export const CATALOGS: Record<Locale, Catalog> = registry
export const LOCALES: Locale[] = ['en', 'zh', 'ja', 'de']
