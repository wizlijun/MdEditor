#!/usr/bin/env python3
"""Generate SEO landing pages (compare / integrations / guides) into public/.

Usage: python3 build_pages.py   (run inside website/)
Edit PAGES below, re-run. Every page is a self-contained static HTML file.
"""
import os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "i18n"))
import pages_de, pages_ja, pages_zh

BASE = "https://notemd.net"

CSS = """
:root{--ink:#17181C;--paper:#FAFAF7;--amber:#F59E0B;--gray:#9CA3AF;--line:#E7E5E0;
--serif:"Playfair Display",Georgia,serif;--body:"EB Garamond",Georgia,serif;
--mono:"Courier Prime","Courier New",monospace;}
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:var(--body);background:var(--paper);color:var(--ink);line-height:1.7;font-size:17.5px;-webkit-font-smoothing:antialiased}
a{color:inherit}
.wrap{max-width:880px;margin:0 auto;padding:0 28px}
nav{position:sticky;top:0;z-index:50;background:rgba(23,24,28,.9);backdrop-filter:blur(12px);border-bottom:1px solid #26282F}
.nav-in{display:flex;align-items:center;gap:24px;height:60px;color:var(--paper)}
.logo{display:flex;align-items:center;gap:10px;font-weight:700;font-size:16px;font-family:var(--mono);text-decoration:none}
.logo .dot{color:var(--amber)}
.nav-cta{font-family:var(--mono);background:var(--amber);color:var(--ink);font-weight:700;font-size:13px;padding:7px 16px;border-radius:8px;text-decoration:none}
.npl{font-family:var(--mono);font-size:13px;color:#B9BDC7;text-decoration:none;border-bottom:1px dotted transparent;padding-bottom:2px}
.npl:hover{color:#fff;border-bottom-color:var(--amber)}
header.ph{background:var(--ink);color:var(--paper);padding:72px 0 64px}
.crumb{font-family:var(--mono);font-size:12px;letter-spacing:.18em;text-transform:uppercase;color:var(--amber);margin-bottom:20px}
h1{font-family:var(--serif);font-size:42px;line-height:1.15;font-weight:700;margin-bottom:18px}
.lead{font-size:19px;color:#C3C7CF;font-style:italic;max-width:640px}
main{padding:56px 0 24px}
h2{font-family:var(--serif);font-size:28px;margin:44px 0 14px;font-weight:700}
p{margin:0 0 16px;color:#33363D}
li{margin:0 0 10px;color:#33363D}
ul,ol{padding-left:24px;margin:0 0 18px}
table{width:100%;border-collapse:collapse;margin:26px 0;font-size:16px}
th,td{border:1px solid var(--line);padding:12px 14px;text-align:left;vertical-align:top}
th{font-family:var(--mono);font-size:13px;letter-spacing:.04em;background:#fff}
td:first-child{font-weight:600;width:22%}
tr td:nth-child(2){background:#FFF9EE}
code,.mono{font-family:var(--mono);font-size:.92em}
pre{background:var(--ink);color:#E8E9EB;padding:18px 20px;border-radius:10px;overflow-x:auto;margin:0 0 18px;font-size:14px;line-height:1.6}
pre code{color:inherit}
.faq h3{font-family:var(--serif);font-size:19px;margin:22px 0 6px}
.cta{background:var(--ink);color:var(--paper);text-align:center;padding:64px 0;margin-top:64px}
.cta h2{margin:0 0 10px;font-style:italic}
.cta p{color:#C3C7CF}
.btn{display:inline-block;margin-top:18px;background:var(--amber);color:var(--ink);font-family:var(--mono);font-weight:700;font-size:14.5px;padding:13px 26px;border-radius:10px;text-decoration:none}
footer{background:var(--ink);color:#7C8290;font-size:13.5px;padding:34px 0 44px}
.flinks{display:grid;grid-template-columns:repeat(3,1fr);gap:20px;margin-bottom:26px}
.flinks b{display:block;font-family:var(--mono);font-size:11px;letter-spacing:.16em;text-transform:uppercase;color:#9CA3AF;margin-bottom:8px}
.flinks a{display:block;color:#B9BDC7;text-decoration:none;margin-bottom:5px}
.flinks a:hover{color:#fff}
.fbase{border-top:1px solid #26282F;padding-top:18px;font-family:var(--mono);font-size:12.5px}
html[lang="zh"]{--serif:"Playfair Display","Songti SC","Noto Serif SC",STSong,serif;--body:"EB Garamond","Songti SC","Noto Serif SC",STSong,serif}
html[lang="zh"] .lead{font-style:normal}
html[lang="ja"]{--serif:"Playfair Display","Hiragino Mincho ProN","Noto Serif JP",serif;--body:"EB Garamond","Hiragino Mincho ProN","Noto Serif JP",serif}
html[lang="ja"] .lead{font-style:normal}
.lang-sw{margin-left:auto;display:flex;gap:12px;font-family:var(--mono);font-size:12px;color:#7C8290}
.lang-sw a{text-decoration:none;padding-bottom:2px;border-bottom:1px dotted transparent}
.lang-sw a:hover{color:#fff}
.lang-sw a.on{color:var(--amber);border-bottom-color:var(--amber)}
@media(max-width:720px){
h1{font-size:28px}
h2{font-size:22px}
.flinks{grid-template-columns:1fr}
.nav-cta{display:none}
.nav-in{gap:12px;height:54px}
.logo{font-size:15px}
.lang-sw{gap:9px;font-size:11.5px;white-space:nowrap}
header.ph{padding:48px 0 42px}
.lead{font-size:17px}
main{padding:40px 0 16px}
table{display:block;overflow-x:auto}
td:first-child{min-width:110px}
td,th{padding:10px 11px;font-size:14.5px}
pre{font-size:12.5px;padding:14px 15px}
.cta{padding:48px 0;margin-top:48px}
.btn{width:calc(100% - 56px)}
}
"""

LOGO_SVG = '<svg width="24" height="24" viewBox="0 0 512 512" aria-hidden="true"><rect width="512" height="512" rx="115" fill="#17181C" stroke="#3A3D46" stroke-width="14"/><path d="M 185.49318,76.468676 C 202.86539,165.0158 220.23759,183.99019 301.30788,202.96457 220.23759,221.93895 202.86539,240.91333 185.49318,329.46046 168.12097,240.91333 150.74877,221.93895 69.67847,202.96457 150.74877,183.99019 168.12097,165.0158 185.49318,76.468676 Z" fill="#F59E0B"/><rect x="260.643" y="239.444" width="186.136" height="48" rx="29.39" fill="#FAFAF7"/><circle cx="289.722" cy="342.122" r="28.414" fill="#9CA3AF"/><rect x="333.7" y="324.65" width="112.797" height="40" rx="26.856" fill="#9CA3AF"/><circle cx="288.251" cy="420.101" r="28.414" fill="#9CA3AF"/><rect x="336.022" y="403.894" width="109.256" height="40" rx="26.013" fill="#9CA3AF"/></svg>'

