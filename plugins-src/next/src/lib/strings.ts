export type Locale = 'en' | 'zh' | 'ja' | 'de'

export type MessageKey =
  | 'app.title'
  | 'app.value'
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
  | 'action.hideCapture'
  | 'action.findPlaced'
  | 'action.hidePlaced'
  | 'action.place'
  | 'action.reopen'
  | 'action.relink'
  | 'action.openSource'
  | 'badge.proofed'
  | 'badge.orphan'
  | 'badge.unsupported'
  | 'warning.wip'
  | 'warning.waiting'
  | 'warning.readOnly'
  | 'search.placeholder'
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
  | 'exit.done'
  | 'exit.stopped'
  | 'exit.transferred'
  | 'exit.compressed'
  | 'via.none'
  | 'via.delegateDone'
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
  'app.value': 'Give every idea a next step — or a place to rest.',
  'common.loading': 'Loading…',
  'common.refresh': 'Refresh',
  'common.open': 'Open',
  'common.cancel': 'Cancel',
  'common.save': 'Place idea',
  'common.optional': 'Optional',
  'common.error': 'Something went wrong',
  'empty.wip': 'Nothing is actively committed.',
  'empty.waiting': 'Nothing is waiting for review.',
  'empty.capture': 'No recent ideas need a decision.',
  'empty.search': 'No placed ideas match.',
  'section.wip': 'In hand',
  'section.waiting': 'Waiting',
  'section.capture': 'Ready to place',
  'section.resurfaced': 'Back in view',
  'section.placed': 'Placed ideas',
  'section.repair': 'Needs attention',
  'count.wip': '{count}/3',
  'count.waiting': '{count}',
  'action.placeOne': 'Place an idea',
  'action.hideCapture': 'Hide ideas',
  'action.findPlaced': 'Find placed ideas',
  'action.hidePlaced': 'Hide placed ideas',
  'action.place': 'Place',
  'action.reopen': 'Reopen',
  'action.relink': 'Relink',
  'action.openSource': 'Open source',
  'badge.proofed': 'Proofed',
  'badge.orphan': 'Source missing',
  'badge.unsupported': 'Needs repair',
  'warning.wip': 'Three or more ideas are already in hand. Finish or place one before adding another.',
  'warning.waiting': 'More than five items are waiting. Make sure each still has a real review responsibility.',
  'warning.readOnly': 'Next is read-only until its event document is repaired.',
  'search.placeholder': 'Search title, action, or destination',
  'sheet.title': 'Place “{title}”',
  'sheet.choose': 'What happens next?',
  'route.commit': 'Move forward now',
  'route.commit.detail': 'Take responsibility for a concrete next step.',
  'route.wait': 'Wait to review',
  'route.wait.detail': 'An external result still needs your acceptance.',
  'route.park': 'Look again later',
  'route.park.detail': 'Keep the memory without a current commitment.',
  'route.settle': 'End or move elsewhere',
  'route.settle.detail': 'Finish, stop, transfer, or compress the idea.',
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
  'exit.done': 'Done',
  'exit.stopped': 'Stopped',
  'exit.transferred': 'Moved elsewhere',
  'exit.compressed': 'Turned into a system',
  'via.none': 'Completed directly',
  'via.delegateDone': 'Delegated and accepted',
  'via.delegateTransferred': 'Responsibility transferred',
  'via.drop': 'Dropped',
  'via.disproved': 'Disproved',
  'via.ignore': 'Ignored',
  'via.merge': 'Merged',
  'via.project': 'Moved to project',
  'via.buy': 'Bought an existing solution',
  'via.publish': 'Published for others',
  'via.principle': 'Turned into a principle',
  'via.automate': 'Automated',
  'relink.title': 'Relink source',
  'relink.helpExact': 'These files have the same creation time. Confirm the correct source; Next will not rename or edit it.',
  'relink.helpManual': 'No creation-time match was found. These are unclaimed idea files; check the path and time yourself.',
  'relink.created': 'Created',
  'relink.createdUnknown': 'unknown',
  'relink.noCandidates': 'No unclaimed idea files were found.',
  'error.required': 'Complete the required fields.',
  'error.load': 'Next could not load your ideas.',
  'error.save': 'Next could not save this decision.',
  'error.open': 'The source file could not be opened.',
  'status.capture': 'Ready to place',
  'status.wip': 'In hand',
  'status.waiting': 'Waiting',
  'status.dormant': 'Later',
  'status.closed': 'Closed',
  'status.unsupported': 'Unsupported',
}

