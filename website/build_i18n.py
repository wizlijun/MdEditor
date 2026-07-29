#!/usr/bin/env python3
"""Generate static /de/ /ja/ /zh/ homepages from the English master public/index.html.

Usage: python3 build_i18n.py   (run inside website/; site lives in public/)
Rows are (en, zh, de, ja); en must match public/index.html exactly.
Unmatched strings are reported loudly."""
import os, sys

BASE = "https://notemd.net"

STRINGS = [('<title>note.md — The markdown editor for humans and agents</title>',
  '<title>note.md — 人与 agent 共用的 markdown 编辑器</title>',
  '<title>note.md — Der Markdown-Editor für Menschen und Agents</title>',
  '<title>note.md — 人間とエージェントのための markdown エディタ</title>'),
 ('content="note.md is a markdown reader, editor and bidirectional-linking notes tool for the AI-native era. Read what '
  'your agents write, annotate it, keep it in plain files you own forever."',
  'content="note.md 是为 AI-native 时代打造的 markdown 阅读器、编辑器与双链笔记工具。读 agent 写的东西，就地批注，存在永远属于你的纯文本文件里。"',
  'content="note.md ist ein Markdown-Reader, -Editor und Notiz-Tool mit bidirektionalen Links für das KI-Zeitalter. '
  'Lies, was deine Agents schreiben, annotiere es, behalte es in einfachen Dateien, die dir für immer gehören."',
  'content="note.md は AI ネイティブ時代の markdown リーダー、エディタ、双方向リンクのノートツール。エージェントが書いたものを読み、その場で書き込み、永遠にあなたのものであるプレーンなファイルに残す。"'),
 ('<a href="#features">features</a>',
  '<a href="#features">功能</a>',
  '<a href="#features">funktionen</a>',
  '<a href="#features">機能</a>'),
 ('<a href="#sidecar">sidecar notes</a>',
  '<a href="#sidecar">手记</a>',
  '<a href="#sidecar">randnotizen</a>',
  '<a href="#sidecar">サイドノート</a>'),
 ('<a href="/orchestrate-agents/">for agents</a>',
  '<a href="/orchestrate-agents/">给 agent</a>',
  '<a href="/orchestrate-agents/">für agents</a>',
  '<a href="/orchestrate-agents/">エージェント向け</a>'),
 ('<a href="https://plugins.notemd.net">plugins</a>',
  '<a href="https://plugins.notemd.net">插件</a>',
  '<a href="https://plugins.notemd.net">plugins</a>',
  '<a href="https://plugins.notemd.net">プラグイン</a>'),
 ('"/download">Download</a>',
  '"/download">下载</a>',
  '"/download">Laden</a>',
  '"/download">ダウンロード</a>'),
 ('<span class="kicker">For the age of infinite text</span>',
  '<span class="kicker">无限文本的时代</span>',
  '<span class="kicker">Für das Zeitalter des unendlichen Texts</span>',
  '<span class="kicker">無限のテキストの時代に</span>'),
 ('<h1>Read what AI writes.<br>Keep what you think.<br>Keep what only <em>you</em> can write.<span class="cursor"></span></h1>',
  '<h1>读 AI 写的。<br>留下你想的。<br>留下只有<em>你</em>写得出的。<span class="cursor"></span></h1>',
  '<h1>Lies, was die KI schreibt.<br>Behalte, was du denkst.<br>Behalte, was nur <em>du</em> schreiben kannst.<span class="cursor"></span></h1>',
  '<h1>読むのは AI の文章。<br>残すのはあなたの考え。<br>残すのは、<em>あなた</em>にしか書けないもの。<span class="cursor"></span></h1>'),
 ("Agents write more in a night than you'll read all year. Plain files. No lock-in. Yours.",
  'agent 一晚写的，比你一年读得完的还多。纯文件，无锁定，属于你。',
  'Deine Agents schreiben in einer Nacht mehr, als du in einem Jahr liest. Einfache Dateien. Kein Lock-in. Deins.',
  'エージェントが一晩で書く量は、あなたが一年で読む量より多い。プレーンなファイル。ロックインなし。あなたのもの。'),
 ('<span class="bl">Download for macOS</span>',
  '<span class="bl">下载 macOS 版</span>',
  '<span class="bl">Für macOS laden</span>',
  '<span class="bl">macOS 版をダウンロード</span>'),
 ('<span class="bl">Star on GitHub</span>',
  '<span class="bl">GitHub 加星</span>',
  '<span class="bl">Auf GitHub sternen</span>',
  '<span class="bl">GitHub でスター</span>'),
 ('macOS 13+ · free &amp; open · your files stay on your disk · <a href="/download?arch=x86_64">Intel Mac?</a>',
  'macOS 13+ · 免费开源 · 文件只在你的磁盘上 · <a href="/download?arch=x86_64">Intel 芯片 Mac？</a>',
  'macOS 13+ · frei &amp; offen · deine Dateien bleiben auf deiner Platte · <a href="/download?arch=x86_64">Intel-Mac?</a>',
  'macOS 13+ · 無料＆オープン · ファイルはあなたのディスクに · <a href="/download?arch=x86_64">Intel Mac は？</a>'),
 ('<div class="sec-k">Four things</div>',
  '<div class="sec-k">四件事</div>',
  '<div class="sec-k">Vier Dinge</div>',
  '<div class="sec-k">四つのこと</div>'),
 # ---- claim 01 ----
 ('<h2>The best place to read what your agents wrote</h2>',
  '<h2>读 agent 写的东西，<br>这里体验最好</h2>',
  '<h2>Der beste Ort, um zu lesen,<br>was deine Agents geschrieben haben</h2>',
  '<h2>エージェントが書いたものを<br>読むなら、ここが一番いい</h2>'),
 ('Rich view and source view, one keystroke apart. Any Notion- or Typora-style theme. Mermaid, Graphviz and KaTeX, '
  'tuned and lazily loaded. 11 MB, no bundled Chromium.',
  '富文本与源码双模，一个快捷键之隔。任意导入 Notion、Typora 风格主题。Mermaid、Graphviz、KaTeX 都专门调过，按需加载。11 MB，没有捆绑 Chromium。',
  'Rich-View und Quelltext, einen Tastendruck auseinander. Jedes Theme im Notion- oder Typora-Stil. Mermaid, Graphviz '
  'und KaTeX — abgestimmt und bei Bedarf geladen. 11 MB, kein gebündeltes Chromium.',
  'リッチ表示とソース表示は、キー一つ隣。Notion 風・Typora 風のテーマを自由に読み込める。Mermaid、Graphviz、KaTeX はすべて調整済みで、必要なときだけ読み込む。11 MB、Chromium は同梱しない。'),
 ("Highlight a claim, leave a question in the margin, fix the sentence right where it's wrong.",
  '高亮一句断言，在旁边留下你的疑问，就地把写错的句子改对。',
  'Markiere eine Behauptung, lass eine Frage am Rand, korrigiere den Satz genau dort, wo er falsch ist.',
  '主張をハイライトし、余白に疑問を残し、間違っている文をその場で直す。'),
 ('Claude, Codex and OpenClaw each have a chat window. None of them is a place to read.',
  'Claude、Codex、OpenClaw 各有各的对话窗口，但没有一个是「读」的地方。',
  'Claude, Codex und OpenClaw haben je ein Chatfenster. Keines davon ist ein Ort zum Lesen.',
  'Claude も Codex も OpenClaw もチャット窓を持っている。だが、どれも「読む」ための場所ではない。'),
 # ---- claim 02 ----
 ('<h2>Everything the last generation<br>got right, built in</h2>',
  '<h2>上一代笔记工具做对的事，<br>全都内置</h2>',
  '<h2>Alles, was die letzte Generation<br>richtig machte — eingebaut</h2>',
  '<h2>前の世代が正しかったことは、<br>すべて内蔵した</h2>'),
 ('local-first · git sync · outliner · [[wikilinks]] · backlinks · wiki pages · daily notes · plugins',
  'local-first · git 同步 · 大纲 · [[双链]] · 反向链接 · wiki 页面 · 每日笔记 · 插件',
  'local-first · Git-Sync · Outliner · [[Wikilinks]] · Backlinks · Wiki-Seiten · Tagesnotizen · Plugins',
  'local-first · git 同期 · アウトライナー · [[ウィキリンク]] · バックリンク · wiki ページ · デイリーノート · プラグイン'),
 ('Roam Research and Obsidian figured these out. note.md ships them on files: one plugin imports your whole Roam '
  'graph, and an Obsidian vault opens directly.',
  '这些是 Roam Research 和 Obsidian 想明白的事，note.md 把它们落在文件上：一个插件导入你整份 Roam 数据，Obsidian 的 vault 直接打开。',
  'Roam Research und Obsidian haben das herausgefunden. note.md liefert es auf Dateien: ein Plugin importiert deinen '
  'gesamten Roam-Graphen, und ein Obsidian-Vault öffnet sich direkt.',
  'Roam Research と Obsidian が見つけ出したこと。note.md はそれをファイルの上で提供する：プラグイン一つで Roam のグラフを丸ごと取り込み、Obsidian の vault はそのまま開く。'),
 # ---- claim 03 ----
 ('<h2>No AI inside.<br>Fully AI-native.</h2>',
  '<h2>它自己不带 AI。<br>它仍然是 AI-native。</h2>',
  '<h2>Keine KI eingebaut.<br>Trotzdem KI-nativ.</h2>',
  '<h2>AI は入っていない。<br>それでも AI ネイティブ。</h2>'),
 ('note.md calls no model and sends no request.',
  'note.md 不调模型、不发一个请求。',
  'note.md ruft kein Modell auf und sendet keine Anfrage.',
  'note.md はモデルを呼ばない。リクエストも送らない。'),
 ('Your vault is the shared, version-controlled context that many agents and harnesses work in — Cowork, Claude Code, '
  'Codex, ChatGPT Work, OpenClaw, Hermes — through public conventions any of them can read. '
  '<a href="/orchestrate-agents/">See how</a>.',
  '你的 vault 是多 agent、多 harness 共用的、受版本控制的上下文环境——Cowork、Claude Code、Codex、ChatGPT Work、OpenClaw、Hermes'
  '——它们通过公共约定读写同一批文件。<a href="/orchestrate-agents/">看怎么做</a>。',
  'Dein Vault ist der geteilte, versionierte Kontext, in dem viele Agents und Harnesses arbeiten — Cowork, Claude '
  'Code, Codex, ChatGPT Work, OpenClaw, Hermes — über öffentliche Konventionen, die jeder von ihnen lesen kann. '
  '<a href="/orchestrate-agents/">So geht\'s</a>.',
  'あなたの vault は、多くのエージェントとハーネスが共有する、バージョン管理されたコンテキスト——Cowork、Claude Code、Codex、'
  'ChatGPT Work、OpenClaw、Hermes——どれもが読める公開の約束事を通して。<a href="/orchestrate-agents/">やり方を見る</a>。'),
 ('Switch AI tools whenever you like. The asset stays yours.',
  '你随时可以换 AI 工具。资产始终在你手里。',
  'Wechsle die KI-Tools, wann du willst. Das Asset bleibt deins.',
  'AI ツールはいつ乗り換えてもいい。資産はあなたの手に残る。'),
 # ---- claim 04 ----
 ('<h2>Whatever else you need,<br>grow it yourself</h2>',
  '<h2>剩下的，<br>你自己长出来</h2>',
  '<h2>Was du sonst brauchst,<br>lässt du selbst wachsen</h2>',
  '<h2>ほかに必要なものは、<br>自分で生やせばいい</h2>'),
 ('Write a plugin. Wire an OpenClaw cron job. Hang skills off it.',
  '写个插件。配一条 OpenClaw 定时任务。挂上 skills。',
  'Schreib ein Plugin. Häng einen OpenClaw-Cronjob dran. Setz Skills obendrauf.',
  'プラグインを書く。OpenClaw の定期実行をつなぐ。skills をぶら下げる。'),
 ('Put a <span class="mono-s">?</span> in an annotation and an agent picks it up: it revises the document '
  'asynchronously, fills in the context you asked for, and hands it back for you to accept — or not.',
  '在批注里打一个 <span class="mono-s">?</span>，agent 就会接走：异步改这篇文档、补上你要的上下文，再交回来等你决定采不采纳。',
  'Setz ein <span class="mono-s">?</span> in eine Anmerkung, und ein Agent nimmt sie auf: Er überarbeitet das Dokument '
  'asynchron, ergänzt den gewünschten Kontext und gibt es dir zurück — annehmen oder nicht, entscheidest du.',
  '注釈に <span class="mono-s">?</span> を一つ置けば、エージェントが引き取る：非同期で文書を直し、頼んだ文脈を補い、採用するかどうかをあなたに委ねて返してくる。'),
 ('<div class="sec-k">The trick</div>',
  '<div class="sec-k">戏法</div>',
  '<div class="sec-k">Der Trick</div>',
  '<div class="sec-k">からくり</div>'),
 ("<h2>AI text is infinite.<br>Your attention isn't.</h2>",
  '<h2>AI 的文字无限。<br>你的注意力有限。</h2>',
  '<h2>KI-Text ist unendlich.<br>Deine Aufmerksamkeit nicht.</h2>',
  '<h2>AI のテキストは無限。<br>あなたの注意力は違う。</h2>'),
 ('Every document gets a shadow: a note file of its own. What the agent wrote and what you think, side by side — never '
  'tangled.',
  '每篇文档都有一个影子：属于它自己的笔记文件。agent 写的和你想的并排存放——永不纠缠。',
  'Jedes Dokument bekommt einen Schatten: eine eigene Notizdatei. Was der Agent schrieb und was du denkst, Seite an '
  'Seite — nie vermischt.',
  'すべてのドキュメントに影がひとつ：専用のノートファイル。エージェントが書いたものと、あなたが考えたこと。並んで、決して混ざらない。'),
 ('<span class="ft">The document. An agent wrote it, and can write it again tomorrow. Cheap, clean, '
  'replaceable.</span>',
  '<span class="ft">文档。agent 写的，明天还能再写一遍。便宜、干净、可替换。</span>',
  '<span class="ft">Das Dokument. Ein Agent hat es geschrieben und kann es morgen wieder schreiben. Billig, sauber, '
  'ersetzbar.</span>',
  '<span class="ft">ドキュメント。エージェントが書いた。明日また書ける。安く、クリーンで、置き換え可能。</span>'),
 ('<span class="ft">Your highlights, doubts, and questions — the one thing no model can generate.</span>',
  '<span class="ft">你的高亮、怀疑和问题——唯一没有模型能生成的东西。</span>',
  '<span class="ft">Deine Markierungen, Zweifel und Fragen — das Einzige, was kein Modell generieren kann.</span>',
  '<span class="ft">あなたのハイライト、疑問、問い——どのモデルにも生成できない唯一のもの。</span>'),
 ('Anyone can generate ten thousand words. No one can generate your opinion of them — <b>the rarest dataset in the '
  "world, and it's sitting on your disk.</b>",
  '现在谁都能生成一万字，但没人能生成你对这一万字的看法——<b>世界上最稀有的数据集，就躺在你的磁盘上。</b>',
  'Zehntausend Wörter kann jeder generieren. Deine Meinung dazu kann niemand generieren — <b>der seltenste Datensatz '
  'der Welt, und er liegt auf deiner Platte.</b>',
  '一万語なら誰でも生成できる。だが、それについてのあなたの意見は誰にも生成できない——<b>世界で最も希少なデータセットが、あなたのディスクに眠っている。</b>'),
 ('<span class="star">✦</span> what AI writes',
  '<span class="star">✦</span> AI 写的',
  '<span class="star">✦</span> was die KI schreibt',
  '<span class="star">✦</span> AI が書いたもの'),
 ('<span class="pt"></span> what you think',
  '<span class="pt"></span> 你想的',
  '<span class="pt"></span> was du denkst',
  '<span class="pt"></span> あなたの考え'),
 ('<h2>Own your thinking.</h2>', '<h2>拥有你的思考。</h2>', '<h2>Besitze dein Denken.</h2>', '<h2>思考を所有せよ。</h2>'),
 ("Free. Open. A folder of markdown on your Mac. That's the whole architecture.",
  '免费。开源。你 Mac 上的一个 markdown 文件夹。这就是全部架构。',
  'Frei. Offen. Ein Ordner voller Markdown auf deinem Mac. Das ist die ganze Architektur.',
  '無料。オープン。あなたの Mac にある markdown フォルダ。アーキテクチャはそれだけ。'),
 ('macOS 13 or later · Apple Silicon &amp; <a href="/download?arch=x86_64">Intel</a> · from GitHub Releases',
  'macOS 13 或更高 · Apple Silicon 与 <a href="/download?arch=x86_64">Intel</a> · 从 GitHub Releases 获取',
  'macOS 13 oder neuer · Apple Silicon &amp; <a href="/download?arch=x86_64">Intel</a> · von GitHub Releases',
  'macOS 13 以降 · Apple Silicon &amp; <a href="/download?arch=x86_64">Intel</a> · GitHub Releases から'),
 ('Developed and maintained entirely by AI coding — reviewed, tested and smoke-run by a human before every release.',
  '完全由 AI Coding 开发和维护——每次发布前都由人审阅、测试并实机验证。',
  'Vollständig per AI-Coding entwickelt und gepflegt — vor jedem Release von einem Menschen geprüft, getestet und '
  'live verifiziert.',
  '開発も保守も、すべて AI コーディングによる——リリースのたびに人がレビューし、テストし、実機で確認している。'),
 ("Text is cheap now. What you thought about it isn't.",
  '文字如今很廉价。你对它的看法不是。',
  'Text ist jetzt billig. Was du darüber dachtest, nicht.',
  'テキストはもう安い。あなたがそれについて考えたことは、そうではない。'),
 ('<b>Compare</b>', '<b>对比</b>', '<b>Vergleich</b>', '<b>比較</b>'),
 ('<b>Integrations</b>', '<b>集成</b>', '<b>Integrationen</b>', '<b>連携</b>'),
 ('<b>Guides</b>', '<b>指南</b>', '<b>Anleitungen</b>', '<b>ガイド</b>'),
 ('>Free sharing on Cloudflare</a>',
  '>Cloudflare 免费分享</a>',
  '>Kostenlos teilen über Cloudflare</a>',
  '>Cloudflare で無料共有</a>'),
 ('>Vault on GitHub</a>', '>GitHub 托管 vault</a>', '>Vault auf GitHub</a>', '>GitHub で Vault をホスト</a>')]