LANG_ORDER = ["en", "de", "ja", "zh"]
LANG_LABEL = {"en": "EN", "de": "DE", "ja": "日本語", "zh": "中文"}

CHROME = {
 "en": {"dl": "Download", "pl": "plugins", "cta_h2": "Own your thinking.", "cta_p": "Free. Open. A folder of markdown on your Mac.",
        "cta_btn": "Download for macOS", "faq": "FAQ",
        "g_cmp": "Compare", "g_int": "Integrations", "g_gui": "Guides",
        "l_orch": "One vault, many agents", "l_cf": "Free sharing on Cloudflare", "l_gh": "Vault on GitHub", "l_llm": "llms.txt (for agents)",
        "sig": "Text is forever. So is what you thought about it."},
 "de": {"dl": "Laden", "pl": "plugins", "cta_h2": "Besitze dein Denken.", "cta_p": "Frei. Offen. Ein Ordner voller Markdown auf deinem Mac.",
        "cta_btn": "Für macOS laden", "faq": "FAQ",
        "g_cmp": "Vergleich", "g_int": "Integrationen", "g_gui": "Anleitungen",
        "l_orch": "Ein Vault, viele Agents", "l_cf": "Kostenlos teilen über Cloudflare", "l_gh": "Vault auf GitHub", "l_llm": "llms.txt (für Agents)",
        "sig": "Text ist für immer. Was du darüber dachtest, auch."},
 "ja": {"dl": "ダウンロード", "pl": "プラグイン", "cta_h2": "思考を所有せよ。", "cta_p": "無料。オープン。あなたの Mac にある markdown フォルダ。",
        "cta_btn": "macOS 版をダウンロード", "faq": "FAQ",
        "g_cmp": "比較", "g_int": "連携", "g_gui": "ガイド",
        "l_orch": "ひとつの Vault、多くのエージェント", "l_cf": "Cloudflare で無料共有", "l_gh": "GitHub で Vault をホスト", "l_llm": "llms.txt（エージェント向け）",
        "sig": "テキストは永遠に残る。あなたがそれについて考えたことも。"},
 "zh": {"dl": "下载", "pl": "插件", "cta_h2": "拥有你的思考。", "cta_p": "免费。开源。你 Mac 上的一个 markdown 文件夹。",
        "cta_btn": "下载 macOS 版", "faq": "FAQ",
        "g_cmp": "对比", "g_int": "集成", "g_gui": "指南",
        "l_orch": "一个 vault，多个 agent", "l_cf": "Cloudflare 免费分享", "l_gh": "GitHub 托管 vault", "l_llm": "llms.txt（给 agent）",
        "sig": "文字永存。你对它的看法也是。"},
}

def lp(lang, path):
    return path if lang == "en" else "/" + lang + path

def switcher(lang, path):
    links = "".join(
        f'<a href="{lp(l, path)}"{" class=on" if l == lang else ""}>{LANG_LABEL[l]}</a>'
        for l in LANG_ORDER)
    return f'<div class="lang-sw">{links}</div>'

def hreflangs(path):
    lines = [f'<link rel="alternate" hreflang="{l}" href="{BASE}{lp(l, path)}">' for l in LANG_ORDER]
    lines.append(f'<link rel="alternate" hreflang="x-default" href="{BASE}{path}">')
    return "\n".join(lines)

def foot_links(lang):
    c = CHROME[lang]
    def a(path, label):
        return f'<a href="{lp(lang, path)}">{label}</a>'
    return f"""<div class="flinks">
<div><b>{c['g_cmp']}</b>
{a('/compare/roam-research/', 'note.md vs Roam Research')}
{a('/compare/obsidian/', 'note.md vs Obsidian')}
{a('/compare/notion/', 'note.md vs Notion')}</div>
<div><b>{c['g_int']}</b>
{a('/integrations/openclaw/', 'OpenClaw')}
{a('/integrations/cowork/', 'Claude Cowork')}
{a('/integrations/codex/', 'Codex')}
{a('/integrations/chatgpt-work/', 'ChatGPT Work')}
{a('/integrations/hermes/', 'Hermes')}</div>
<div><b>{c['g_gui']}</b>
{a('/orchestrate-agents/', c['l_orch'])}
{a('/guides/share-on-cloudflare/', c['l_cf'])}
{a('/guides/vault-on-github/', c['l_gh'])}
<a href="/llms.txt">{c['l_llm']}</a></div>
</div>"""

def faq_jsonld(faq):
    import json
    return json.dumps({
        "@context": "https://schema.org", "@type": "FAQPage",
        "mainEntity": [{"@type": "Question", "name": q,
                        "acceptedAnswer": {"@type": "Answer", "text": a}} for q, a, *_ in faq]
    }, ensure_ascii=False)