const zh: Catalog = {
  'app.title': 'Next',
  'app.value': '给每个想法一个下一步，或一个安心的去处。',
  'common.loading': '正在载入…',
  'common.refresh': '刷新',
  'common.open': '打开',
  'common.cancel': '取消',
  'common.save': '安放想法',
  'common.optional': '可选',
  'common.error': '出现了问题',
  'empty.wip': '手上没有正在承诺的想法。',
  'empty.waiting': '没有等待回收的结果。',
  'empty.capture': '最近没有需要安放的想法。',
  'empty.search': '没有匹配的已安放想法。',
  'section.wip': '手上',
  'section.waiting': '等回收',
  'section.capture': '待安放',
  'section.resurfaced': '再次浮现',
  'section.placed': '已安放',
  'section.repair': '需要处理',
  'count.wip': '{count}/3',
  'count.waiting': '{count}',
  'action.placeOne': '安放一个想法',
  'action.hideCapture': '收起想法',
  'action.findPlaced': '查找已安放',
  'action.hidePlaced': '收起已安放',
  'action.place': '安放',
  'action.reopen': '重新考虑',
  'action.relink': '重新关联',
  'action.openSource': '打开原文',
  'badge.proofed': '已有论证',
  'badge.orphan': '原文失联',
  'badge.unsupported': '需要修复',
  'warning.wip': '手上已有三个或更多想法。请先完成或安放其中一个，再加入新的承诺。',
  'warning.waiting': '等待项已超过五个。请确认每一项仍有真实的回收责任。',
  'warning.readOnly': '事件文档修复前，Next 将保持只读。',
  'search.placeholder': '搜索标题、下一步或去向',
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
  'exit.done': '完成',
  'exit.stopped': '停止',
  'exit.transferred': '转到别处',
  'exit.compressed': '变成机制',
  'via.none': '直接完成',
  'via.delegateDone': '委托并已验收',
  'via.delegateTransferred': '责任已移交',
  'via.drop': '放弃',
  'via.disproved': '证伪',
  'via.ignore': '忽略',
  'via.merge': '合并',
  'via.project': '升级为项目',
  'via.buy': '购买已有方案',
  'via.publish': '公开给他人',
  'via.principle': '沉淀为原则',
  'via.automate': '自动化',
  'relink.title': '重新关联原文',
  'relink.helpExact': '这些文件的创建时间相同，请确认正确原文。Next 不会重命名或修改它。',
  'relink.helpManual': '没有找到相同创建时间。这些只是尚未被认领的 idea，请自行核对路径与时间。',
  'relink.created': '创建时间',
  'relink.createdUnknown': '未知',
  'relink.noCandidates': '没有找到尚未被认领的 idea 文件。',
  'error.required': '请补全必填内容。',
  'error.load': 'Next 无法载入你的想法。',
  'error.save': 'Next 无法保存这次安放。',
  'error.open': '无法打开原文。',
  'status.capture': '待安放',
  'status.wip': '手上',
  'status.waiting': '等回收',
  'status.dormant': '以后',
  'status.closed': '已关闭',
  'status.unsupported': '无法识别',
}

const ja: Catalog = {
  'app.title': 'Next',
  'app.value': 'すべてのアイデアに、次の一歩か安心できる置き場所を。',
  'common.loading': '読み込み中…',
  'common.refresh': '更新',
  'common.open': '開く',
  'common.cancel': 'キャンセル',
  'common.save': 'アイデアを置く',
  'common.optional': '任意',
  'common.error': '問題が発生しました',
  'empty.wip': '現在引き受けているアイデアはありません。',
  'empty.waiting': '確認待ちの項目はありません。',
  'empty.capture': '最近、判断が必要なアイデアはありません。',
  'empty.search': '一致するアイデアはありません。',
  'section.wip': '進行中',
  'section.waiting': '確認待ち',
  'section.capture': '置き場所を決める',
  'section.resurfaced': '再び表示',
  'section.placed': '配置済み',
  'section.repair': '確認が必要',
  'count.wip': '{count}/3',
  'count.waiting': '{count}',
  'action.placeOne': 'アイデアを置く',
  'action.hideCapture': 'アイデアを隠す',
  'action.findPlaced': '配置済みを探す',
  'action.hidePlaced': '配置済みを隠す',
  'action.place': '置く',
  'action.reopen': '再検討',
  'action.relink': '再リンク',
  'action.openSource': '原文を開く',
  'badge.proofed': '検証済み',
  'badge.orphan': '原文が見つかりません',
  'badge.unsupported': '修復が必要',
  'warning.wip': 'すでに三つ以上進行中です。一つを終えるか置き直してから追加してください。',
  'warning.waiting': '確認待ちが五つを超えています。すべてに確認責任が残っているか確かめてください。',
  'warning.readOnly': 'イベント文書を修復するまで Next は読み取り専用です。',
  'search.placeholder': 'タイトル、次の行動、移動先を検索',
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
  'exit.done': '完了',
  'exit.stopped': '中止',
  'exit.transferred': '別の場所へ移動',
  'exit.compressed': '仕組みに変換',
  'via.none': '直接完了',
  'via.delegateDone': '委託して検収済み',
  'via.delegateTransferred': '責任を移管',
  'via.drop': '取り下げ',
  'via.disproved': '反証',
  'via.ignore': '無視',
  'via.merge': '統合',
  'via.project': 'プロジェクトへ移動',
  'via.buy': '既存製品を購入',
  'via.publish': '公開',
  'via.principle': '原則に変換',
  'via.automate': '自動化',
  'relink.title': '原文を再リンク',
  'relink.helpExact': '作成時刻が同じファイルです。正しい原文か確認してください。Next は名前も内容も変更しません。',
  'relink.helpManual': '同じ作成時刻の候補がありません。未使用の idea ファイルなので、パスと時刻を自分で確認してください。',
  'relink.created': '作成時刻',
  'relink.createdUnknown': '不明',
  'relink.noCandidates': '未使用の idea ファイルがありません。',
  'error.required': '必須項目を入力してください。',
  'error.load': 'Next はアイデアを読み込めませんでした。',
  'error.save': 'Next はこの判断を保存できませんでした。',
  'error.open': '原文を開けませんでした。',
  'status.capture': '配置待ち',
  'status.wip': '進行中',
  'status.waiting': '確認待ち',
  'status.dormant': 'あとで',
  'status.closed': '終了',
  'status.unsupported': '未対応',
}

