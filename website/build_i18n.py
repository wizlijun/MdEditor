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
 ('<h1>One night of AI.<br>A <em>year</em> of reading.<span class="cursor"></span></h1>',
  '<h1>AI 一晚写的，<br>你<em>一年</em>也读不完。<span class="cursor"></span></h1>',
  '<h1>Eine Nacht KI.<br>Ein <em>Jahr</em> Lesestoff.<span class="cursor"></span></h1>',
  '<h1>AI が一晩で書く量は、<br><em>一年</em>でも読み切れない。<span class="cursor"></span></h1>'),
 ('note.md is where you get through it: highlight, question, fix it on the spot. What you save is a plain markdown '
  'file, on your own Mac.',
  'note.md 让你读得进去：划重点、写疑问、直接改。存下来的都是普通 markdown 文件，在你自己电脑上。',
  'In note.md kommst du wirklich durch: markieren, nachfragen, direkt korrigieren. Was du behältst, ist eine '
  'einfache Markdown-Datei auf deinem eigenen Mac.',
  'note.md なら、読み進められる。ハイライトして、疑問を書いて、その場で直す。残るのは、あなたの Mac の中のただの markdown ファイル。'),
 ('11 MB<i>·</i>any Typora theme<i>·</i>Mermaid &amp; Graphviz<i>·</i>outliner, [[links]], daily notes'
  '<i>·</i>one folder every agent shares',
  '11 MB<i>·</i>主题随你换<i>·</i>Mermaid、Graphviz<i>·</i>大纲、[[双链]]、每日笔记<i>·</i>所有 agent 共用一个文件夹',
  '11 MB<i>·</i>jedes Typora-Theme<i>·</i>Mermaid &amp; Graphviz<i>·</i>Outliner, [[Links]], Tagesnotizen'
  '<i>·</i>ein Ordner für alle Agents',
  '11 MB<i>·</i>Typora テーマ対応<i>·</i>Mermaid、Graphviz<i>·</i>アウトライン、[[リンク]]、デイリーノート'
  '<i>·</i>すべての agent が同じフォルダ'),
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
 ('<h2>AI wrote it.<br>This is where you read it.</h2>',
  '<h2>AI 写的东西，<br>在这儿读最舒服。</h2>',
  '<h2>Die KI hat es geschrieben.<br>Hier liest du es.</h2>',
  '<h2>書いたのは AI。<br>読むのは、ここで。</h2>'),
 ('Preview and source, one key apart. Notion and Typora themes work as they are. Mermaid, Graphviz and math, all '
  'tuned. The whole app is 11 MB, with no browser engine inside.',
  '预览和源码，一键切换。Notion、Typora 的主题，拿来就能用。Mermaid、Graphviz、公式，都调过。整个应用 11 MB，不带浏览器内核。',
  'Vorschau und Quelltext, eine Taste auseinander. Themes von Notion und Typora laufen, wie sie sind. Mermaid, '
  'Graphviz und Formeln — alles abgestimmt. Die ganze App wiegt 11 MB, ohne Browser-Engine im Bauch.',
  'プレビューとソースは、キー一つ隣。Notion や Typora のテーマは、そのまま使える。Mermaid も Graphviz も数式も、調整済み。アプリ全体で 11 MB、ブラウザエンジンは入っていない。'),
 ("See a shaky line? Highlight it. Have a doubt? Write it in the margin. Something's wrong? Fix it right there.",
  '看到可疑的一句，划出来。有疑问，写在旁边。写错了，直接改。',
  'Eine wacklige Zeile? Markieren. Ein Zweifel? An den Rand schreiben. Etwas falsch? Direkt korrigieren.',
  '怪しい一文は、ハイライト。疑問は、余白に。間違いは、その場で直す。'),
 ('Claude, Codex and OpenClaw all have a chat window. None of them is made for reading.',
  'Claude、Codex、OpenClaw 都有自己的聊天窗口。但没有一个是用来读的。',
  'Claude, Codex und OpenClaw haben alle ein Chatfenster. Keines davon ist zum Lesen gemacht.',
  'Claude も Codex も OpenClaw も、チャット窓は持っている。でも、読むためのものではない。'),
 # ---- claim 02 ----
 ('<h2>The good parts of the last<br>generation. All here.</h2>',
  '<h2>上一代笔记工具的好东西，<br>这里都有。</h2>',
  '<h2>Das Beste der letzten<br>Generation. Alles hier.</h2>',
  '<h2>前の世代の良かったところは、<br>全部ここに。</h2>'),
 ('local-first · git sync · outliner · [[wikilinks]] · backlinks · wiki pages · daily notes · plugins',
  '本地优先 · git 同步 · 大纲 · [[双链]] · 反向链接 · wiki 页面 · 每日笔记 · 插件',
  'local-first · Git-Sync · Outliner · [[Wikilinks]] · Backlinks · Wiki-Seiten · Tagesnotizen · Plugins',
  'ローカル優先 · git 同期 · アウトライナー · [[ウィキリンク]] · バックリンク · wiki ページ · デイリーノート · プラグイン'),
 ('Roam Research and Obsidian worked this out years ago. note.md puts it back into files. One plugin brings your '
  'whole Roam graph over. An Obsidian folder just opens.',
  '这些事，Roam Research 和 Obsidian 早就想明白了。note.md 把它们放回文件里。装个插件，Roam 的笔记全搬过来。Obsidian 的文件夹，打开就能用。',
  'Roam Research und Obsidian hatten das vor Jahren heraus. note.md legt es zurück in Dateien. Ein Plugin holt deinen '
  'ganzen Roam-Graphen herüber. Ein Obsidian-Ordner geht einfach auf.',
  'Roam Research と Obsidian は、とっくに答えを出していた。note.md はそれをファイルに戻す。プラグイン一つで、Roam のノートは丸ごと移せる。Obsidian のフォルダは、そのまま開く。'),
 # ---- claim 03 ----
 ('<h2>No AI inside.<br>Made for AI.</h2>',
  '<h2>它自己不带 AI。<br>但它为 AI 而生。</h2>',
  '<h2>Keine KI eingebaut.<br>Für KI gemacht.</h2>',
  '<h2>AI は入っていない。<br>AI のために作られている。</h2>'),
 ('It calls no model. It sends no request. Not one.',
  '它不连模型，也不往外发一个请求。',
  'Es ruft kein Modell auf. Es sendet keine Anfrage. Keine einzige.',
  'モデルを呼ばない。リクエストも送らない。一つも。'),
 ('Your folder is the workspace every AI tool shares. Cowork, Claude Code, Codex, ChatGPT, OpenClaw, Hermes — same '
  'files, same house rules. It all lives in git, so you can see what changed. '
  '<a href="/orchestrate-agents/">See how</a>.',
  '你的文件夹，就是所有 AI 工具共用的工作台。Cowork、Claude Code、Codex、ChatGPT、OpenClaw、Hermes，读的是同一批文件，守的是同一套规矩。'
  '全都在 git 里，谁改了什么，一看就知道。<a href="/orchestrate-agents/">看怎么做</a>。',
  'Dein Ordner ist der Arbeitsplatz, den alle KI-Tools teilen. Cowork, Claude Code, Codex, ChatGPT, OpenClaw, Hermes '
  '— dieselben Dateien, dieselben Hausregeln. Alles liegt in Git, du siehst jede Änderung. '
  '<a href="/orchestrate-agents/">So geht\'s</a>.',
  'あなたのフォルダは、どの AI ツールも共有する作業台。Cowork、Claude Code、Codex、ChatGPT、OpenClaw、Hermes——同じファイルを読み、'
  '同じルールを守る。すべて git の中だから、何が変わったかすぐわかる。<a href="/orchestrate-agents/">やり方を見る</a>。'),
 ('Change AI tools whenever you want. Your work stays put, and stays yours.',
  '换 AI 工具，随时。东西还在原地，还是你的。',
  'Wechsle das KI-Tool, wann du willst. Deine Arbeit bleibt, wo sie ist — und bleibt deine.',
  'AI ツールは、いつ変えてもいい。中身はそのまま、あなたのもの。'),
 # ---- claim 04 ----
 ('<h2>Need something else?<br>Add it.</h2>',
  '<h2>还想要什么，<br>自己加。</h2>',
  '<h2>Brauchst du mehr?<br>Bau es dazu.</h2>',
  '<h2>ほかに欲しいものは、<br>自分で足す。</h2>'),
 ('Write a plugin. Add a scheduled job. Hang your skills off it.',
  '写个插件。加个定时任务。挂上你的 skills。',
  'Schreib ein Plugin. Häng einen Cronjob dran. Setz deine Skills obendrauf.',
  'プラグインを書く。定期実行を足す。skills をぶら下げる。'),
 ('Put a <span class="mono-s">?</span> in a note and the AI takes it from there. It edits the document, fills in what '
  'you asked for, and hands it back. Whether you use it is up to you.',
  '在批注里打个 <span class="mono-s">?</span>，AI 就接手了。它去改文档、补上你要的资料，改完交回来。用不用，你说了算。',
  'Setz ein <span class="mono-s">?</span> in eine Notiz, und die KI übernimmt. Sie überarbeitet das Dokument, ergänzt, '
  'worum du gebeten hast, und gibt es zurück. Ob du es nimmst, entscheidest du.',
  '注釈に <span class="mono-s">?</span> を置けば、あとは AI が引き取る。文書を直し、頼んだことを補い、返してくる。使うかどうかは、あなたが決める。'),
 ('<div class="sec-k">The trick</div>',
  '<div class="sec-k">关键</div>',
  '<div class="sec-k">Der Trick</div>',
  '<div class="sec-k">仕組み</div>'),
 ("<h2>AI text is infinite.<br>Your attention isn't.</h2>",
  '<h2>AI 的文字无限。<br>你的注意力有限。</h2>',
  '<h2>KI-Text ist unendlich.<br>Deine Aufmerksamkeit nicht.</h2>',
  '<h2>AI のテキストは無限。<br>あなたの注意力は違う。</h2>'),
 ('Every document gets a note file of its own. What the AI wrote stays on one side. What you think stays on the other.',
  '每篇文档都配一个笔记文件。AI 写的归 AI，你想的归你，各存各的。',
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
 ('Ten thousand words? Anyone can generate those. What you think of them, nobody can. <b>And it\'s sitting on your '
  'Mac.</b>',
  '一万字，谁都能生成。你对这一万字的看法，谁也生成不了。<b>而它就在你自己电脑里。</b>',
  'Zehntausend Wörter? Kann jeder generieren. Was du davon hältst, niemand. <b>Und es liegt auf deinem Mac.</b>',
  '一万語なら、誰でも生成できる。それをどう思うかは、誰にも生成できない。<b>そしてそれは、あなたの Mac の中にある。</b>'),
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