def render(p, lang):
    c = CHROME[lang]
    faq_html = ""
    jsonld = ""
    if p.get("faq"):
        items = "".join(f"<h3>{q}</h3><p>{a}</p>" for q, a in p["faq"])
        faq_html = f'<section class="faq"><h2>{c["faq"]}</h2>{items}</section>'
        jsonld = f'<script type="application/ld+json">{faq_jsonld(p["faq"])}</script>'
    table_html = ""
    if p.get("table"):
        head = "".join(f"<th>{h}</th>" for h in p["table"]["head"])
        rows = "".join("<tr>" + "".join(f"<td>{cell}</td>" for cell in r) + "</tr>" for r in p["table"]["rows"])
        table_html = f"<table><thead><tr>{head}</tr></thead><tbody>{rows}</tbody></table>"
    body_html = "".join(f"<h2>{h}</h2>{html}" for h, html in p["sections"])
    return f"""<!DOCTYPE html>
<html lang="{lang}">
<head>
<meta charset="UTF-8">
<!-- Hello, agent. The plain-text version of this site is at /llms.txt -->
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{p['title']}</title>
<meta name="description" content="{p['desc']}">
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="icon" href="/favicon.ico" sizes="32x32">
<link rel="canonical" href="{BASE}{lp(lang, p['path'])}">
{hreflangs(p['path'])}
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Playfair+Display:ital,wght@0,400..900;1,400..900&family=EB+Garamond:ital,wght@0,400..700;1,400..700&family=Courier+Prime:wght@400;700&display=swap" rel="stylesheet">
{jsonld}
<style>{CSS}</style>
</head>
<body>
<nav><div class="wrap nav-in">
<a class="logo" href="{lp(lang, '/')}">{LOGO_SVG}<span>note<span class="dot">.</span>md</span></a>
<a class="npl" href="https://plugins.notemd.net">{c['pl']}</a>
{switcher(lang, p['path'])}
<a class="nav-cta" href="/download">{c['dl']}</a>
</div></nav>
<header class="ph"><div class="wrap">
<div class="crumb">{p['crumb']}</div>
<h1>{p['h1']}</h1>
<p class="lead">{p['lead']}</p>
</div></header>
<main><div class="wrap">
{table_html}
{body_html}
{faq_html}
</div></main>
<section class="cta"><div class="wrap">
<h2>{c['cta_h2']}</h2>
<p>{c['cta_p']}</p>
<a class="btn" href="/download">{c['cta_btn']}</a>
</div></section>
<footer><div class="wrap">
{foot_links(lang)}
<div class="fbase">note<span style="color:var(--amber)">.</span>md — <a href="{lp(lang, '/')}" style="color:#B9BDC7">notemd.net</a> · {c['sig']}</div>
</div></footer>
</body>
</html>"""

DL = '<a href="/download">Download note.md</a>'