const de: Catalog = {
  'app.title': 'Next',
  'app.value': 'Gib jeder Idee einen nächsten Schritt – oder einen ruhigen Ort.',
  'common.loading': 'Wird geladen…',
  'common.refresh': 'Aktualisieren',
  'common.open': 'Öffnen',
  'common.cancel': 'Abbrechen',
  'common.save': 'Idee ablegen',
  'common.optional': 'Optional',
  'common.error': 'Ein Fehler ist aufgetreten',
  'empty.wip': 'Keine Idee ist derzeit verbindlich aktiv.',
  'empty.waiting': 'Nichts wartet auf eine Prüfung.',
  'empty.capture': 'Keine neue Idee braucht eine Entscheidung.',
  'empty.search': 'Keine passende abgelegte Idee.',
  'section.wip': 'In Arbeit',
  'section.waiting': 'Wartet',
  'section.capture': 'Noch abzulegen',
  'section.resurfaced': 'Wieder im Blick',
  'section.placed': 'Abgelegte Ideen',
  'section.repair': 'Prüfung nötig',
  'count.wip': '{count}/3',
  'count.waiting': '{count}',
  'action.placeOne': 'Eine Idee ablegen',
  'action.hideCapture': 'Ideen ausblenden',
  'action.findPlaced': 'Abgelegte Ideen suchen',
  'action.hidePlaced': 'Abgelegte Ideen ausblenden',
  'action.place': 'Ablegen',
  'action.reopen': 'Neu prüfen',
  'action.relink': 'Neu verknüpfen',
  'action.openSource': 'Quelle öffnen',
  'badge.proofed': 'Geprüft',
  'badge.orphan': 'Quelle fehlt',
  'badge.unsupported': 'Reparatur nötig',
  'warning.wip': 'Drei oder mehr Ideen sind bereits aktiv. Schließe eine ab oder lege sie neu ab, bevor du eine weitere hinzufügst.',
  'warning.waiting': 'Mehr als fünf Punkte warten. Prüfe, ob für jeden noch echte Abnahmeverantwortung besteht.',
  'warning.readOnly': 'Next bleibt schreibgeschützt, bis das Ereignisdokument repariert ist.',
  'search.placeholder': 'Titel, nächsten Schritt oder Ziel suchen',
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
  'exit.done': 'Erledigt',
  'exit.stopped': 'Gestoppt',
  'exit.transferred': 'Weitergegeben',
  'exit.compressed': 'In ein System überführt',
  'via.none': 'Direkt abgeschlossen',
  'via.delegateDone': 'Delegiert und abgenommen',
  'via.delegateTransferred': 'Verantwortung übertragen',
  'via.drop': 'Verworfen',
  'via.disproved': 'Widerlegt',
  'via.ignore': 'Ignoriert',
  'via.merge': 'Zusammengeführt',
  'via.project': 'In ein Projekt übertragen',
  'via.buy': 'Bestehende Lösung gekauft',
  'via.publish': 'Veröffentlicht',
  'via.principle': 'In einen Grundsatz überführt',
  'via.automate': 'Automatisiert',
  'relink.title': 'Quelle neu verknüpfen',
  'relink.helpExact': 'Diese Dateien haben denselben Erstellungszeitpunkt. Bestätige die richtige Quelle; Next ändert sie nicht.',
  'relink.helpManual': 'Kein gleicher Erstellungszeitpunkt gefunden. Prüfe Pfad und Zeitpunkt dieser unbelegten Ideen selbst.',
  'relink.created': 'Erstellt',
  'relink.createdUnknown': 'unbekannt',
  'relink.noCandidates': 'Keine unbelegten Ideendateien gefunden.',
  'error.required': 'Fülle die erforderlichen Felder aus.',
  'error.load': 'Next konnte deine Ideen nicht laden.',
  'error.save': 'Next konnte diese Entscheidung nicht speichern.',
  'error.open': 'Die Quelldatei konnte nicht geöffnet werden.',
  'status.capture': 'Noch abzulegen',
  'status.wip': 'In Arbeit',
  'status.waiting': 'Wartet',
  'status.dormant': 'Später',
  'status.closed': 'Geschlossen',
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
