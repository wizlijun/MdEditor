# German translations of the SEO landing pages in build_pages.py.
# Same structure, same order, same keys. Paths, URLs, code and commands unchanged.

PAGES = [
# ---------------------------------------------------------------- compare
{
 "path": "/compare/roam-research/",
 "title": "note.md vs Roam Research (2026) — Dateien, Agents und was aus Roam wurde",
 "desc": "Ein ehrlicher Vergleich von note.md und Roam Research: Outliner-Notizen, Daily Notes und [[Links]] — als lokale Markdown-Dateien mit AI-Agent-Support, gegen Roams Graph im Browser-Tab. Inklusive Migrationspfad.",
 "crumb": "Vergleich",
 "h1": "note.md vs Roam Research",
 "lead": "Beide lieben Outlines, Daily Notes und [[doppelte Klammern]]. Das eine hält deine zehn Jahre Denkarbeit im Browser-Tab einer Firma. Das andere in einem Ordner, der dir gehört.",
 "table": {
  "head": ["", "note.md", "Roam Research"],
  "rows": [
   ["Wo deine Notizen leben", "Reine Markdown-Dateien auf deiner Platte", "Proprietäre Graph-Datenbank in der Cloud"],
   ["Preis", "Kostenlos, Open Source", "Ab 15 $/Monat"],
   ["Daily Notes &amp; Outlines", "Ja — <code>.note.md</code>-Outline-Dateien", "Ja — hier wurde das Muster erfunden"],
   ["[[Wikilinks]] &amp; Backlinks", "Ja, ein Namespace über den ganzen Vault", "Ja, plus Block-Referenzen und Queries"],
   ["Zitate auf Block-Ebene", "Ja — <code>((file#b-xxxxxx))</code>, überlebt Edits", "Ja — Block-Refs, tiefer (Embeds, Queries)"],
   ["AI-Agents", "First-class: reine Dateien + <code>AGENTS.md</code>, Agents lesen deine Anmerkungen", "Nichts eingebaut"],
   ["AI-Dokumente lesen &amp; annotieren", "Kern-Workflow — Sidecar-Datei <code>.note.md</code>", "Kein Fokus"],
   ["Entwicklungstempo", "Aktiv", "Berühmt-berüchtigt still seit ~2021"],
   ["Offline / Langlebigkeit", "Dateien lesbar in jedem Editor, für immer", "Export nötig; ohne App kein Graph"],
  ]},
 "sections": [
  ("Die ehrliche Einschätzung", """<p>Roam hat 2020 das Denken in Daily Notes plus Backlinks erfunden, und Respekt, wo Respekt hingehört: Wer Block-Referenzen, Embeds und Datalog-Queries intensiv nutzt, kommt mit Roam immer noch tiefer als mit note.md. Das behauptet hier niemand anders.</p>
<p>Aber Roam hat eine Wette platziert, die schlecht gealtert ist: Dein Graph lebt in deren Datenbank, hinter deren Abo, ausgeliefert an deren Roadmap — und diese Roadmap ist seit Jahren still. Währenddessen hat sich die Welt gedreht. Agents schreiben Markdown im Megabyte-Takt, und die Tools, die jetzt zählen, sind die, die <em>reine Dateien</em> lesen und schreiben. Ein Graph im Browser-Tab kann nicht das Gedächtnis deines Agents sein. Ein Ordner voller Markdown schon.</p>
<p>note.md behält, was Roam großartig gemacht hat — den Outline-Editor, Daily Notes, einen großen <code>[[Namespace]]</code>, Sofort-Suche — und baut es neu auf Dateien. Dein Vault öffnet sich in jedem Editor, heute und in fünfzig Jahren. Und es kommt das dazu, was Roam nie hatte: deine Agents als vollwertige Bürger, die deine Anmerkungen lesen, bevor sie das nächste Wort schreiben.</p>"""),
  ("Migration von Roam", """<p>Exportiere deinen Graph als JSON (Roam unterstützt Voll-Export), und note.mds Roam-Importer (auf der Roadmap, Converter verfügbar) verwandelt Pages in <code>wikipage/</code>-Outline-Notizen und Daily Notes in <code>dailynote/yyyy/yyyy-MM-dd.note.md</code> — Datumslinks wie <code>[[July 10th, 2026]]</code> werden auf das kanonische <code>[[2026-07-10]]</code> umgeschrieben, kaputte Links werden gemeldet. Deine drei Jahre Notizen werden drei Jahre agent-durchsuchbarer Kontext.</p>"""),
  ("Entscheide dich", """<ul>
<li><b>Bleib bei Roam</b>, wenn Block-Referenzen, Embeds und Queries tragende Wände in deinem Workflow sind und du mit dem Abo und dem Tempo leben kannst.</li>
<li><b>Nimm note.md</b>, wenn du Roams Schreibgefühl auf Dateien willst, die dir gehören, deine Notizen zugleich Agent-Gedächtnis sein sollen und das Lesen von AI-Output ein erstklassiger Akt sein soll.</li>
</ul>"""),
 ],
 "faq": [
  ("Kann ich meinen Roam-Research-Graph in note.md importieren?",
   "Ja — exportiere deinen Graph als JSON aus Roam und konvertiere Pages zu Wiki-Notizen und Daily Notes zu datierten Outline-Dateien. Datumslinks werden auf die kanonische [[yyyy-MM-dd]]-Form umgeschrieben, kaputte Links werden gemeldet."),
  ("Hat note.md Block-Referenzen wie Roam?",
   "note.md hat stabile Block-IDs: Jeder Top-Level-Block bekommt eine b-xxxxxx-ID, die du von überall als ((file#b-xxxxxx)) zitieren kannst. Das deckt Zitieren und Navigation ab; Transklusion/Embeds im Roam-Stil sind kein Ziel."),
  ("Ist note.md kostenlos?",
   "Ja. note.md ist kostenlos und Open Source (Apache-2.0). Roam Research startet bei 15 $/Monat."),
 ],
},
{
 "path": "/compare/obsidian/",
 "title": "note.md vs Obsidian (2026) — zwei File-over-App-Editoren, einer für Agents gebaut",
 "desc": "note.md und Obsidian speichern deine Notizen beide als lokales Markdown. Der Unterschied: note.md ist fürs Lesen und Annotieren von AI-Output gebaut — mit Randnotizen und Agent-Konventionen out of the box.",
 "crumb": "Vergleich",
 "h1": "note.md vs Obsidian",
 "lead": "Engste Verwandte. Beide glauben an Dateien statt Apps. Obsidian ist der Alles-Werkzeugkasten; note.md ist eine geschärfte Klinge für den AI-Lese-Loop. Dein Vault öffnet sich in beiden — mit Absicht.",
 "table": {
  "head": ["", "note.md", "Obsidian"],
  "rows": [
   ["Speicherung", "Reine Markdown-Dateien, lokal", "Reine Markdown-Dateien, lokal"],
   ["Preis", "Kostenlos, Open Source", "Kostenlos (Closed Source); Sync/Publish kosten extra"],
   ["AI-Dokumente lesen", "Kern-Workflow: saubere Leseansicht, Markierungen bleiben", "Ein General-Editor; mit Bastelei machbar"],
   ["Annotationen", "Sidecar-Datei <code>.note.md</code> — die Quelle bleibt sauber", "Inline-Edits oder Community-Plugins"],
   ["Agent-Support", "Eingebaut: <code>AGENTS.md</code>-Konventionen, Block-Zitate, Annotationen als Agent-Input", "Via Plugins und DIY (ein beliebtes Muster)"],
   ["Outliner", "Native <code>.note.md</code>-Outline-Ansicht", "Via Plugins; Obsidian denkt in Seiten"],
   ["Plugin-Ökosystem", "Klein, out-of-process, capability-gated", "Riesig — Tausende Community-Plugins"],
   ["Mobile", "Noch nicht (macOS zuerst)", "Exzellente iOS/Android-Apps"],
   ["Interop", "Vault öffnet sich in Obsidian", "Vault öffnet sich in note.md"],
  ]},
 "sections": [
  ("Die ehrliche Einschätzung", """<p>Wenn du Obsidian liebst, behalt es — im Ernst. Es ist der erfolgreichste File-over-App-Editor aller Zeiten, sein Plugin-Ökosystem ist unerreicht, und Claude Code auf einen Obsidian-Vault loszulassen ist eines der großen DIY-Muster des Jahrzehnts. note.mds Vault-Format ist absichtlich Obsidian-kompatibel, weil wir dasselbe glauben wie sie: Deine Dateien sollen sich überall öffnen lassen.</p>
<p>Der Unterschied ist, was out of the box passiert. Obsidian ist ein Universal-Werkzeugkasten, den du selbst zusammenbaust: Für den AI-Lese-Loop verdrahtest du Plugins, Konventionen, eine Agent-Config — und hoffst, dass die Teile kompatibel bleiben. note.md liefert den Loop als Produkt: Agents schreiben Dokumente, du liest sie in einer Ansicht, die fürs Urteilen gebaut ist, deine Highlights landen in einer Sidecar-Datei <code>.note.md</code>, die die Quelle nie verschmutzt, und jeder Agent, der deinen Vault besucht, liest zuerst deine Randnotizen. Kein Zusammenbauen.</p>
<p>Die Sidecar-Datei ist die eigentliche Weggabelung. Obsidians Annotationen leben im Dokument selbst — okay für Notizen, die du geschrieben hast, unangenehm für Dokumente, die ein Agent generiert hat und vielleicht neu generiert. note.md trennt das Regenerierbare (den Text der AI) vom Unersetzlichen (deinem Urteil), Datei für Datei.</p>"""),
  ("Nutz beide", """<p>Das ist keine Scheidung. Ein note.md-Vault ist ein Ordner voller Markdown: Öffne ihn in Obsidian für Graph-Ansicht und mobiles Festhalten, öffne ihn in note.md für den Lese-Annotations-Loop und Agent-Workflows. Zwei Clients, eine Quelle der Wahrheit. Genau das ist der Punkt von Dateien.</p>"""),
  ("Entscheide dich (oder lass es)", """<ul>
<li><b>Nimm Obsidian</b>, wenn du maximale Plugins, mobile Apps und die Graph-Ansicht willst — und Spaß daran hast, deinen AI-Workflow selbst zusammenzubauen.</li>
<li><b>Nimm note.md</b>, wenn dein Tag zunehmend daraus besteht, zu lesen, was Agents geschrieben haben, und du Annotationen-als-Daten und Agent-Konventionen ohne Bastelei willst.</li>
<li><b>Nutz beide</b> auf demselben Vault. Dateien zwingen dich nicht zur Wahl.</li>
</ul>"""),
 ],
 "faq": [
  ("Kann ich meinen note.md-Vault in Obsidian öffnen?",
   "Ja. Ein note.md-Vault ist reines Markdown mit dateinamen-auflösbaren [[Wikilinks]], absichtlich Obsidian-kompatibel gehalten. Sidecar-Dateien (.note.md) erscheinen dort als gewöhnliche Notizen."),
  ("Muss ich Obsidian verlassen, um note.md zu nutzen?",
   "Nein. Richte beide Apps auf denselben Ordner. Viele behalten Obsidian für mobiles Festhalten und die Graph-Ansicht und nutzen note.md fürs Lesen und Annotieren von AI-Dokumenten."),
  ("Was ist eine Sidecar-Annotation?",
   "Wenn du in note.md etwas in xxx.md markierst oder kommentierst, werden deine Markierungen in einer Begleitdatei xxx.note.md gespeichert. Das Originaldokument bleibt sauber und regenerierbar; dein Urteil wird zu separaten, durchsuchbaren Daten."),
 ],
},
{
 "path": "/compare/notion/",
 "title": "note.md vs Notion (2026) — deine Dateien vs deren Workspace",
 "desc": "Notion ist ein All-in-one-Cloud-Workspace. note.md ist ein Ordner voller Markdown auf deiner Platte, gebaut für die AI-Ära. Eigentum, Langlebigkeit, Agents — und wann welches Tool wirklich gewinnt.",
 "crumb": "Vergleich",
 "h1": "note.md vs Notion",
 "lead": "Notion will der Workspace für alles sein, was dein Team tut. note.md will nichts sein — nur Dateien, ein guter Reader und dein Urteil. Entgegengesetzte Wetten auf dieselbe Zukunft.",
 "table": {
  "head": ["", "note.md", "Notion"],
  "rows": [
   ["Modell", "Lokale Markdown-Dateien, die dir gehören", "Cloud-Workspace, Blöcke in deren Datenbank"],
   ["Preis", "Kostenlos, Open Source", "Free-Tier; Teams zahlen pro Sitz, AI extra"],
   ["Offline", "Immer — es ist deine Platte", "Eingeschränkt; Cloud-first"],
   ["AI", "Jeder Agent, über reine Dateien — du wählst", "Notion AI, in Notion, zu deren Bedingungen"],
   ["Team-Kollaboration", "Teilen über Git; Single-Player zuerst", "Exzellent — Echtzeit-Multiplayer, Kommentare"],
   ["Datenbanken &amp; Projekt-Tools", "Nein — ein Notiz-Tool (CSV-Grid inklusive)", "Ja — Tabellen, Kanban, Kalender, Formulare"],
   ["Daten-Langlebigkeit", "In fünfzig Jahren lesbar, in jedem Editor", "Export nach Markdown/CSV; Struktur leidet"],
   ["Lock-in", "Keiner — der Ordner ist das Produkt", "Der Workspace ist das Produkt"],
  ]},
 "sections": [
  ("Die ehrliche Einschätzung", """<p>Wenn du ein Team-Wiki, einen Projekt-Tracker und eine Hiring-Pipeline betreibst, ist Notion wirklich gut — und note.md versucht gar nicht erst, das zu sein. Echtzeit-Multiplayer, Datenbanken, Berechtigungen: Das ist Notions Heimspiel, und die Sitze sind ihr Geld wert.</p>
<p>Aber persönliches Wissen ist ein anderes Spiel mit einem anderen Zeithorizont. Deine Notizen sollten deinen Arbeitgeber überleben, deine Tools — und möglicherweise Notion Labs Inc. Jede Seite, die du in einen Cloud-Workspace schreibst, ist eine Seite, die du eines Tages exportieren, neu formatieren und betrauern wirst — frag irgendjemanden, der Evernote verlassen hat. note.mds Antwort ist strukturell: Es gibt nichts zu exportieren, weil es nie etwas anderes als Dateien gab.</p>
<p>Und dann die AI-Frage. Notion gibt dir Notion AI — einen Assistenten, in einer App, pro Sitz bepreist. note.md gibt dir einen Vault, den jeder Agent bearbeiten kann: heute Claude Code, nächste Woche was auch immer erscheint, alle lesen dieselben Dateien und dieselbe <code>AGENTS.md</code>. In einem Jahrzehnt, in dem die Assistenten monatlich wechseln, ist es der neue Lock-in, dein Wissen auf die AI eines einzigen Anbieters zu setzen.</p>"""),
  ("Entscheide dich", """<ul>
<li><b>Nimm Notion</b> für Team-Wikis, Projektmanagement und alles, was Multiplayer-Editing und Datenbanken braucht.</li>
<li><b>Nimm note.md</b> für dein eigenes Denken: AI-Output lesen, Daily Notes, eine persönliche Wissensbasis, die über Jahrzehnte Zinseszins trägt und jeden Agent füttert, den du je benutzen wirst.</li>
<li><b>Übliches Muster:</b> Notion fürs Team, note.md für dich selbst.</li>
</ul>"""),
 ],
 "faq": [
  ("Kann note.md Notion für ein Team ersetzen?",
   "Größtenteils nein. note.md ist Single-Player zuerst — ein persönliches Lese- und Notiz-Tool über reinen Dateien, mit Teilen über Git. Notions Datenbanken und Echtzeit-Kollaboration sind keine Ziele."),
  ("Kann ich Notion-Seiten in note.md exportieren?",
   "Ja. Notion exportiert Markdown; leg die Dateien in deinen Vault und sie werden gewöhnliche Notizen, die du lesen, annotieren und verlinken kannst."),
  ("Warum ist Local-first für AI wichtig?",
   "Agents arbeiten am besten auf reinen Dateien, die sie direkt lesen und schreiben können. Ein lokaler Markdown-Vault ist für jeden CLI-Agent sofort nutzbar — keine API-Tokens, keine Rate-Limits, keine Anbieter-AI als Türsteher."),
 ],
},
# ------------------------------------------------------------ integrations
{
 "path": "/integrations/openclaw/",
 "title": "note.md mit OpenClaw nutzen — gib deinem persönlichen Agent ein echtes Gedächtnis",
 "desc": "OpenClaw speichert sein Gedächtnis als Markdown-Dateien. note.md ist ein Markdown-Vault mit Lese-Annotations-Loop. Richte beide auf denselben Ordner, und das Gedächtnis deines Agents wird dein Notizbuch.",
 "crumb": "Integrationen",
 "h1": "note.md + OpenClaw",
 "lead": "OpenClaws Philosophie: Das Modell erinnert sich nur an das, was auf der Platte landet. note.mds Philosophie: Die Platte ist das Produkt. Das ist kaum eine Integration — eher zwei Tools, die entdecken, dass sie füreinander gebaut wurden.",
 "sections": [
  ("Warum das Paar funktioniert", """<p>OpenClaw hält sein Gedächtnis als reines Markdown — <code>MEMORY.md</code> für Langzeit-Fakten, <code>memory/YYYY-MM-DD.md</code> für tägliche Arbeitsnotizen. Das ist strukturell identisch mit der <code>wikipage/</code>- und <code>dailynote/</code>-Konvention eines note.md-Vaults: datierte Outlines plus kuratierte Seiten. Dieselbe Idee, konvergent evolviert.</p>
<p>Kombinier sie, und jede Seite bekommt, was ihr fehlt: OpenClaw bekommt einen Menschen, der sein Gedächtnis tatsächlich liest und kuratiert, in einer Ansicht, die dafür gebaut ist; du bekommst einen Agent, der rund um die Uhr arbeitet und alles dort aufschreibt, wo du es sehen kannst.</p>"""),
  ("Setup", """<ol>
<li>Leg eine <code>AGENTS.md</code> in die Vault-Wurzel, die die Konventionen beschreibt (Sidecar-Pairing, Daily-Note-Pfade, <code>[[yyyy-MM-dd]]</code>-Datumslinks). Die Zusammenfassung gibt's in <a href="/llms-full.txt">llms-full.txt</a>.</li>
<li>Richte OpenClaws Workspace auf deinen Vault (oder symlink sein <code>memory/</code> nach <code>dailynote/</code> — datierte Dateien sind datierte Dateien).</li>
<li>Lass OpenClaw Reports und Recherchen als <code>.md</code>-Dokumente in den Vault schreiben.</li>
<li>Öffne sie in note.md, lies, markiere, hinterfrage — deine Markierungen landen in Sidecar-Dateien (<code>.note.md</code>).</li>
<li>Sag OpenClaw, es soll die Sidecar-Dateien vor Folgearbeiten lesen. Dein Urteil wird sein Steuersignal.</li>
</ol>"""),
  ("Der Loop in der Praxis", """<p>Abends: OpenClaw recherchiert ein Thema und legt <code>research/topic.md</code> in den Vault. Morgens: Du liest es in note.md beim Kaffee, markierst zwei Behauptungen, notierst einen Zweifel. Nachmittags: OpenClaw nimmt sich <code>research/topic.note.md</code> vor, sieht genau, welche Behauptungen deine Aufmerksamkeit verdient haben, und gräbt dort nach, wo du gezweifelt hast. Kein Prompt-Engineering — nur Dateien.</p>"""),
 ],
 "faq": [
  ("Braucht OpenClaw ein Plugin, um mit note.md zu arbeiten?",
   "Nein. Beide Seiten sprechen reine Markdown-Dateien. Eine AGENTS.md in der Vault-Wurzel, die die Konventionen beschreibt, ist die ganze 'Integration'."),
  ("Ist es sicher, OpenClaw in meinen Vault schreiben zu lassen?",
   "Halte den Vault in Git (siehe die GitHub-Anleitung), dann ist jeder Agent-Write diffbar und rückgängig zu machen. Per Konvention schreiben Agents nicht in deine .note.md-Sidecar-Dateien — halte diese Regel in der AGENTS.md fest."),
 ],
},
{
 "path": "/integrations/cowork/",
 "title": "note.md mit Claude Cowork nutzen — annotiere, was Claude baut",
 "desc": "Claudes Cowork liefert Markdown-Reports und -Dokumente. Halte sie in einem note.md-Vault, lies und annotiere sie lokal, und lass die nächste Session deine Randnotizen lesen.",
 "crumb": "Integrationen",
 "h1": "note.md + Claude Cowork",
 "lead": "Cowork lässt Claude in der Cloud laufen und verbindet sich mit Ordnern auf deinem Mac. Verbinde deinen Vault, und alles, was Claude produziert, wird etwas, das du lesen, markieren und behalten kannst.",
 "sections": [
  ("Warum das Paar funktioniert", """<p>Coworks Deliverables sind überwältigend oft Markdown: Recherche-Reports, Pläne, Specs, Entwürfe. Standardmäßig verstreuen sie sich — ein Download hier, ein Konversations-Anhang dort. Richte Cowork stattdessen auf deinen note.md-Vault, und sein Output landet dort, wo dein Lese-Loop lebt: Jeder Report bekommt ein Zuhause, jede Lektüre hinterlässt eine Sidecar-Datei voller Urteil, und deiner nächsten Cowork-Session kannst du sagen, sie soll diese Sidecar-Dateien zuerst lesen.</p>"""),
  ("Setup", """<ol>
<li>Verbinde in der Claude-Desktop-App deinen Vault-Ordner mit der Cowork-Session ("Add folder").</li>
<li>Leg eine <code>AGENTS.md</code> in die Vault-Wurzel (Konventions-Zusammenfassung: <a href="/llms-full.txt">llms-full.txt</a>) — Claude liest sie automatisch und hält sich an die Hausregeln.</li>
<li>Bitte Claude, Deliverables in den Vault zu speichern, z. B. <code>research/2026-07-11-competitor-scan.md</code>.</li>
<li>Lies sie in note.md; deine Highlights und Notizen landen in Sidecar-Dateien (<code>.note.md</code>).</li>
<li>Nächste Session, eine Zeile: "Lies die .note.md-Sidecars zu den Reports von letzter Woche und geh auf meine Randnotizen ein." Der Loop schließt sich.</li>
</ol>"""),
  ("Tipps", """<ul>
<li>Bitte Claude, <code>[[wikilinks]]</code> und das <code>[[yyyy-MM-dd]]</code>-Datumsformat zu nutzen, damit seine Dokumente Teil deines Link-Graphen werden, statt außerhalb zu treiben.</li>
<li>Halte den Vault in Git — Cowork-Writes sind dann diffbar, und seine Datei-Versionierung und deine kommen sich nie in die Quere.</li>
</ul>"""),
 ],
 "faq": [
  ("Hält sich Claude an die Vault-Konventionen?",
   "Ja, wenn du sie in eine AGENTS.md in der Ordner-Wurzel legst — Claude Code und Cowork lesen Agent-Instruktionsdateien standardmäßig."),
  ("Kann Claude meine Annotationen lesen?",
   "Genau das ist der Punkt. Sidecar-Dateien (.note.md) sind reines Markdown; bitte irgendeine Session, sie zu lesen, und sie sieht exakt, was du markiert und hinterfragt hast."),
 ],
},
{
 "path": "/integrations/codex/",
 "title": "note.md mit Codex nutzen — AGENTS.md ist schon seine Muttersprache",
 "desc": "OpenAIs Codex CLI liest AGENTS.md per Konvention. Ein note.md-Vault trägt seine Regeln in genau dieser Datei. Starte codex im Vault, und es weiß schon, wie es sich zu benehmen hat.",
 "crumb": "Integrationen",
 "h1": "note.md + Codex",
 "lead": "Codex hat AGENTS.md populär gemacht — eine reine Datei, die dem Agent erklärt, wie ein Ordner funktioniert. Ein note.md-Vault ist ein Ordner, dessen Regeln in AGENTS.md leben. Du ahnst, worauf das hinausläuft.",
 "sections": [
  ("Warum das Paar funktioniert", """<p>Codex liest <code>AGENTS.md</code> aus dem Verzeichnis, in dem es läuft — das ist seine native Konvention, keine Konfiguration nötig. Ein note.md-Vault veröffentlicht seine Dateiregeln (Sidecar-Pairing, Outline-Format, Datumslinks, Block-Zitate) in genau dieser Datei. Die Integration lautet also: <code>cd vault &amp;&amp; codex</code>. Fertig.</p>
<p>Codex ist am stärksten als Arbeits-Agent: Lass es entwerfen, Dokumente refactoren, Notizen batch-verarbeiten oder die kleinen Skripte bauen, die sich in deinem Vault ansammeln (Importer, Link-Checker, Report-Generatoren). Alles, was es schreibt, ist Markdown im Vault — und damit fließt alles, was es schreibt, in deinen Lese-Annotations-Loop.</p>"""),
  ("Setup", """<ol>
<li>Kopiere die Konventions-Zusammenfassung aus <a href="/llms-full.txt">llms-full.txt</a> in eine <code>AGENTS.md</code> in deiner Vault-Wurzel.</li>
<li>Ergänze vault-spezifische Regeln — z. B. "niemals <code>*.note.md</code> anfassen", "neue Recherchen kommen mit Datums-Präfix unter <code>research/</code>".</li>
<li>Starte <code>codex</code> im Vault-Verzeichnis. Es nimmt die Regeln automatisch auf.</li>
<li>Prüfe seinen Output in note.md; annotiere; sag dem nächsten Lauf, er soll die Sidecar-Dateien lesen.</li>
</ol>"""),
 ],
 "faq": [
  ("Braucht Codex einen MCP-Server für den Vault?",
   "Nein. Der Vault ist reine Dateien im Arbeitsverzeichnis — Codex' Heimspiel. Ein MCP-Endpunkt existiert für den Share-Worker (Seiten veröffentlichen), nicht für die normale Vault-Arbeit."),
  ("Was sollte ich in der AGENTS.md verbieten?",
   "Die eine harte Regel: Agents schreiben nicht in deine .note.md-Sidecar-Dateien — dort liegt menschliches Urteil. Alles andere (Benennung, Ordner, Link-Stil) ist Hauspräferenz."),
 ],
},
{
 "path": "/integrations/hermes/",
 "title": "note.md mit Hermes nutzen — persistentes Gedächtnis trifft permanentes Notizbuch",
 "desc": "Hermes (Nous Research) ist ein offener Agent mit persistentem Gedächtnis und AGENTS.md-Konventionen. Gib ihm einen note.md-Vault, und sein Gedächtnis wird etwas, das du lesen, annotieren und besitzen kannst.",
 "crumb": "Integrationen",
 "h1": "note.md + Hermes",
 "lead": "Hermes wächst mit dir — ein offener Agent, der sich erinnert. note.md ist der Ort, an dem ein Mensch sein Urteil aufbewahrt. Ein Ordner, beide Jobs.",
 "sections": [
  ("Warum das Paar funktioniert", """<p>Hermes (von Nous Research) ist um persistentes, dateibasiertes Gedächtnis herum gebaut und liest <code>AGENTS.md</code>-Konventionen — dieselbe Open-Agent-Linie wie OpenClaw, mit Betonung auf selbst gehosteter Souveränität. Diese Weltsicht ist note.mds Weltsicht: kein versteckter Zustand, Dateien als Wahrheit, alles inspizierbar.</p>
<p>Lass Hermes über einen note.md-Vault laufen, und sein angesammeltes Gedächtnis hört auf, ein undurchsichtiges Agent-Artefakt zu sein, und wird Teil deiner Wissensbasis: lesbar in der Outline-Ansicht, verlinkbar mit <code>[[wikilinks]]</code> und — entscheidend — annotierbar. Du kannst buchstäblich Randnotizen an den Erinnerungen deines Agents hinterlassen.</p>"""),
  ("Setup", """<ol>
<li><code>AGENTS.md</code> in die Vault-Wurzel, wie immer — Konventionen aus <a href="/llms-full.txt">llms-full.txt</a> plus deine Hausregeln.</li>
<li>Konfiguriere Hermes' Memory-/Workspace-Verzeichnis so, dass es im Vault liegt (z. B. <code>agents/hermes/</code>), oder lass es seine Outputs in deine Vault-Ordner schreiben.</li>
<li>Lass es arbeiten. Lies in note.md, was es geschrieben hat; annotiere.</li>
<li>Weise Hermes an, <code>*.note.md</code>-Sidecar-Dateien zu konsultieren, bevor es ein Thema wieder aufgreift — deine Korrekturen werden seine Stützräder.</li>
</ol>"""),
 ],
 "faq": [
  ("Ist Hermes dasselbe wie OpenClaw?",
   "Nein — Hermes ist der offene Agent von Nous Research mit Fokus auf persistentem Gedächtnis und Self-Hosting; OpenClaw ist ein separater, viraler Open-Source-Personal-Agent. Beide sprechen Markdown und AGENTS.md, also passen beide auf dieselbe Weise zu note.md."),
  ("Können sich mehrere Agents einen Vault teilen?",
   "Ja — das ist das Design. Reine Dateien plus eine AGENTS.md heißt: OpenClaw, Codex, Hermes und Claude können alle denselben Vault bearbeiten. Halte ihn in Git, damit jeder Write zuordenbar und rückgängig zu machen ist."),
 ],
},
# ----------------------------------------------------------------- guides
{
 "path": "/guides/share-on-cloudflare/",
 "title": "Kostenloses Dokumenten-Sharing mit note.md auf Cloudflare — dein eigener Worker, deine eigenen Links",
 "desc": "Deploye note.mds Share-Worker in zehn Minuten auf Cloudflares Free-Tier. Veröffentliche beliebiges Markdown als schöne, in sich geschlossene Seite — mit Mathe, Diagrammen, Dark Mode — auf Infrastruktur, die du kontrollierst.",
 "crumb": "Anleitungen",
 "h1": "Kostenloses Sharing auf deinem eigenen Cloudflare",
 "lead": "Cmd+Shift+L veröffentlicht ein Dokument als Webseite — KaTeX, Mermaid, Dark Mode, mobiltauglich. Der Twist: Es veröffentlicht auf deinen Cloudflare-Account, nicht auf unseren. Der Free-Tier deckt eine persönliche Last locker ab.",
 "sections": [
  ("Warum selbst gehostetes Sharing", """<p>Jeder "Teilen"-Button, den du je geklickt hast, hat dein Dokument auf den Server von jemand anderem geladen, zu den Bedingungen von jemand anderem, mit der Lebensdauer von jemand anderem. note.mds Share-Plugin deployt einen kleinen Worker auf <em>deinen</em> Cloudflare-Account: deine Links, deine Daten, dein Kill-Switch. Der Free-Tier (100k Requests/Tag) ist weit mehr, als ein Mensch beim Teilen von Dokumenten je verbrauchen wird.</p>"""),
  ("In zehn Minuten deployt", """<pre><code>cd worker
pnpm install
wrangler login
wrangler kv:namespace create SHARES     # copy the id into wrangler.toml
openssl rand -hex 32 | wrangler secret put SHARE_API_KEY
wrangler deploy                          # prints your Worker URL</code></pre>
<p>Worker-URL und API-Key in <b>note.md → Preferences → Share</b> einfügen, neu starten, fertig. Alle Details stehen in der <code>worker/README.md</code> des Repos.</p>"""),
  ("Was du bekommst", """<ul>
<li><b>Ein Tastendruck:</b> <code>Cmd+Shift+L</code> veröffentlicht die aktuelle Datei; die URL landet in deiner Zwischenablage. Nochmal teilen aktualisiert in place; Unshare liefert 410.</li>
<li><b>Treues Rendering:</b> KaTeX-Mathe, Mermaid-Diagramme als SVG, Syntax-Highlighting, Hell/Dunkel via <code>prefers-color-scheme</code>, mobil-optimiert.</li>
<li><b>Bilder inklusive:</b> Bildlastige Dokumente laufen automatisch nach Cloudflare R2 über (ebenfalls Free-Tier).</li>
<li><b>Agent-ready:</b> Der Worker stellt einen MCP-Endpunkt bereit, deine Agents können also in deinem Namen veröffentlichen — <code>notemd -s draft.md</code> erledigt es aus jedem Skript.</li>
</ul>"""),
 ],
 "faq": [
  ("Was kostet das?",
   "Nichts, für persönliche Nutzung. Cloudflares Free-Tier umfasst 100.000 Worker-Requests pro Tag und 10 GB R2-Speicher — Größenordnungen über dem, was ein Mensch beim Teilen von Dokumenten braucht."),
  ("Kann ich eine geteilte Seite wieder offline nehmen?",
   "Ja, sofort. File → Unshare (oder notemd share --unshare) widerruft den Link; Besucher bekommen einen 410. Es ist dein Worker — du kannst ihn auch einfach löschen."),
 ],
},
{
 "path": "/guides/vault-on-github/",
 "title": "Kostenloses Vault-Hosting auf GitHub — Versionshistorie und Sync für einen Ordner voller Markdown",
 "desc": "Ein note.md-Vault ist reine Dateien, also funktioniert Git perfekt: kostenloses privates Hosting auf GitHub, volle Versionshistorie, Multi-Device-Sync — und jeder Agent-Write ist diffbar und rückgängig zu machen.",
 "crumb": "Anleitungen",
 "h1": "Dein Vault auf GitHub, kostenlos",
 "lead": "Ein Vault ist ein Ordner voller Markdown. Git wurde für Ordner voller Text gebaut. GitHub hostet private Repos kostenlos. Drei Fakten, die sich zu kugelsicherer Null-Kosten-Infrastruktur für ein Leben voller Notizen addieren.",
 "sections": [
  ("Warum Git das perfekte Vault-Backend ist", """<p>Datenbanken brauchen Backups, die du vergessen wirst. Sync-Dienste brauchen Abos und Vertrauen. Git braucht keins von beidem: Jedes Speichern ist ein Commit, jeder Commit ist Historie, jeder Push ist ein Off-site-Backup. Und in der Agent-Ära verdient es sein Geld doppelt — <b>wenn Agents in deinen Vault schreiben, macht Git jeden Write diffbar, zuordenbar und rückgängig zu machen.</b> Der schlechte Tag eines Agents ist ein <code>git revert</code>, keine Tragödie.</p>"""),
  ("Setup", """<pre><code>cd ~/Vault
git init
printf '.DS_Store\\n.mdeditor/\\n' &gt; .gitignore
git add -A &amp;&amp; git commit -m "vault: day one"
gh repo create my-vault --private --source=. --push</code></pre>
<p>Das war's. Ein privates GitHub-Repo ist kostenlos, mit unbegrenzter Historie. Ab jetzt committest du, so oft du willst — oder automatisierst es.</p>"""),
  ("Sync und Automatisierung", """<ul>
<li><b>note.md-Integration:</b> Das Sync-to-Vault-Plugin kopiert Dateien mit Datums-Präfix in deinen git-synchronisierten Vault, mit konflikt-bewusstem Refresh; die Recent-Files-Historie spiegelt sich über den Vault auf alle Geräte.</li>
<li><b>Auto-Commit:</b> Eine Cron-Zeile oder ein launchd-Job mit <code>git add -A &amp;&amp; git commit -m "auto" &amp;&amp; git push</code> jede Stunde gibt dir mühelos kontinuierliches Backup.</li>
<li><b>Multi-Device:</b> Klone das Repo auf einen zweiten Mac; pull vor dem Schreiben, push danach. Konflikte in Outlines sind selten (kleine Dateien), und Git zeigt exakt, was passiert ist, wenn sie doch auftreten.</li>
<li><b>Agents:</b> Gib Agents eine Working Copy. Reviewe ihre Commits, wie du den PR einer Kollegin reviewen würdest — denn genau das sind sie jetzt.</li>
</ul>"""),
 ],
 "faq": [
  ("Ist ein privates GitHub-Repo wirklich kostenlos?",
   "Ja — unbegrenzt viele private Repositories mit voller Historie im Free-Plan von GitHub. Ein Text-Vault aus Jahrzehnten passt in Megabytes."),
  ("Was ist mit sensiblen Notizen?",
   "Der Vault gehört dir: Nimm ein privates Repo, ein selbst gehostetes Gitea oder gar kein Remote — Git funktioniert lokal. Für extra Vorsicht verschlüsseln git-crypt oder age ausgewählte Pfade."),
  ("Muss ich Git können?",
   "Kaum. Drei Befehle decken den Alltag ab (add, commit, push), und note.mds Sync-Features verstecken das meiste davon. Der Payoff — die totale Historie jedes Gedankens, den du je aufgeschrieben hast — ist unverhältnismäßig groß."),
 ],
},
{
 "path": "/integrations/chatgpt-work/",
 "title": "note.md mit ChatGPT (Work-Modus) nutzen — generiere in einen Vault, der dir gehört",
 "desc": "ChatGPTs Work-Modus verbindet sich mit deinen Ordnern und ist stark beim Entwerfen und bei der Bildgenerierung. Richte ihn auf einen note.md-Vault, und alles, was er macht, wird Markdown, das du lesen, annotieren und behalten kannst.",
 "crumb": "Integrationen",
 "h1": "note.md + ChatGPT (Work)",
 "lead": "ChatGPT ist der stärkste Generalist, den die meisten schon haben — großartig beim Entwerfen, Zusammenfassen und Generieren von Bildern. note.md gibt dem, was er produziert, ein dauerhaftes Zuhause: dein Vault, deine Dateien, dein Urteil obendrauf.",
 "sections": [
  ("Warum das Paar funktioniert", """<p>ChatGPTs Work-Modus verbindet sich mit Ordnern und Dateien und glänzt am generativen Ende der Pipeline: eine grobe Outline in einen Entwurf verwandeln, einen Stapel Dokumente zusammenfassen und — zunehmend — Bilder und Diagramme im Batch generieren. Sich selbst überlassen, lebt dieser Output in einem Chat-Thread, den du verlieren wirst. Richte ihn stattdessen auf einen note.md-Vault, und jedes Deliverable landet als reines Markdown (mit den Bildern daneben), wo dein Lese-Annotations-Loop es auffangen kann.</p>
<p>Hier verdient die Idee "ein Vault, viele Agents" ihr Geld: ChatGPT ist selten dein einziger Agent. Es ist der schnelle Generalist, zu dem du greifst, um zu <em>produzieren</em> — und das Review, die langlaufende Automatisierung und das finale Urteil können jeweils zu dem gehen, der darin am besten ist. Dieselben Dateien, andere Arbeiter.</p>"""),
  ("Setup", """<ol>
<li>Halte deinen Vault in einem Ordner, den ChatGPT erreichen kann — einem OpenAI-verbundenen Ordner oder einem cloud-/git-synchronisierten Verzeichnis, das er lesen und schreiben kann.</li>
<li>Leg eine <code>AGENTS.md</code> in die Vault-Wurzel (Konventions-Zusammenfassung: <a href="/llms-full.txt">llms-full.txt</a>) und füge dieselben Hausregeln in deine ChatGPT-Projektinstruktionen ein — er liest die Datei nicht automatisch wie ein CLI-Agent, also sag es ihm.</li>
<li>Bitte ihn, Deliverables als datiertes Markdown in den Vault zu speichern, z. B. <code>drafts/2026-07-23-launch-post.md</code>, und generierte Bilder mit relativen Links in <code>{docname}_files/</code> abzulegen.</li>
<li>Öffne das Ergebnis in note.md; lies, markiere, hinterfrage — deine Markierungen landen in Sidecar-Dateien (<code>.note.md</code>), die Quelle bleibt sauber und regenerierbar.</li>
</ol>"""),
  ("Der Loop in der Praxis", """<p>Du bittest ChatGPT, einen Launch-Post zu entwerfen und drei Hero-Bilder zu generieren; er schreibt <code>drafts/launch-post.md</code> und füllt einen <code>_files/</code>-Ordner. Du liest es in note.md, streichst zwei Bilder, markierst einen Absatz, der übertreibt, und hinterlässt eine Notiz. Als Nächstes gibst du <code>launch-post.note.md</code> an einen sorgfältigeren Reviewer-Agent — "geh auf die Randnotizen ein". ChatGPT hat schnell generiert; der Vault hat es behalten; du hast es beurteilt. Das ist die Arbeitsteilung.</p>"""),
 ],
 "faq": [
  ("Liest ChatGPT AGENTS.md automatisch?",
   "Nicht so wie ein CLI-Agent (Codex, Claude Code). Füge deine Vault-Konventionen in das ChatGPT-Projekt oder die Custom Instructions ein und weise es auf die AGENTS.md-Datei hin, damit es sich an dieselben Hausregeln hält — Sidecar-Dateien sind tabu, neue Arbeit ist datiertes Markdown."),
  ("Können von ChatGPT generierte Bilder in meinem Vault leben?",
   "Ja. Speichere sie neben dem Dokument in einem {docname}_files/-Ordner mit relativen Links — dieselbe Konvention, die note.md für eingefügte Screenshots nutzt. Sie rendern in der Leseansicht und reisen mit dem Vault in Git mit."),
  ("Muss ich mich für einen Agent entscheiden?",
   "Nein — genau das ist der Punkt. Nutz ChatGPT für schnelle Generierung, einen anderen Agent für sorgfältiges Review, einen lokalen Agent für private Arbeit. Sie kollaborieren über die Dateien; du orchestrierst. Siehe die Orchestrierungs-Anleitung."),
 ],
},
{
 "path": "/orchestrate-agents/",
 "title": "Ein Vault, viele Agents — Cowork, Codex, OpenClaw & ChatGPT orchestrieren (2026)",
 "desc": "Dein Markdown-Vault ist ein Git-Repo für Agents. Claude Cowork, Claude Code, Codex, ChatGPT, OpenClaw und Hermes können alle dieselben Dateien lesen und schreiben — du weist also jeden Job dem zu, der ihn am besten kann, auf welchem Modell auch immer, und behältst das Urteil für dich.",
 "crumb": "Anleitung",
 "h1": "Ein Vault, viele Agents. Du orchestrierst.",
 "lead": "Der Lock-in, vor dem dich niemand warnt, ist nicht die App — es ist der Agent. Halte dein Wissen in reinen Dateien, und keine einzelne AI besitzt es. Cowork entwirft, Codex refactored, ChatGPT generiert, ein lokaler Agent hütet deine Geheimnisse — und du hältst den Stift.",
 "sections": [
  ("Der Vault ist neutraler Boden", """<p>Die meisten AI-Tools wollen das Zuhause deines Denkens sein: dein Wissen in ihrer Datenbank, deine Annotationen in ihrem Format, dein Agent der, den sie ausliefern, dein Modell das, an das sie dich binden. Dann wird aus "welche AI nutze ich?" die Frage "migriere ich alles?" — und du sitzt eingezäunt in der Roadmap eines einzigen Anbieters.</p>
<p>Ein note.md-Vault dreht das um. Der Vault ist ein Ordner voller reinem Markdown mit gemeinsamen Konventionen — einer <code>AGENTS.md</code>, die die Hausregeln festhält, <code>((file#b-xxxxxx))</code>-Block-Zitaten für präzise Referenzen, Sidecar-Dateien <code>.note.md</code>, die <em>dein</em> Urteil halten, und <code>[[wikilinks]]</code> für einen einzigen Namespace. Diese Konventionen sind ein <b>öffentliches Protokoll</b>: Jeder Agent kann sie lesen, kein Adapter nötig. Die Agents und Modelle werden zu austauschbaren Arbeitern; der Vault ist das eine, das sich nicht ändert. Es ist ein Git-Repo, und sie committen alle hinein.</p>"""),
  ("Weise jeden Job dem zu, der ihn am besten kann", """<p>Kein einzelner Agent ist in allem am besten. Also lass nicht einen alles machen — bau ein Fließband und stell jedes Tool an die Station, an der es am stärksten ist:</p>
<table><thead><tr><th>Stufe</th><th>Gute Wahl</th><th>Warum</th></tr></thead><tbody>
<tr><td>Nächtliche Automatisierung</td><td>OpenClaw / Hermes</td><td>Langlaufend, dateibasiert, selbst gehostetes Gedächtnis</td></tr>
<tr><td>Sorgfältiges Review &amp; Revision</td><td>Claude Cowork / Code</td><td>Starkes Reasoning; liest deine Randnotizen vor dem Editieren</td></tr>
<tr><td>Schnelles Entwerfen &amp; Bilder</td><td>ChatGPT (Work-Modus)</td><td>Generalisten-Generierung, Batch-Bilderstellung</td></tr>
<tr><td>In-Repo-Refactors &amp; Skripte</td><td>Codex</td><td>Native <code>AGENTS.md</code>, läuft im Arbeitsverzeichnis</td></tr>
<tr><td>Finales Urteil</td><td>Du</td><td>Das eine, das kein Modell generieren kann</td></tr>
</tbody></table>
<p>Du wählst den Agent <em>und</em> das Modell pro Job — ein billiges, schnelles Modell zum Triagieren, ein Frontier-Modell zum Reasonen, ein lokales Modell für alles Private. Dem Vault ist egal, welches; er hält nur die Dateien, die sie untereinander weiterreichen.</p>"""),
  ("Ein Loop in der Praxis", """<p>Hier eine echte Pipeline, vier Tools und drei Modelle über einen Vault:</p>
<ol>
<li><b>OpenClaw</b> läuft über Nacht und verarbeitet einen Batch roher Notizen zu <code>drafts/*.md</code>.</li>
<li>Du gibst die Entwürfe an <b>Claude Cowork</b> auf einem sorgfältigen Modell — "reviewe und überarbeite diese, markiere alles Wackelige".</li>
<li><b>ChatGPT</b> generiert die Hero-Bilder im Batch in den <code>_files/</code>-Ordner jedes Dokuments.</li>
<li>Die fertigen Dokumente landen in note.md, wo <b>du</b> sie liest, streichst, was übertreibt, hervorhebst, was zählt, und die Notizen hinterlässt, die nur du schreiben konntest.</li>
</ol>
<p>Vier Tools, drei Modelle, ein Vault, ein Orchestrator. Niemand musste Gedächtnis teilen oder ein privates Protokoll sprechen — sie reichten <code>.md</code>-Dateien auf der Platte weiter, und deine Sidecar-Annotationen (<code>.note.md</code>) waren das Steuersignal für den Nächsten.</p>"""),
  ("Warum Dateien es funktionieren lassen", """<ul>
<li><b>Anti-Lock-in, eine Ebene tiefer.</b> Files-over-App befreit dich von der App; dies befreit dich vom Agent und vom Modell. Das beste Modell von heute ist nächsten Monat ersetzt — dein Wissen sollte nicht mitwandern.</li>
<li><b>Kollaboration ohne Plattform.</b> Agents reichen <code>.md</code> auf der Platte weiter — kein geteiltes Gedächtnis, keine private API, kein Plugin-Store. Der Output des einen Agents ist der Input des nächsten.</li>
<li><b>Du bleibst im Loop, am Checkpoint.</b> Es ist keine Blackbox, die von Anfang bis Ende durchläuft; es ist ein Fließband aus Stationen mit einem Menschen in der Qualitätskontrolle. Agents schreiben, reviewen und illustrieren — du entscheidest, was rausgeht.</li>
<li><b>Immer noch nur Dateien.</b> Keine Orchestrierungs-Datenbank, kein versteckter Zustand. Regeln in <code>AGENTS.md</code>, Output in <code>.md</code>, Urteil in <code>.note.md</code> — alles lesbar von Obsidian, einer CLI oder jedem Agent. Tausch note.md aus, und der Vault ist immer noch der gemeinsame Arbeitsraum aller.</li>
</ul>
<p>Halte den Vault in <a href="/guides/vault-on-github/">Git</a>, und jeder Agent-Write ist diffbar, zuordenbar und rückgängig zu machen — der schlechte Tag eines Agents ist ein <code>git revert</code>, keine Tragödie.</p>"""),
  ("Richte es ein", """<ol>
<li>Leg eine <code>AGENTS.md</code> in die Vault-Wurzel — hol dir die Konventionen aus <a href="/llms-full.txt">llms-full.txt</a> und füge Hausregeln hinzu (die harte: Agents schreiben nie in <code>*.note.md</code>-Sidecar-Dateien).</li>
<li>Verdrahte jeden Agent auf denselben Ordner: <a href="/integrations/openclaw/">OpenClaw</a>, <a href="/integrations/cowork/">Cowork</a>, <a href="/integrations/codex/">Codex</a>, <a href="/integrations/chatgpt-work/">ChatGPT</a>, <a href="/integrations/hermes/">Hermes</a>.</li>
<li>Lies und annotiere die Ergebnisse in note.md; sag dem nächsten Agent, er soll zuerst die Sidecar-Dateien lesen. Der Loop schließt sich auf deiner Platte.</li>
</ol>"""),
 ],
 "faq": [
  ("Können verschiedene AI-Agents sich wirklich einen Vault teilen?",
   "Ja — das ist das Design. Ein Vault ist reines Markdown plus eine AGENTS.md, die die Konventionen beschreibt. Claude Cowork, Claude Code, Codex, ChatGPT, OpenClaw und Hermes lesen und schreiben alle diese Dateien, du kannst also jede Aufgabe an den Agent (und das Modell) routen, der dafür am besten ist. Halte den Vault in Git, damit jeder Write diffbar und rückgängig zu machen ist."),
  ("Wie reichen Agents Arbeit aneinander weiter?",
   "Über Dateien. Ein Agent schreibt Markdown in den Vault; der nächste liest es als Input. Deine Annotationen leben in Sidecar-Dateien (.note.md) und wirken als Steuersignal — ein Agent liest deine Randnotizen vor seinem nächsten Durchgang. Kein geteiltes Gedächtnis, kein privates Protokoll nötig."),
  ("Braucht das ein spezielles Orchestrierungs-Tool oder einen MCP-Server?",
   "Nein. Die Orchestrierung bist du, und das Medium ist das Dateisystem. Es gibt keine zentrale Datenbank und keinen versteckten Zustand — Regeln in AGENTS.md, Output in .md, Urteil in .note.md. Ein Vault-MCP-Server ist auf der Roadmap für Agents, die ein Tool-Interface bevorzugen, aber reine Dateien funktionieren schon heute."),
  ("Warum nicht einfach eine AI für alles nutzen?",
   "Weil kein einzelner Agent in allem am besten ist. Nächtliche Automatisierung, sorgfältiges Review, schnelle Bildgenerierung, private lokale Arbeit und finales Urteil sind verschiedene Jobs mit verschiedenen Best-Fit-Tools. Sie auf Spezialisten aufzuteilen — über Dateien, die dir gehören — schlägt einen Generalisten, der alles macht, und lässt dich frei, jeden Arbeiter auszutauschen."),
 ],
},
]