PAGES = [
# ---------------------------------------------------------------- compare
{
 "path": "/compare/roam-research/",
 "title": "note.md vs Roam Research (2026) — files, agents, and what happened to Roam",
 "desc": "An honest comparison of note.md and Roam Research: outliner notes, daily notes and [[wikilinks]] — as plain local files with AI-agent support, versus Roam's in-browser graph. Including a migration path.",
 "crumb": "Compare",
 "h1": "note.md vs Roam Research",
 "lead": "Both love outlines, daily notes, and [[double brackets]]. One keeps your ten years of thinking in a company's browser tab. The other keeps it in a folder you own.",
 "table": {
  "head": ["", "note.md", "Roam Research"],
  "rows": [
   ["Where your notes live", "Plain markdown files on your disk", "Proprietary graph database in the cloud"],
   ["Price", "Free, open source", "From $15/month"],
   ["Daily notes &amp; outlines", "Yes — <code>.note.md</code> outline files", "Yes — where the pattern was born"],
   ["[[Wikilinks]] &amp; backlinks", "Yes, one namespace across the vault", "Yes, plus block references and queries"],
   ["Block-level citations", "Yes — <code>((file#b-xxxxxx))</code>, edit-resilient", "Yes — block refs, deeper (embeds, queries)"],
   ["AI agents", "First-class: plain files + <code>AGENTS.md</code>, agents read your annotations", "None built in"],
   ["Reading &amp; annotating AI documents", "Core workflow — sidecar <code>.note.md</code>", "Not a focus"],
   ["Development pace", "Active", "Famously quiet since ~2021"],
   ["Offline / longevity", "Files readable in any editor, forever", "Export required; app needed to read the graph"],
  ]},
 "sections": [
  ("The honest take", """<p>Roam invented the daily-notes-plus-backlinks way of thinking in 2020, and credit where due: if you use block references, embeds, and datalog queries heavily, Roam still goes deeper than note.md does. Nothing here pretends otherwise.</p>
<p>But Roam made one bet that aged badly: your graph lives in their database, behind their subscription, at the mercy of their roadmap — and that roadmap has been quiet for years. Meanwhile the world flipped. Agents write markdown by the megabyte, and the tools that matter now are the ones that read and write <em>plain files</em>. A browser-tab graph can't be your agent's memory. A folder of markdown can.</p>
<p>note.md keeps what made Roam great — the outline editor, daily notes, one big <code>[[namespace]]</code>, instant search — and rebuilds it on files. Your vault opens in any editor today and in fifty years. And it adds the thing Roam never had: your agents as first-class citizens, reading your annotations before they write another word.</p>"""),
  ("Migrating from Roam", """<p>Export your graph as JSON (Roam supports full export), and note.md's Roam importer (on the roadmap, converter available) turns pages into <code>wikipage/</code> outline notes and daily notes into <code>dailynote/yyyy/yyyy-MM-dd.note.md</code> — rewriting date links like <code>[[July 10th, 2026]]</code> to the canonical <code>[[2026-07-10]]</code> and reporting any broken links. Your three years of notes become three years of agent-searchable context.</p>"""),
  ("Choose one", """<ul>
<li><b>Stay with Roam</b> if block references, embeds, and queries are load-bearing in your workflow, and you're comfortable with the subscription and the pace.</li>
<li><b>Choose note.md</b> if you want Roam's writing feel on files you own, your notes to double as agent memory, and reading AI output to be a first-class act.</li>
</ul>"""),
 ],
 "faq": [
  ("Can I import my Roam Research graph into note.md?",
   "Yes — export your graph as JSON from Roam, and convert pages to wiki notes and daily notes to dated outline files. Date links are rewritten to the canonical [[yyyy-MM-dd]] form and broken links are reported."),
  ("Does note.md have block references like Roam?",
   "note.md has stable block IDs: every top-level block gets a b-xxxxxx id you can cite from anywhere as ((file#b-xxxxxx)). It covers citation and navigation; Roam-style transclusion/embeds are not a goal."),
  ("Is note.md free?",
   "Yes. note.md is free and open source (Apache-2.0). Roam Research starts at $15/month."),
 ],
},
{
 "path": "/compare/obsidian/",
 "title": "note.md vs Obsidian (2026) — two file-over-app editors, one built for agents",
 "desc": "note.md and Obsidian both keep your notes as local markdown. The difference: note.md is built for reading and annotating AI output, with sidecar notes and agent conventions out of the box.",
 "crumb": "Compare",
 "h1": "note.md vs Obsidian",
 "lead": "Closest cousins. Both believe in files over apps. Obsidian is the everything-toolbox; note.md is a sharpened blade for the AI reading loop. Your vault opens in both — by design.",
 "table": {
  "head": ["", "note.md", "Obsidian"],
  "rows": [
   ["Storage", "Plain markdown files, local", "Plain markdown files, local"],
   ["Price", "Free, open source", "Free (closed source); paid Sync/Publish"],
   ["Reading AI documents", "Core workflow: clean reading view, marks kept", "A general editor; possible via setup"],
   ["Annotations", "Sidecar <code>.note.md</code> — source stays clean", "Inline edits, or community plugins"],
   ["Agent support", "Built in: <code>AGENTS.md</code> conventions, block citations, annotations as agent input", "Via plugins and DIY (a popular pattern)"],
   ["Outliner", "Native <code>.note.md</code> outline view", "Via plugins; Obsidian is page-oriented"],
   ["Plugin ecosystem", "Small, out-of-process, capability-gated", "Enormous — thousands of community plugins"],
   ["Mobile", "Not yet (macOS first)", "Excellent iOS/Android apps"],
   ["Interop", "Vault opens in Obsidian", "Vault opens in note.md"],
  ]},
 "sections": [
  ("The honest take", """<p>If you love Obsidian, keep it — seriously. It's the most successful file-over-app editor ever made, its plugin ecosystem is unmatched, and pointing Claude Code at an Obsidian vault is one of the great DIY patterns of the decade. note.md's vault format is deliberately Obsidian-compatible, because we believe the same thing they do: your files should open anywhere.</p>
<p>The difference is what happens out of the box. Obsidian is a general-purpose toolbox you assemble: to get the AI reading loop working you wire up plugins, conventions, an agent config, and hope the pieces stay compatible. note.md ships the loop as the product: agents write documents, you read them in a view built for judgment, your highlights land in a sidecar <code>.note.md</code> that never pollutes the source, and every agent that visits your vault reads your margins first. No assembly.</p>
<p>The sidecar is the real fork in the road. Obsidian's annotations live inside the document — fine for notes you wrote, awkward for documents an agent generated and might regenerate. note.md separates the regenerable (the AI's text) from the irreplaceable (your judgment), file by file.</p>"""),
  ("Use both", """<p>This isn't a divorce. A note.md vault is a folder of markdown: open it in Obsidian for graph view and mobile capture, open it in note.md for the reading-annotation loop and agent workflows. Two clients, one source of truth. That's the whole point of files.</p>"""),
  ("Choose one (or don't)", """<ul>
<li><b>Choose Obsidian</b> if you want maximum plugins, mobile apps, and graph view — and enjoy assembling your own AI workflow.</li>
<li><b>Choose note.md</b> if your day is increasingly reading what agents wrote, and you want annotations-as-data and agent conventions without any assembly.</li>
<li><b>Use both</b> on the same vault. Files don't make you choose.</li>
</ul>"""),
 ],
 "faq": [
  ("Can I open my note.md vault in Obsidian?",
   "Yes. A note.md vault is plain markdown with filename-resolvable [[wikilinks]], deliberately kept Obsidian-compatible. Sidecar .note.md files appear as ordinary notes there."),
  ("Do I have to leave Obsidian to use note.md?",
   "No. Point both apps at the same folder. Many users keep Obsidian for mobile capture and graph view, and use note.md for reading AI documents and annotating."),
  ("What is a sidecar annotation?",
   "When you highlight or comment on xxx.md in note.md, your marks are saved to a companion file xxx.note.md. The original document stays clean and regenerable; your judgment becomes separate, searchable data."),
 ],
},
{
 "path": "/compare/notion/",
 "title": "note.md vs Notion (2026) — your files vs their workspace",
 "desc": "Notion is an all-in-one cloud workspace. note.md is a folder of markdown on your disk, built for the AI era. Ownership, longevity, agents, and when each one actually wins.",
 "crumb": "Compare",
 "h1": "note.md vs Notion",
 "lead": "Notion wants to be the workspace for everything your team does. note.md wants to be nothing — just files, a good reader, and your judgment. Opposite bets on the same future.",
 "table": {
  "head": ["", "note.md", "Notion"],
  "rows": [
   ["Model", "Local markdown files you own", "Cloud workspace, blocks in their database"],
   ["Price", "Free, open source", "Free tier; teams pay per seat, AI extra"],
   ["Offline", "Always — it's your disk", "Limited; cloud-first"],
   ["AI", "Any agent, via plain files — yours to choose", "Notion AI, inside Notion, on their terms"],
   ["Team collaboration", "Git-based sharing; single-player first", "Excellent — real-time multiplayer, comments"],
   ["Databases &amp; project tools", "No — it's a notes tool (CSV grid included)", "Yes — tables, kanban, calendars, forms"],
   ["Data longevity", "Readable in fifty years, any editor", "Export to markdown/CSV; structure degrades"],
   ["Lock-in", "None — the folder is the product", "The workspace is the product"],
  ]},
 "sections": [
  ("The honest take", """<p>If you run a team wiki, a project tracker, and a hiring pipeline, Notion is genuinely good and note.md is not trying to be that. Real-time multiplayer, databases, permissions — that's Notion's home turf and it earns its seats.</p>
<p>But personal knowledge is a different game with a different time horizon. Your notes should outlive your employer, your tools, and possibly Notion Labs Inc. Every page you write into a cloud workspace is a page you'll someday export, reformat, and grieve over — ask anyone who has left Evernote. note.md's answer is structural: there is nothing to export, because there was never anything but files.</p>
<p>Then there's the AI question. Notion gives you Notion AI — one assistant, inside one app, priced per seat. note.md gives you a vault any agent can work: Claude Code today, whatever ships next week, all reading the same files and the same <code>AGENTS.md</code>. In a decade where the assistants change monthly, betting your knowledge on one vendor's AI is the new lock-in.</p>"""),
  ("Choose one", """<ul>
<li><b>Choose Notion</b> for team wikis, project management, and anything that needs multiplayer editing and databases.</li>
<li><b>Choose note.md</b> for your own thinking: reading AI output, daily notes, a personal knowledge base that compounds for decades and feeds every agent you'll ever use.</li>
<li><b>Common pattern:</b> Notion for the team, note.md for yourself.</li>
</ul>"""),
 ],
 "faq": [
  ("Can note.md replace Notion for a team?",
   "Mostly no. note.md is single-player first — a personal reading and notes tool over plain files, with git-based sharing. Notion's databases and real-time collaboration are not goals."),
  ("Can I export Notion pages into note.md?",
   "Yes. Notion exports markdown; drop the files into your vault and they become ordinary notes you can read, annotate, and link."),
  ("Why does local-first matter for AI?",
   "Agents work best on plain files they can read and write directly. A local markdown vault is instantly usable by any CLI agent — no API tokens, no rate limits, no vendor's AI as gatekeeper."),
 ],
},
# ------------------------------------------------------------ integrations
{
 "path": "/integrations/openclaw/",
 "title": "Using note.md with OpenClaw — give your personal agent a real memory",
 "desc": "OpenClaw stores memory as markdown files. note.md is a markdown vault with a reading-annotation loop. Point them at the same folder and your agent's memory becomes your notebook.",
 "crumb": "Integrations",
 "h1": "note.md + OpenClaw",
 "lead": "OpenClaw's philosophy: the model only remembers what gets saved to disk. note.md's philosophy: the disk is the product. This is barely an integration — more like two tools discovering they were built for each other.",
 "sections": [
  ("Why this pairing works", """<p>OpenClaw keeps its memory as plain markdown — <code>MEMORY.md</code> for long-term facts, <code>memory/YYYY-MM-DD.md</code> for daily working notes. That's structurally identical to a note.md vault's <code>wikipage/</code> and <code>dailynote/</code> convention: dated outlines plus curated pages. Same idea, convergently evolved.</p>
<p>Pair them and each side gets what it lacks: OpenClaw gets a human who actually reads and curates its memory in a view built for that; you get an agent that works around the clock and writes everything down where you can see it.</p>"""),
  ("Setup", """<ol>
<li>Put an <code>AGENTS.md</code> at your vault root describing the conventions (sidecar pairing, daily-note paths, <code>[[yyyy-MM-dd]]</code> date links). Grab the summary from <a href="/llms-full.txt">llms-full.txt</a>.</li>
<li>Point OpenClaw's workspace at your vault (or symlink its <code>memory/</code> into <code>dailynote/</code> — dated files are dated files).</li>
<li>Have OpenClaw write reports and research as <code>.md</code> documents into the vault.</li>
<li>Open them in note.md, read, highlight, question — your marks land in sidecar <code>.note.md</code> files.</li>
<li>Tell OpenClaw to read sidecars before follow-up work. Your judgment becomes its steering signal.</li>
</ol>"""),
  ("The loop in practice", """<p>Evening: OpenClaw researches a topic and drops <code>research/topic.md</code> in the vault. Morning: you read it in note.md over coffee, highlight two claims, add a doubt. Afternoon: OpenClaw picks up <code>research/topic.note.md</code>, sees exactly which claims earned your attention, and digs where you doubted. No prompt engineering — just files.</p>"""),
 ],
 "faq": [
  ("Does OpenClaw need a plugin to work with note.md?",
   "No. Both sides speak plain markdown files. An AGENTS.md at the vault root describing the conventions is all the 'integration' there is."),
  ("Is it safe to let OpenClaw write into my vault?",
   "Keep the vault in git (see the GitHub guide) so every agent write is diffable and revertible. By convention agents should not write into your .note.md sidecars — state that rule in AGENTS.md."),
 ],
},
{
 "path": "/integrations/cowork/",
 "title": "Using note.md with Claude Cowork — annotate what Claude builds",
 "desc": "Claude's Cowork delivers markdown reports and documents. Keep them in a note.md vault, read and annotate them locally, and let the next session read your margins.",
 "crumb": "Integrations",
 "h1": "note.md + Claude Cowork",
 "lead": "Cowork runs Claude in the cloud and connects to folders on your Mac. Connect your vault, and everything Claude produces becomes something you can read, mark, and keep.",
 "sections": [
  ("Why this pairing works", """<p>Cowork's deliverables are overwhelmingly markdown: research reports, plans, specs, drafts. By default they scatter — a download here, a conversation attachment there. Point Cowork at your note.md vault instead, and its output lands where your reading loop lives: every report gets a home, every read-through leaves a sidecar of judgment, and your next Cowork session can be told to read those sidecars first.</p>"""),
  ("Setup", """<ol>
<li>In the Claude desktop app, connect your vault folder to the Cowork session ("Add folder").</li>
<li>Add an <code>AGENTS.md</code> at the vault root (conventions summary: <a href="/llms-full.txt">llms-full.txt</a>) — Claude reads it automatically and follows the house rules.</li>
<li>Ask Claude to save deliverables into the vault, e.g. <code>research/2026-07-11-competitor-scan.md</code>.</li>
<li>Read them in note.md; your highlights and notes save to sidecar <code>.note.md</code> files.</li>
<li>Next session, one line: "Read the .note.md sidecars for the reports you wrote last week and address my margins." The loop closes.</li>
</ol>"""),
  ("Tips", """<ul>
<li>Ask Claude to use <code>[[wikilinks]]</code> and the <code>[[yyyy-MM-dd]]</code> date format so its documents join your vault's link graph instead of floating outside it.</li>
<li>Keep the vault in git — Cowork writes are then diffable, and its file-versioning plus yours never fight.</li>
</ul>"""),
 ],
 "faq": [
  ("Does Claude respect the vault conventions?",
   "Yes, if you put them in an AGENTS.md at the folder root — Claude Code and Cowork read agent instruction files as standard practice."),
  ("Can Claude read my annotations?",
   "That's the point. Sidecar .note.md files are plain markdown; ask any session to read them and it will see exactly what you highlighted and questioned."),
 ],
},
{
 "path": "/integrations/codex/",
 "title": "Using note.md with Codex — AGENTS.md is already its native language",
 "desc": "OpenAI's Codex CLI reads AGENTS.md by convention. A note.md vault carries its rules in exactly that file. Run codex inside your vault and it already knows how to behave.",
 "crumb": "Integrations",
 "h1": "note.md + Codex",
 "lead": "Codex popularized AGENTS.md — a plain file telling the agent how a folder works. A note.md vault is a folder whose rules live in AGENTS.md. You can see where this is going.",
 "sections": [
  ("Why this pairing works", """<p>Codex reads <code>AGENTS.md</code> from the directory it runs in — that's its native convention, no configuration. A note.md vault publishes its file rules (sidecar pairing, outline format, date links, block citations) in exactly that file. So the integration is: <code>cd vault &amp;&amp; codex</code>. Done.</p>
<p>Codex is strongest as a working agent: ask it to draft, refactor documents, batch-process notes, or build the small scripts your vault accumulates (importers, link checkers, report generators). Everything it writes is markdown in the vault, which means everything it writes flows into your reading-annotation loop.</p>"""),
  ("Setup", """<ol>
<li>Copy the conventions summary from <a href="/llms-full.txt">llms-full.txt</a> into <code>AGENTS.md</code> at your vault root.</li>
<li>Add vault-specific rules — e.g. "never modify <code>*.note.md</code>", "new research goes under <code>research/</code> with a date prefix".</li>
<li>Run <code>codex</code> in the vault directory. It picks up the rules automatically.</li>
<li>Review its output in note.md; annotate; tell the next run to read the sidecars.</li>
</ol>"""),
 ],
 "faq": [
  ("Does Codex need an MCP server to use the vault?",
   "No. The vault is plain files in the working directory — Codex's home turf. An MCP endpoint exists for the share worker (publishing pages), not for basic vault work."),
  ("What should I forbid in AGENTS.md?",
   "The one hard rule: agents don't write into your .note.md sidecar files — those hold human judgment. Everything else (naming, folders, link style) is house preference."),
 ],
},
{
 "path": "/integrations/hermes/",
 "title": "Using note.md with Hermes — persistent memory meets a permanent notebook",
 "desc": "Hermes (Nous Research) is an open agent with persistent memory and AGENTS.md conventions. Give it a note.md vault and its memory becomes something you can read, annotate, and own.",
 "crumb": "Integrations",
 "h1": "note.md + Hermes",
 "lead": "Hermes grows with you — an open agent that remembers. note.md is where a human keeps judgment. Same folder, both jobs.",
 "sections": [
  ("Why this pairing works", """<p>Hermes (by Nous Research) is built around persistent, file-based memory and reads <code>AGENTS.md</code> conventions — the same open-agent lineage as OpenClaw, with an emphasis on self-hosted sovereignty. That worldview is note.md's worldview: no hidden state, files as truth, everything inspectable.</p>
<p>Run Hermes over a note.md vault and its accumulated memory stops being an opaque agent artifact and becomes part of your knowledge base: readable in the outline view, linkable with <code>[[wikilinks]]</code>, and — crucially — annotatable. You can literally leave margin notes on your agent's memories.</p>"""),
  ("Setup", """<ol>
<li><code>AGENTS.md</code> at the vault root, as always — conventions from <a href="/llms-full.txt">llms-full.txt</a> plus your house rules.</li>
<li>Configure Hermes's memory/workspace directory to live inside the vault (e.g. <code>agents/hermes/</code>), or have it write its outputs into your vault folders.</li>
<li>Let it work. Read what it wrote in note.md; annotate.</li>
<li>Instruct Hermes to consult <code>*.note.md</code> sidecars before revisiting a topic — your corrections become its training wheels.</li>
</ol>"""),
 ],
 "faq": [
  ("Is Hermes the same as OpenClaw?",
   "No — Hermes is Nous Research's open agent focused on persistent memory and self-hosted operation; OpenClaw is a separate viral open-source personal agent. Both speak markdown and AGENTS.md, so both pair with note.md the same way."),
  ("Can multiple agents share one vault?",
   "Yes — that's the design. Plain files plus one AGENTS.md means OpenClaw, Codex, Hermes, and Claude can all work the same vault. Keep it in git so every write is attributable and revertible."),
 ],
},
{
 "path": "/integrations/chatgpt-work/",
 "title": "Using note.md with ChatGPT (work mode) — generate into a vault you own",
 "desc": "ChatGPT's work mode connects to your folders and is strong at drafting and image generation. Point it at a note.md vault and everything it makes becomes markdown you can read, annotate, and keep.",
 "crumb": "Integrations",
 "h1": "note.md + ChatGPT (work)",
 "lead": "ChatGPT is the strongest generalist most people already have — great at drafting, summarizing, and generating images. note.md gives what it produces a permanent home: your vault, your files, your judgment on top.",
 "sections": [
  ("Why this pairing works", """<p>ChatGPT's work mode connects to folders and files and shines at the generative end of the pipeline: turning a rough outline into a draft, summarizing a stack of documents, and — increasingly — batch-generating images and diagrams. Left to itself, that output lives in a chat thread you'll lose. Aim it at a note.md vault instead and every deliverable lands as plain markdown (with images beside it), where your reading-annotation loop can catch it.</p>
<p>This is where the "one vault, many agents" idea earns its keep: ChatGPT is rarely your only agent. It's the fast generalist you reach for to <em>produce</em> — and the review, the long-running automation, and the final judgment can each go to whoever's best at that. Same files, different workers.</p>"""),
  ("Setup", """<ol>
<li>Keep your vault in a folder ChatGPT can reach — an OpenAI-connected folder, or a cloud/git-synced directory it can read and write.</li>
<li>Add an <code>AGENTS.md</code> at the vault root (conventions summary: <a href="/llms-full.txt">llms-full.txt</a>) and paste the same house rules into your ChatGPT project instructions — it won't auto-read the file the way a CLI agent does, so tell it.</li>
<li>Ask it to save deliverables into the vault as dated markdown, e.g. <code>drafts/2026-07-23-launch-post.md</code>, and to drop generated images into <code>{docname}_files/</code> with relative links.</li>
<li>Open the result in note.md; read, highlight, question — your marks land in sidecar <code>.note.md</code> files, the source stays clean and regenerable.</li>
</ol>"""),
  ("The loop in practice", """<p>You ask ChatGPT to draft a launch post and generate three hero images; it writes <code>drafts/launch-post.md</code> and fills a <code>_files/</code> folder. You read it in note.md, cut two images, highlight a paragraph that overclaims, and leave a note. Next you hand <code>launch-post.note.md</code> to a more careful reviewer agent — "address the margins." ChatGPT generated fast; the vault kept it; you judged it. That's the division of labor.</p>"""),
 ],
 "faq": [
  ("Does ChatGPT read AGENTS.md automatically?",
   "Not the way a CLI agent (Codex, Claude Code) does. Paste your vault conventions into the ChatGPT project or custom instructions, and point it at the AGENTS.md file so it follows the same house rules — sidecar files are off-limits, new work is dated markdown."),
  ("Can ChatGPT-generated images live in my vault?",
   "Yes. Save them beside the document in a {docname}_files/ folder with relative links — the same convention note.md uses for pasted screenshots. They render in the reading view and travel with the vault in git."),
  ("Do I have to pick one agent?",
   "No — that's the whole point. Use ChatGPT for fast generation, another agent for careful review, a local agent for private work. They collaborate through the files; you orchestrate. See the orchestration guide."),
 ],
},
# ----------------------------------------------------------------- guides
{
 "path": "/guides/share-on-cloudflare/",
 "title": "Free document sharing with note.md on Cloudflare — your own worker, your own links",
 "desc": "Deploy note.md's share worker to Cloudflare's free tier in ten minutes. Publish any markdown as a beautiful self-contained page — with math, diagrams, dark mode — on infrastructure you control.",
 "crumb": "Guides",
 "h1": "Free sharing on your own Cloudflare",
 "lead": "Cmd+Shift+L publishes a document as a web page — KaTeX, Mermaid, dark mode, mobile-ready. The twist: it publishes to your Cloudflare account, not ours. Free tier covers a personal workload easily.",
 "sections": [
  ("Why self-hosted sharing", """<p>Every "share" button you've ever clicked uploaded your document to someone else's server, under someone else's terms, with someone else's lifespan. note.md's share plugin deploys a small Worker to <em>your</em> Cloudflare account: your links, your data, your kill switch. The free tier (100k requests/day) is far more than a human sharing documents will ever use.</p>"""),
  ("Deploy in ten minutes", """<pre><code>cd worker
pnpm install
wrangler login
wrangler kv:namespace create SHARES     # copy the id into wrangler.toml
openssl rand -hex 32 | wrangler secret put SHARE_API_KEY
wrangler deploy                          # prints your Worker URL</code></pre>
<p>Paste the Worker URL and API key into <b>note.md → Preferences → Share</b>, restart, done. Full details live in the repo's <code>worker/README.md</code>.</p>"""),
  ("What you get", """<ul>
<li><b>One keystroke:</b> <code>Cmd+Shift+L</code> publishes the current file; the URL lands in your clipboard. Share again to update in place; unshare returns 410.</li>
<li><b>Faithful rendering:</b> KaTeX math, Mermaid diagrams as SVG, syntax highlighting, light/dark via <code>prefers-color-scheme</code>, mobile-optimized.</li>
<li><b>Images included:</b> image-heavy documents spill to Cloudflare R2 (also free tier) automatically.</li>
<li><b>Agent-ready:</b> the Worker exposes an MCP endpoint, so your agents can publish on your behalf — <code>notemd -s draft.md</code> does it from any script.</li>
</ul>"""),
 ],
 "faq": [
  ("How much does this cost?",
   "Nothing for personal use. Cloudflare's free tier includes 100,000 Worker requests per day and 10GB of R2 storage — orders of magnitude beyond a person sharing documents."),
  ("Can I take a shared page down?",
   "Yes, instantly. File → Unshare (or notemd share --unshare) revokes the link; visitors get a 410. It's your Worker — you can also just delete it."),
 ],
},
{
 "path": "/guides/vault-on-github/",
 "title": "Free vault hosting on GitHub — version history and sync for a folder of markdown",
 "desc": "A note.md vault is plain files, which means git works perfectly: free private hosting on GitHub, full version history, multi-device sync, and every agent write diffable and revertible.",
 "crumb": "Guides",
 "h1": "Your vault on GitHub, free",
 "lead": "A vault is a folder of markdown. Git was built for folders of text. GitHub hosts private repos for free. Three facts that add up to bulletproof, zero-cost infrastructure for a lifetime of notes.",
 "sections": [
  ("Why git is the perfect vault backend", """<p>Databases need backups you'll forget to make. Sync services need subscriptions and trust. Git needs neither: every save is a commit, every commit is history, every push is an off-site backup. And in the agent era it earns its keep twice over — <b>when agents write into your vault, git makes every write diffable, attributable, and revertible.</b> An agent's bad day is a <code>git revert</code>, not a tragedy.</p>"""),
  ("Setup", """<pre><code>cd ~/Vault
git init
printf '.DS_Store\\n.mdeditor/\\n' &gt; .gitignore
git add -A &amp;&amp; git commit -m "vault: day one"
gh repo create my-vault --private --source=. --push</code></pre>
<p>That's it. A private GitHub repo is free with unlimited history. From then on, commit as often as you like — or automate it.</p>"""),
  ("Sync and automation", """<ul>
<li><b>note.md integration:</b> the Sync-to-Vault plugin copies files into your git-synced vault with date-prefixed names and conflict-aware refresh; recent-file history mirrors across devices through the vault.</li>
<li><b>Auto-commit:</b> a cron line or launchd job running <code>git add -A &amp;&amp; git commit -m "auto" &amp;&amp; git push</code> every hour gives you effortless continuous backup.</li>
<li><b>Multi-device:</b> clone the repo on a second Mac; pull before writing, push after. Conflicts in outlines are rare (small files) and git shows exactly what happened when they occur.</li>
<li><b>Agents:</b> give agents a working copy. Review their commits like you'd review a colleague's PR — because that's what they are now.</li>
</ul>"""),
 ],
 "faq": [
  ("Is a private GitHub repo really free?",
   "Yes — unlimited private repositories with full history on GitHub's free plan. A text vault of decades fits in megabytes."),
  ("What about sensitive notes?",
   "The vault is yours: choose a private repo, a self-hosted Gitea, or no remote at all — git works locally. For extra caution, git-crypt or age can encrypt selected paths."),
  ("Do I need to know git?",
   "Barely. Three commands cover daily life (add, commit, push), and note.md's sync features hide most of it. The payoff — total history of every thought you ever wrote — is disproportionate."),
 ],
},
# ----------------------------------------------------------------- essays
{
 "path": "/orchestrate-agents/",
 "title": "One vault, many agents — orchestrate Cowork, Codex, OpenClaw & ChatGPT (2026)",
 "desc": "Your markdown vault is a git repo for agents. Claude Cowork, Claude Code, Codex, ChatGPT, OpenClaw and Hermes can all read and write the same files — so you assign each job to whoever's best at it, on whatever model, and keep the judgment for yourself.",
 "crumb": "Guide",
 "h1": "One vault, many agents. You orchestrate.",
 "lead": "The lock-in nobody warns you about isn't the app — it's the agent. Keep your knowledge in plain files, and no single AI owns it. Cowork drafts, Codex refactors, ChatGPT generates, a local agent guards your secrets — and you hold the pen.",
 "sections": [
  ("The vault is neutral ground", """<p>Most AI tools want to be the home for your thinking: your knowledge in their database, your annotations in their format, your agent the one they ship, your model the one they lock you to. Then "which AI do I use?" becomes "do I migrate everything?" — and you're fenced inside one vendor's roadmap.</p>
<p>A note.md vault flips it. The vault is a folder of plain markdown with shared conventions — an <code>AGENTS.md</code> that states the house rules, <code>((file#b-xxxxxx))</code> block citations for precise references, sidecar <code>.note.md</code> files that hold <em>your</em> judgment, and <code>[[wikilinks]]</code> for a single namespace. Those conventions are a <b>public protocol</b>: any agent can read them, no adapter required. The agents and models become interchangeable workers; the vault is the one thing that doesn't change. It's a git repo, and they're all committing to it.</p>"""),
  ("Assign each job to whoever's best at it", """<p>No single agent is best at everything. So don't make one do everything — build a line and put each tool at the station it's strongest at:</p>
<table><thead><tr><th>Stage</th><th>A good fit</th><th>Why</th></tr></thead><tbody>
<tr><td>Overnight automation</td><td>OpenClaw / Hermes</td><td>Long-running, file-based, self-hosted memory</td></tr>
<tr><td>Careful review &amp; revision</td><td>Claude Cowork / Code</td><td>Strong reasoning; reads your margins before editing</td></tr>
<tr><td>Fast drafting &amp; images</td><td>ChatGPT (work mode)</td><td>Generalist generation, batch image creation</td></tr>
<tr><td>In-repo refactors &amp; scripts</td><td>Codex</td><td>Native <code>AGENTS.md</code>, runs in the working dir</td></tr>
<tr><td>Final judgment</td><td>You</td><td>The one thing no model can generate</td></tr>
</tbody></table>
<p>You pick the agent <em>and</em> the model per job — a cheap fast model to triage, a frontier model to reason, a local model for anything private. The vault doesn't care which; it just holds the files they pass between them.</p>"""),
  ("A loop in practice", """<p>Here's a real pipeline, four tools and three models over one vault:</p>
<ol>
<li><b>OpenClaw</b> runs overnight, processing a batch of raw notes into <code>drafts/*.md</code>.</li>
<li>You hand the drafts to <b>Claude Cowork</b> on a careful model — "review and revise these, flag anything shaky."</li>
<li><b>ChatGPT</b> batch-generates the hero images into each doc's <code>_files/</code> folder.</li>
<li>The finished documents land in note.md, where <b>you</b> read them, cut what overclaims, highlight what matters, and leave the notes only you could write.</li>
</ol>
<p>Four tools, three models, one vault, one orchestrator. Nobody had to share memory or speak a private protocol — they handed off <code>.md</code> files on disk, and your sidecar <code>.note.md</code> annotations were the steering signal for the next one.</p>"""),
  ("Why files make it work", """<ul>
<li><b>Anti-lock-in, one layer deeper.</b> Files-over-app frees you from the app; this frees you from the agent and the model. Today's best model is replaced next month — your knowledge shouldn't move with it.</li>
<li><b>Collaboration without a platform.</b> Agents hand off <code>.md</code> on disk — no shared memory, no private API, no plugin store. One agent's output is the next one's input.</li>
<li><b>You stay in the loop, at the checkpoint.</b> It's not a black box that runs end to end; it's a line of stations with a human at quality control. Agents write, review, and illustrate — you decide what ships.</li>
<li><b>Still just files.</b> No orchestration database, no hidden state. Rules in <code>AGENTS.md</code>, output in <code>.md</code>, judgment in <code>.note.md</code> — all readable by Obsidian, a CLI, or any agent. Swap out note.md and the vault is still everyone's shared workspace.</li>
</ul>
<p>Keep the vault in <a href="/guides/vault-on-github/">git</a> and every agent write is diffable, attributable, and revertible — an agent's bad day is a <code>git revert</code>, not a tragedy.</p>"""),
  ("Set it up", """<ol>
<li>Put an <code>AGENTS.md</code> at your vault root — grab the conventions from <a href="/llms-full.txt">llms-full.txt</a> and add house rules (the hard one: agents never write into <code>*.note.md</code> sidecars).</li>
<li>Wire up each agent on the same folder: <a href="/integrations/openclaw/">OpenClaw</a>, <a href="/integrations/cowork/">Cowork</a>, <a href="/integrations/codex/">Codex</a>, <a href="/integrations/chatgpt-work/">ChatGPT</a>, <a href="/integrations/hermes/">Hermes</a>.</li>
<li>Read and annotate the results in note.md; tell the next agent to read the sidecars first. The loop closes on your disk.</li>
</ol>"""),
 ],
 "faq": [
  ("Can different AI agents really share one vault?",
   "Yes — that's the design. A vault is plain markdown plus one AGENTS.md describing the conventions. Claude Cowork, Claude Code, Codex, ChatGPT, OpenClaw and Hermes all read and write those files, so you can route each task to whichever agent (and model) is best for it. Keep the vault in git so every write is diffable and revertible."),
  ("How do agents hand work off to each other?",
   "Through files. One agent writes markdown into the vault; the next reads it as input. Your annotations live in sidecar .note.md files and act as the steering signal — an agent reads your margins before its next pass. No shared memory or private protocol is needed."),
  ("Does this need a special orchestration tool or MCP server?",
   "No. The orchestration is you, and the medium is the filesystem. There's no central database or hidden state — rules in AGENTS.md, output in .md, judgment in .note.md. A Vault MCP server is on the roadmap for agents that prefer a tool interface, but plain files already work today."),
  ("Why not just use one AI for everything?",
   "Because no single agent is best at everything. Overnight automation, careful review, fast image generation, private local work, and final judgment are different jobs with different best-fit tools. Splitting them across specialists — over files you own — beats one generalist doing all of it, and keeps you free to swap any worker out."),
 ],
},
]

