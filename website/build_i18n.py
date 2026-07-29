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
 ('<h1>Read what AI writes.<br>Keep what <em>you</em> think.<span class="cursor"></span></h1>',
  '<h1>读 AI 写的，<br>留下<em>你想的</em>。<span class="cursor"></span></h1>',
  '<h1>Lies, was KI schreibt.<br>Behalte, was <em>du</em> denkst.<span class="cursor"></span></h1>',
  '<h1>読むのは AI の文。<br>残すのは<em>あなたの考え</em>。<span class="cursor"></span></h1>'),
 ('AI writes more than you can read. note.md is where you get through it: highlight, question, fix it on the spot. '
  'It all lands in the vault your agents share — plain markdown, and the more you leave, the better they know you.',
  'AI 写的，读不完。note.md 让你读得进去：划重点、写疑问、直接改。全部存进你和 agent 共享的 vault，纯 markdown——留得越多，agent 越懂你。',
  'KI schreibt mehr, als du lesen kannst. In note.md kommst du durch: markieren, nachfragen, direkt korrigieren. '
  'Alles landet im Vault, den deine Agents teilen — pures Markdown. Je mehr du hinterlässt, desto besser kennen sie dich.',
  'AI が書く量は、読み切れない。note.md なら読み進められる。ハイライトして、疑問を書いて、その場で直す。'
  'すべては agent と共有する vault に、ただの markdown として残る——残すほど、agent はあなたを理解していく。'),
 ('11 MB<i>·</i>any Typora theme<i>·</i>Mermaid &amp; Graphviz, tuned<i>·</i>outliner, [[wikilinks]], daily notes'
  '<i>·</i>one vault every agent shares',
  '11 MB<i>·</i>主题随你换<i>·</i>Mermaid、Graphviz 都调过<i>·</i>大纲、[[双链]]、每日笔记<i>·</i>一个 vault，所有 agent 共用',
  '11 MB<i>·</i>jedes Typora-Theme<i>·</i>Mermaid &amp; Graphviz, abgestimmt<i>·</i>Outliner, [[Wikilinks]], Tagesnotizen'
  '<i>·</i>ein Vault für alle Agents',
  '11 MB<i>·</i>Typora テーマ対応<i>·</i>Mermaid、Graphviz 調整済み<i>·</i>アウトライン、[[ウィキリンク]]、デイリーノート'
  '<i>·</i>一つの vault をすべての agent と'),
 ('<span class="bl">Download for macOS</span>',
  '<span class="bl">下载 macOS 版</span>',
  '<span class="bl">Für macOS laden</span>',
  '<span class="bl">macOS 版をダウンロード</span>'),
 ('<span class="bl">Star on GitHub</span>',
  '<span class="bl">GitHub 加星</span>',
  '<span class="bl">Auf GitHub sternen</span>',
  '<span class="bl">GitHub でスター</span>'),
 ('macOS 13+ · free &amp; open · your files stay on your own Mac · <a href="/download?arch=x86_64">Intel Mac?</a>',
  'macOS 13+ · 免费开源 · 文件都在你自己电脑上 · <a href="/download?arch=x86_64">Intel 芯片 Mac？</a>',
  'macOS 13+ · frei &amp; offen · deine Dateien bleiben auf deinem Mac · <a href="/download?arch=x86_64">Intel-Mac?</a>',
  'macOS 13+ · 無料＆オープン · ファイルはあなたの Mac の中に · <a href="/download?arch=x86_64">Intel Mac は？</a>'),
 ('<div class="sec-k">Four things</div>',
  '<div class="sec-k">四件事</div>',
  '<div class="sec-k">Vier Dinge</div>',
  '<div class="sec-k">四つのこと</div>'),
 # ---- claim 01 ----
 ('<h2>AI handles the writing.<br>This handles the reading.</h2>',
  '<h2>写，交给 AI。<br>读，交给这里。</h2>',
  '<h2>Die KI schreibt.<br>Hier liest du.</h2>',
  '<h2>書くのは AI。<br>読むのは、ここ。</h2>'),
 ('Preview and source, one key apart. Notion and Typora themes work as they are. Mermaid, Graphviz and math, all '
  'tuned. The whole app is 11 MB, with no browser engine inside.',
  '预览与源码，一键之隔。Notion、Typora 的主题，拿来就用。Mermaid、Graphviz、公式，都调过。整个应用 11 MB，没有浏览器内核。',
  'Vorschau und Quelltext, eine Taste auseinander. Themes von Notion und Typora laufen, wie sie sind. Mermaid, '
  'Graphviz und Formeln — alles abgestimmt. Die ganze App wiegt 11 MB, ohne Browser-Engine im Bauch.',
  'プレビューとソースは、キー一つ隣。Notion や Typora のテーマは、そのまま使える。Mermaid も Graphviz も数式も、調整済み。アプリ全体で 11 MB、ブラウザエンジンは入っていない。'),
 ('A shaky line? Highlight it. A doubt? In the margin. Wrong? Fix it on the spot.',
  '可疑的句子，划出来。疑问，写在旁边。错了，当场改。',
  'Eine wacklige Zeile? Markieren. Ein Zweifel? An den Rand. Falsch? Sofort korrigieren.',
  '怪しい一文は、ハイライト。疑問は、余白に。間違いは、その場で。'),
 ('Claude, Codex and OpenClaw all have a chat window. None of them is made for reading.',
  'Claude、Codex、OpenClaw 都有聊天窗口。没有一个是为读而做的。',
  'Claude, Codex und OpenClaw haben alle ein Chatfenster. Keines davon ist zum Lesen gemacht.',
  'Claude も Codex も OpenClaw も、チャット窓は持っている。でも、読むためのものではない。'),
 # ---- claim 02 ----
 ('<h2>The best of the last<br>generation. Built in.</h2>',
  '<h2>上一代的精华，<br>全都内置。</h2>',
  '<h2>Das Beste der letzten<br>Generation. Eingebaut.</h2>',
  '<h2>前世代の良いところは、<br>全部内蔵。</h2>'),
 ('local-first · git sync · outliner · [[wikilinks]] · backlinks · wiki pages · daily notes · plugins',
  '本地优先 · git 同步 · 大纲 · [[双链]] · 反向链接 · wiki 页面 · 每日笔记 · 插件',
  'local-first · Git-Sync · Outliner · [[Wikilinks]] · Backlinks · Wiki-Seiten · Tagesnotizen · Plugins',
  'ローカル優先 · git 同期 · アウトライナー · [[ウィキリンク]] · バックリンク · wiki ページ · デイリーノート · プラグイン'),
 ('Roam and Obsidian worked this out years ago. note.md puts it back into files. One plugin brings your '
  'whole Roam graph over. An Obsidian vault just opens.',
  '这些事，Roam 和 Obsidian 早就想明白了。note.md 把它们放回文件里。一个插件，搬来整个 Roam 图谱；Obsidian 的 vault，直接打开。',
  'Roam und Obsidian hatten das vor Jahren heraus. note.md legt es zurück in Dateien. Ein Plugin holt deinen '
  'ganzen Roam-Graphen herüber. Ein Obsidian-Vault geht einfach auf.',
  'Roam と Obsidian は、とっくに答えを出していた。note.md はそれをファイルに戻す。プラグイン一つで Roam のグラフを丸ごと移せる。Obsidian の vault は、そのまま開く。'),
 # ---- claim 03 ----
 ('<h2>No AI inside.<br>Made for AI.</h2>',
  '<h2>不带 AI。<br>为 AI 而生。</h2>',
  '<h2>Keine KI eingebaut.<br>Für KI gemacht.</h2>',
  '<h2>AI は入っていない。<br>AI のために作られている。</h2>'),
 ('Calls no model. Sends no request. Not one.',
  '不连模型，不发请求。一个都没有。',
  'Ruft kein Modell auf. Sendet keine Anfrage. Keine einzige.',
  'モデルを呼ばない。リクエストも送らない。一つも。'),
 ('Your folder is the workspace every AI tool shares. Cowork, Claude Code, Codex, ChatGPT, OpenClaw, Hermes — same '
  'files, same house rules, all in git. '
  '<a href="/orchestrate-agents/">See how</a>.',
  '你的文件夹，就是所有 AI 工具共用的工作台。Cowork、Claude Code、Codex、ChatGPT、OpenClaw、Hermes——同一批文件，同一套规矩，'
  '全在 git 里。<a href="/orchestrate-agents/">看怎么做</a>。',
  'Dein Ordner ist der Arbeitsplatz, den alle KI-Tools teilen. Cowork, Claude Code, Codex, ChatGPT, OpenClaw, Hermes '
  '— dieselben Dateien, dieselben Hausregeln, alles in Git. '
  '<a href="/orchestrate-agents/">So geht\'s</a>.',
  'あなたのフォルダは、どの AI ツールも共有する作業台。Cowork、Claude Code、Codex、ChatGPT、OpenClaw、Hermes——同じファイル、'
  '同じルール、すべて git の中。<a href="/orchestrate-agents/">やり方を見る</a>。'),
 ("Change AI tools whenever you want. What's yours stays yours.",
  '换 AI 工具，随时。你的东西，永远是你的。',
  'Wechsle das KI-Tool, wann du willst. Was deins ist, bleibt deins.',
  'AI ツールは、いつ変えてもいい。あなたのものは、あなたのまま。'),
 # ---- claim 04 ----
 ('<h2>Need something else?<br>Add it.</h2>',
  '<h2>还要什么，<br>自己加。</h2>',
  '<h2>Brauchst du mehr?<br>Bau es dazu.</h2>',
  '<h2>ほかに欲しいものは、<br>自分で足す。</h2>'),
 ('Write a plugin. Add a scheduled job. Hang your skills off it.',
  '写个插件。加个定时任务。挂上你的 skills。',
  'Schreib ein Plugin. Häng einen Cronjob dran. Setz deine Skills obendrauf.',
  'プラグインを書く。定期実行を足す。skills をぶら下げる。'),
 ('Put a <span class="mono-s">?</span> in a note and an agent takes it from there: edits the document, fills in '
  'the context, hands it back — async. Whether you keep it is up to you.',
  '在批注里打个 <span class="mono-s">?</span>，agent 就接手：改文档、补上下文，异步交回来。用不用，你说了算。',
  'Setz ein <span class="mono-s">?</span> in eine Notiz, und ein Agent übernimmt: überarbeitet das Dokument, ergänzt '
  'den Kontext, gibt es zurück — asynchron. Ob du es nimmst, entscheidest du.',
  '注釈に <span class="mono-s">?</span> を置けば、agent が引き取る。文書を直し、文脈を補い、非同期で返してくる。使うかどうかは、あなたが決める。'),
 ('<div class="sec-k">The trick</div>',
  '<div class="sec-k">关键</div>',
  '<div class="sec-k">Der Trick</div>',
  '<div class="sec-k">仕組み</div>'),
 ("<h2>AI text is infinite.<br>Your attention isn't.</h2>",
  '<h2>AI 的文字无限。<br>你的注意力有限。</h2>',
  '<h2>KI-Text ist unendlich.<br>Deine Aufmerksamkeit nicht.</h2>',
  '<h2>AI のテキストは無限。<br>あなたの注意力は違う。</h2>'),
 ('Every document gets a note file of its own. What the AI wrote stays on one side. What you think stays on the other.',
  '每篇文档，配一个笔记文件。AI 写的归一边，你想的归另一边。',
  'Jedes Dokument bekommt seine eigene Notizdatei. Was die KI schrieb, bleibt auf der einen Seite. Was du denkst, auf '
  'der anderen.',
  'どの文書にも、専用のノートファイルがつく。AI が書いたものはこちら。あなたが考えたことはあちら。'),
 ('<span class="ft">The document. An AI wrote it, and can write it again tomorrow.</span>',
  '<span class="ft">文档。AI 写的，明天还能再写一份。</span>',
  '<span class="ft">Das Dokument. Eine KI hat es geschrieben und kann es morgen neu schreiben.</span>',
  '<span class="ft">ドキュメント。AI が書いた。明日また書ける。</span>'),
 ('<span class="ft">Your highlights, doubts and questions. This part, no model can write.</span>',
  '<span class="ft">你的高亮、疑问和判断。这部分，AI 写不出来。</span>',
  '<span class="ft">Deine Markierungen, Zweifel und Fragen. Diesen Teil kann kein Modell schreiben.</span>',
  '<span class="ft">あなたのハイライト、疑問、問い。ここだけは、どのモデルにも書けない。</span>'),
 ('Ten thousand words? Anyone can generate those. Your take, nobody can. <b>And it\'s sitting on your '
  'Mac.</b>',
  '一万字，谁都能生成。你的看法，谁也生成不了。<b>而它就在你自己的 Mac 里。</b>',
  'Zehntausend Wörter? Kann jeder generieren. Deine Sicht, niemand. <b>Und sie liegt auf deinem Mac.</b>',
  '一万語なら、誰でも生成できる。あなたの見方は、誰にも。<b>そしてそれは、あなたの Mac の中に。</b>'),
 ('<span class="star">✦</span> what AI writes',
  '<span class="star">✦</span> AI 写的',
  '<span class="star">✦</span> was die KI schreibt',
  '<span class="star">✦</span> AI が書いたもの'),
 ('<span class="pt"></span> what you think',
  '<span class="pt"></span> 你想的',
  '<span class="pt"></span> was du denkst',
  '<span class="pt"></span> あなたの考え'),
 ('<h2>Own your thinking.</h2>', '<h2>拥有你的思考。</h2>', '<h2>Besitze dein Denken.</h2>', '<h2>思考を所有せよ。</h2>'),
 ("Free. Open. A folder on your Mac. That's it.",
  '免费。开源。就是你 Mac 上的一个文件夹。',
  'Kostenlos. Offen. Ein Ordner auf deinem Mac. Mehr nicht.',
  '無料。オープン。あなたの Mac にあるフォルダ一つ。それだけ。'),
 ('macOS 13 or later · Apple Silicon &amp; <a href="/download?arch=x86_64">Intel</a> · from GitHub Releases',
  'macOS 13 或更高 · Apple Silicon 与 <a href="/download?arch=x86_64">Intel</a> · 从 GitHub Releases 获取',
  'macOS 13 oder neuer · Apple Silicon &amp; <a href="/download?arch=x86_64">Intel</a> · von GitHub Releases',
  'macOS 13 以降 · Apple Silicon &amp; <a href="/download?arch=x86_64">Intel</a> · GitHub Releases から'),
 ('Written and maintained entirely by AI coding. Reviewed and tested by a human before every release.',
  '代码全部由 AI 写，也由 AI 维护。每次发布前，都有人亲自审、亲自测。',
  'Vollständig per AI-Coding geschrieben und gepflegt. Vor jedem Release von einem Menschen geprüft und getestet.',
  'コードはすべて AI が書き、AI が保守する。リリースのたびに、人がレビューしてテストしている。'),
 ("Text is cheap now. What you thought about it isn't.",
  '文字现在很便宜。你的看法不便宜。',
  'Text ist jetzt billig. Was du darüber dachtest, nicht.',
  'テキストはもう安い。それをどう思ったかは、安くない。'),
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

# zh headings are set in Source Han Serif; only the zh page pays for the webfont.
FONTS = {
    "zh": [('&family=Courier+Prime:ital,wght@0,400;0,700;1,400&display=swap',
            '&family=Courier+Prime:ital,wght@0,400;0,700;1,400&family=Noto+Serif+SC:wght@700&display=swap')],
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
    for old, new in SWITCH[lang] + FONTS.get(lang, []):
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