COL = {"zh": 1, "de": 2, "ja": 3}

SWITCH = {
    "de": [('<a href="/" class="on">EN</a>', '<a href="/">EN</a>'),
           ('<a href="/de/">DE</a>', '<a href="/de/" class="on">DE</a>')],
    "ja": [('<a href="/" class="on">EN</a>', '<a href="/">EN</a>'),
           ('<a href="/ja/">日本語</a>', '<a href="/ja/" class="on">日本語</a>')],
    "zh": [('<a href="/" class="on">EN</a>', '<a href="/">EN</a>'),
           ('<a href="/zh/">中文</a>', '<a href="/zh/" class="on">中文</a>')],
}

def build(lang):
    idx = COL[lang]
    src = open("public/index.html", encoding="utf-8").read()
    missing = []
    for row in STRINGS:
        en, target = row[0], row[idx]
        if en not in src:
            missing.append(en[:60]); continue
        src = src.replace(en, target)
    src = src.replace('<html lang="en">', f'<html lang="{lang}">')
    src = src.replace(f'<link rel="canonical" href="{BASE}/">', f'<link rel="canonical" href="{BASE}/{lang}/">')
    for old, new in SWITCH[lang]:
        if old not in src:
            missing.append(old[:60]); continue
        src = src.replace(old, new)
    for seg in ("compare", "integrations", "guides", "orchestrate-agents"):
        src = src.replace(f'href="/{seg}/', f'href="/{lang}/{seg}/')
    os.makedirs(f"public/{lang}", exist_ok=True)
    open(f"public/{lang}/index.html", "w", encoding="utf-8").write(src)
    print(f"public/{lang}/index.html written" + (f"  ⚠ {len(missing)} unmatched:" if missing else ""))
    for m in missing:
        print("   -", m)
    return not missing

ok = all([build("de"), build("ja"), build("zh")])
sys.exit(0 if ok else 1)