ALL = {"en": PAGES, "de": pages_de.PAGES, "ja": pages_ja.PAGES, "zh": pages_zh.PAGES}

def write_sitemap():
    paths = ["/"] + [p["path"] for p in PAGES]
    urls = []
    for path in paths:
        alts = "".join(f'\n    <xhtml:link rel="alternate" hreflang="{l}" href="{BASE}{lp(l, path)}"/>' for l in LANG_ORDER)
        alts += f'\n    <xhtml:link rel="alternate" hreflang="x-default" href="{BASE}{path}"/>'
        for l in LANG_ORDER:
            urls.append(f"  <url>\n    <loc>{BASE}{lp(l, path)}</loc>{alts}\n  </url>")
    xml = ('<?xml version="1.0" encoding="UTF-8"?>\n'
           '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"\n'
           '        xmlns:xhtml="http://www.w3.org/1999/xhtml">\n'
           + "\n".join(urls) + "\n</urlset>\n")
    open("public/sitemap.xml", "w", encoding="utf-8").write(xml)
    print("wrote public/sitemap.xml", f"({len(urls)} urls)")

def main():
    for lang, pages in ALL.items():
        for p in pages:
            out = "public" + lp(lang, p["path"]) + "index.html"
            os.makedirs(os.path.dirname(out), exist_ok=True)
            open(out, "w", encoding="utf-8").write(render(p, lang))
            print("wrote", out)
    write_sitemap()

if __name__ == "__main__":
    main()
