// The plugins.notemd.net landing page — a self-contained HTML document served
// by the registry worker at `GET /`. Style tokens mirror the main site
// (notemd.net): dark sticky nav, paper background, Playfair serif headings,
// Courier Prime mono accents, amber (#F59E0B) primary.
//
// The plugin cards are rendered client-side from `/api/index.json`, so the page
// always reflects the currently-published versions with no redeploy. Copy is
// bilingual (EN / 中文) via an inline dictionary; the per-plugin "how to use"
// entry strings live in ENTRY_MAP (keyed by plugin id, both languages), derived
// from each plugin manifest's `contributes.menus.location`.

const SITE = 'https://notemd.net'

export const PAGE_HTML = `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>note.md plugins</title>
<meta name="description" content="Official plugin marketplace for note.md — install plugins and see where to use each one inside the app.">
<link rel="icon" href="${SITE}/favicon.svg" type="image/svg+xml">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Playfair+Display:ital,wght@0,400..900;1,400..900&family=EB+Garamond:ital,wght@0,400..700;1,400..700&family=Courier+Prime:wght@400;700&display=swap" rel="stylesheet">
<style>
:root{--ink:#17181C;--paper:#FAFAF7;--amber:#F59E0B;--gray:#9CA3AF;--line:#E7E5E0;
--serif:"Playfair Display",Georgia,serif;--body:"EB Garamond",Georgia,serif;
--mono:"Courier Prime","Courier New",monospace;}
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:var(--body);background:var(--paper);color:var(--ink);line-height:1.7;font-size:17.5px;-webkit-font-smoothing:antialiased}
a{color:inherit}
.wrap{max-width:960px;margin:0 auto;padding:0 28px}
nav{position:sticky;top:0;z-index:50;background:rgba(23,24,28,.9);backdrop-filter:blur(12px);border-bottom:1px solid #26282F}
.nav-in{display:flex;align-items:center;gap:24px;height:60px;color:var(--paper)}
.logo{display:flex;align-items:center;gap:10px;font-weight:700;font-size:16px;font-family:var(--mono);text-decoration:none}
.logo .dot{color:var(--amber)}
.nav-links{display:flex;gap:22px;font-size:13.5px;color:#B9BDC7;font-family:var(--mono)}
.nav-links a{text-decoration:none;border-bottom:1px dotted transparent;padding-bottom:2px}
.nav-links a:hover{color:#fff;border-bottom-color:var(--amber)}
.nav-links a.on{color:var(--amber);border-bottom-color:var(--amber)}
.lang-sw{margin-left:auto;display:flex;gap:12px;font-family:var(--mono);font-size:12px;color:#7C8290}
.lang-sw a{text-decoration:none;cursor:pointer;padding-bottom:2px;border-bottom:1px dotted transparent}
.lang-sw a:hover{color:#fff}
.lang-sw a.on{color:var(--amber);border-bottom-color:var(--amber)}
.nav-cta{font-family:var(--mono);background:var(--amber);color:var(--ink);font-weight:700;font-size:13px;padding:7px 16px;border-radius:8px;text-decoration:none}
.nav-cta:hover{filter:brightness(1.08)}
header.ph{background:var(--ink);color:var(--paper);padding:72px 0 64px}
.crumb{font-family:var(--mono);font-size:12px;letter-spacing:.18em;text-transform:uppercase;color:var(--amber);margin-bottom:20px}
h1{font-family:var(--serif);font-size:44px;line-height:1.14;font-weight:700;margin-bottom:18px}
.lead{font-size:19px;color:#C3C7CF;font-style:italic;max-width:660px}
main{padding:52px 0 24px}
h2{font-family:var(--serif);font-size:27px;margin:0 0 14px;font-weight:700}
.install{background:#fff;border:1px solid var(--line);border-radius:14px;padding:26px 28px;margin-bottom:40px}
.install ol{margin:12px 0 0;padding-left:22px}
.install li{margin:0 0 8px;color:#33363D}
.install code{font-family:var(--mono);font-size:.9em;background:#FFF9EE;padding:1px 6px;border-radius:5px}
.plugin-group{margin:0 0 34px}
.group-title{font-family:var(--mono);font-size:13px;letter-spacing:.12em;text-transform:uppercase;color:#626874;margin:0 0 12px}
.sys-tag{display:inline-block;margin-left:8px;padding:1px 7px;border:1px solid #D7DAE0;border-radius:20px;color:#7A808C;font-size:9px;letter-spacing:.05em;vertical-align:1px}
.grid{display:grid;grid-template-columns:1fr 1fr;gap:20px}
.card{background:#fff;border:1px solid var(--line);border-radius:14px;padding:24px 24px 22px;display:flex;flex-direction:column}
.card-top{display:flex;align-items:baseline;gap:12px;margin-bottom:10px}
.card h3{font-family:var(--serif);font-size:22px;font-weight:700}
.ai-card{border-color:#B8C8FF;box-shadow:0 8px 28px rgba(50,82,180,.08)}
.ai{font-family:var(--mono);font-size:10px;font-weight:700;letter-spacing:.08em;color:#3154B5;background:#EEF2FF;border:1px solid #D7E0FF;padding:2px 7px;border-radius:20px;white-space:nowrap}
.ver{font-family:var(--mono);font-size:12px;font-weight:700;color:var(--ink);background:var(--amber);padding:2px 9px;border-radius:20px;white-space:nowrap}
.desc{color:#33363D;font-size:16px;margin-bottom:16px;flex:1}
.entry{border-top:1px solid var(--line);padding-top:13px;font-size:15px}
.entry .lbl{font-family:var(--mono);font-size:11px;letter-spacing:.12em;text-transform:uppercase;color:var(--gray);display:block;margin-bottom:4px}
.entry code{font-family:var(--mono);font-size:.9em;background:#FFF9EE;padding:1px 5px;border-radius:4px}
.host{font-family:var(--mono);font-size:12px;color:var(--gray);margin-top:11px}
.msg{grid-column:1/-1;text-align:center;color:var(--gray);font-style:italic;padding:40px 0}
footer{background:var(--ink);color:#7C8290;font-size:13.5px;padding:34px 0 44px;margin-top:64px}
.fbase{font-family:var(--mono);font-size:12.5px}
.fbase a{color:#B9BDC7;text-decoration:none;border-bottom:1px dotted #7C8290}
.fbase a:hover{color:#fff}
html[lang="zh"]{--serif:"Playfair Display","Songti SC","Noto Serif SC",STSong,serif;--body:"EB Garamond","Songti SC","Noto Serif SC",STSong,serif}
html[lang="zh"] .lead{font-style:normal}
@media(max-width:720px){
h1{font-size:30px}h2{font-size:22px}
.grid{grid-template-columns:1fr}
.nav-cta{display:none}
.nav-in{gap:12px;height:54px}
.nav-links{gap:14px;font-size:12.5px}
.logo{font-size:15px}
header.ph{padding:48px 0 42px}
.lead{font-size:17px}
main{padding:38px 0 16px}
.lang-sw{gap:9px;font-size:11.5px}
}
</style>
</head>
<body>
<nav><div class="wrap nav-in">
<a class="logo" href="${SITE}"><span>note<span class="dot">.</span>md</span></a>
<div class="nav-links">
<a href="${SITE}" data-t="nav_home">note.md</a>
<a href="/" class="on" data-t="nav_plugins">Plugins</a>
</div>
<div class="lang-sw">
<a data-lang="en">EN</a>
<a data-lang="zh">中文</a>
</div>
<a class="nav-cta" href="${SITE}/download" data-t="nav_download">Download</a>
</div></nav>

<header class="ph"><div class="wrap">
<div class="crumb" data-t="crumb">Plugin Marketplace</div>
<h1 data-t="title">Plugins for note.md</h1>
<p class="lead" data-t="lead">Small, signed native plugins that add features to note.md — export, import, chat, and more. Install from inside the app; each plugin's process is its own.</p>
</div></header>

<main class="wrap">
<section class="install">
<h2 data-t="install_h">How to install</h2>
<ol>
<li data-t="install_1">Open note.md and go to the <b>Plugins</b> menu → <b>Plugin Marketplace</b>.</li>
<li data-t="install_2">Find a plugin below and click <b>Install</b> — packages are minisign-signed and verified on your machine.</li>
<li data-t="install_3">Use it from the entry shown on each card. Updates appear in the same marketplace.</li>
</ol>
</section>

<h2 data-t="latest_h">Latest plugins</h2>
<div id="grid">
<div class="msg" data-t="loading">Loading plugins…</div>
</div>
</main>

<footer><div class="wrap fbase">
note<span style="color:var(--amber)">.</span>md — <a href="${SITE}">notemd.net</a> · <span data-t="foot">Official plugin marketplace</span>
</div></footer>

<script>
(function(){
var SITE='${SITE}';
var I18N={
en:{
 nav_home:'note.md',nav_plugins:'Plugins',nav_download:'Download',
 crumb:'Plugin Marketplace',title:'Plugins for note.md',
 lead:"Small, signed native plugins that add features to note.md — export, import, chat, and more. Install from inside the app; each plugin's process is its own.",
 install_h:'How to install',
 install_1:'Open note.md and go to the <b>Plugins</b> menu → <b>Plugin Marketplace</b>.',
 install_2:'Find a plugin below and click <b>Install</b> — packages are minisign-signed and verified on your machine.',
 install_3:'Use it from the entry shown on each card. Updates appear in the same marketplace.',
 latest_h:'Latest plugins',loading:'Loading plugins…',
 foot:'Official plugin marketplace',
 entry_lbl:'How to use',host:'Requires note.md ',
 err:"Couldn't load plugins, please retry later.",empty:'No plugins published yet.',
 fallback:'Enable it from the Plugins menu in note.md after install.',
 group_record:'Capture',group_reading:'Read',group_inspiration:'Ideas',
 group_advance:'Move Forward',group_reflect:'Reflect',group_create:'Create',group_import_export:'Import & Export',group_experience:'Experience',group_other:'Other',system:'System Feature'
},
zh:{
 nav_home:'note.md 主站',nav_plugins:'插件',nav_download:'下载',
 crumb:'插件市场',title:'note.md 插件市场',
 lead:'一批小巧、签名的原生插件，为 note.md 扩展能力——导出、导入、对话等。全部从 App 内安装，每个插件独立进程运行。',
 install_h:'如何安装',
 install_1:'打开 note.md，进入顶部「<b>插件</b>」菜单 →「<b>插件市场</b>」。',
 install_2:'在下方找到想要的插件，点「<b>安装</b>」——安装包经 minisign 签名，在你本机校验。',
 install_3:'安装后按每张卡片标注的入口使用。更新也在同一个插件市场里。',
 latest_h:'最新插件',loading:'正在加载插件…',
 foot:'官方插件市场',
 entry_lbl:'使用入口',host:'需要 note.md ',
 err:'暂时无法加载插件列表，请稍后重试。',empty:'暂无已上架插件。',
 fallback:'安装后在 note.md 的「插件」菜单中启用。',
 group_record:'记录',group_reading:'阅读',group_inspiration:'灵感',
 group_advance:'推进',group_reflect:'回顾',group_create:'创作',group_import_export:'导入与导出',group_experience:'体验增强',group_other:'其他',system:'系统功能'
}};
// Per-plugin entry, from each manifest's contributes.menus.location.
// Every plugin command lands in the Plugins menu regardless of the location its
// manifest declares (src-tauri/src/lib.rs builds it that way), so these all name
// that one menu. Labels are the plugin's own localized menu text.
// (No backticks in here: this block lives inside the PAGE_HTML template string.)
var ENTRY_MAP={
 'notemd.claude-agent':{en:'<b>Plugins</b> → <b>Move Forward</b> → Claude Agent… (also CLI <code>notemd agent</code>)',zh:'「<b>插件</b>」→「<b>推进</b>」→ Claude 智能体…（也支持 CLI <code>notemd agent</code>）'},
 'notemd.codex-agent':{en:'<b>Plugins</b> → <b>Move Forward</b> → Codex Agent…',zh:'「<b>插件</b>」→「<b>推进</b>」→ Codex 智能体…'},
 'notemd.deepseek-agent':{en:'<b>Plugins</b> → <b>Move Forward</b> → DeepSeek Agent…',zh:'「<b>插件</b>」→「<b>推进</b>」→ DeepSeek 智能体…'},
 'notemd.openclaw-chat':{en:'<b>Plugins</b> → <b>Move Forward</b> → OpenClaw',zh:'「<b>插件</b>」→「<b>推进</b>」→ OpenClaw'},
 'notemd.next':{en:'<b>Plugins</b> → <b>Move Forward</b> → Next',zh:'「<b>插件</b>」→「<b>推进</b>」→ 下一步'},
 'notemd.idea-spark':{en:'<b>Plugins</b> → <b>Ideas</b> → Idea Spark',zh:'「<b>插件</b>」→「<b>灵感</b>」→ 奇思妙想'},
 'notemd.ebook-import':{en:'<b>Plugins</b> → <b>Read</b> → Import Ebooks… (also CLI <code>notemd ebook</code>)',zh:'「<b>插件</b>」→「<b>阅读</b>」→ 导入电子书…（也支持 CLI <code>notemd ebook</code>）'},
 'notemd.roam-import':{en:'<b>Plugins</b> → <b>Import & Export</b> → Import from Roam Research…',zh:'「<b>插件</b>」→「<b>导入与导出</b>」→ 从 Roam Research 导入…'},
 'notemd.pos-log':{en:'<b>Plugins</b> → <b>Capture</b> → Location Log',zh:'「<b>插件</b>」→「<b>记录</b>」→ 位置记录'},
 'notemd.trace-source':{en:'<b>Plugins</b> → <b>Read</b> → Trace Source',zh:'「<b>插件</b>」→「<b>阅读</b>」→ 溯源'},
 'notemd.decision-log':{en:'<b>Plugins</b> → <b>Reflect</b> → Decision Log',zh:'「<b>插件</b>」→「<b>回顾</b>」→ 决策日志'},
 'notemd.weekly-review':{en:'<b>Plugins</b> → <b>Reflect</b> → Weekly Review',zh:'「<b>插件</b>」→「<b>回顾</b>」→ 周检视'},
 'notemd.md2pdf':{en:'<b>Plugins</b> → <b>Import & Export</b> → Export to PDF… (also CLI <code>notemd pdf</code>)',zh:'「<b>插件</b>」→「<b>导入与导出</b>」→ 导出为 PDF…（也支持 CLI <code>notemd pdf</code>）'},
 'notemd.power-mode':{en:'<b>Plugins</b> → <b>Experience</b> → Power Mode',zh:'「<b>插件</b>」→「<b>体验增强</b>」→ 狂暴模式'}
};
function pickLang(){
 var q=new URLSearchParams(location.search).get('lang');
 if(q==='zh'||q==='en')return q;
 return (navigator.language||'').toLowerCase().indexOf('zh')===0?'zh':'en';
}
var lang=pickLang();
function esc(s){return String(s==null?'':s).replace(/[&<>"]/g,function(c){return{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c];});}
function applyStatic(){
 var d=I18N[lang];
 document.documentElement.lang=lang;
 document.querySelectorAll('[data-t]').forEach(function(el){
  var k=el.getAttribute('data-t');if(d[k]!=null)el.innerHTML=d[k];
 });
 document.querySelectorAll('.lang-sw a').forEach(function(a){
  a.classList.toggle('on',a.getAttribute('data-lang')===lang);
 });
}
function entryFor(id){
 var e=ENTRY_MAP[id];return e?e[lang]:I18N[lang].fallback;
}
var GROUP_ORDER=['record','reading','inspiration','advance','reflect','create','import-export','experience','other'];
var LEGACY_GROUPS={'agents':'advance','capture':'record','capture-import':'record','thinking':'reflect','thinking-review':'reflect','publish-export':'import-export','editing':'experience','editor-extensions':'experience'};
var OFFICIAL_GROUPS={'notemd.pos-log':'record','notemd.roam-import':'import-export','notemd.ebook-import':'reading','notemd.trace-source':'reading','notemd.idea-spark':'inspiration','notemd.next':'advance','notemd.claude-agent':'advance','notemd.codex-agent':'advance','notemd.deepseek-agent':'advance','notemd.openclaw-chat':'advance','notemd.decision-log':'reflect','notemd.weekly-review':'reflect','notemd.md2pdf':'import-export','notemd.power-mode':'experience'};
var AI_ROLES={'notemd.ebook-import':{en:'AI Read',zh:'AI 阅读'},'notemd.idea-spark':{en:'AI Inspire',zh:'AI 启发'},'notemd.trace-source':{en:'AI Reason',zh:'AI 推理'},'notemd.claude-agent':{en:'AI Action',zh:'AI 执行'},'notemd.codex-agent':{en:'AI Action',zh:'AI 执行'},'notemd.deepseek-agent':{en:'AI Action',zh:'AI 执行'},'notemd.openclaw-chat':{en:'AI Action',zh:'AI 执行'}};
function normalizeGroup(value,id){return OFFICIAL_GROUPS[id]||(GROUP_ORDER.indexOf(value)>=0?value:(LEGACY_GROUPS[value]||'other'));}
function groupLabel(key){
 return I18N[lang]['group_'+key.replace(/-/g,'_')]||I18N[lang].group_other;
}
function isSystemGroup(key){return key==='import-export'||key==='experience';}
function groupPlugins(list){
 var groups={};GROUP_ORDER.forEach(function(key){groups[key]=[];});
 (list||[]).forEach(function(plugin){groups[normalizeGroup(plugin.category,plugin.id)].push(plugin);});
 return GROUP_ORDER.filter(function(key){return groups[key].length;}).map(function(key){
  return {key:key,items:groups[key]};
 });
}
function renderCard(p,d){
 var host=p.min_host?'<div class="host">'+esc(d.host)+esc(p.min_host)+'</div>':'';
 var ai=AI_ROLES[p.id];
 return '<div class="card'+(ai?' ai-card':'')+'">'+
  '<div class="card-top"><h3>'+esc(p.name||p.id)+'</h3>'+
  (ai?'<span class="ai">'+esc(ai[lang])+'</span>':'')+
  (p.version?'<span class="ver">v'+esc(p.version)+'</span>':'')+'</div>'+
  '<p class="desc">'+esc(p.description||'')+'</p>'+
  '<div class="entry"><span class="lbl">'+esc(d.entry_lbl)+'</span>'+entryFor(p.id)+host+'</div>'+
  '</div>';
}
function renderPlugins(list){
 var d=I18N[lang];var grid=document.getElementById('grid');
 if(!list||!list.length){grid.innerHTML='<div class="msg">'+esc(d.empty)+'</div>';return;}
 grid.innerHTML=groupPlugins(list).map(function(group){
  return '<section class="plugin-group"><h3 class="group-title">'+esc(groupLabel(group.key))+(isSystemGroup(group.key)?'<span class="sys-tag">'+esc(d.system)+'</span>':'')+'</h3>'+
   '<div class="grid">'+group.items.map(function(p){return renderCard(p,d);}).join('')+'</div></section>';
 }).join('');
}
function cmpVer(a,b){
 a=(a||'0').split('.');b=(b||'0').split('.');
 for(var i=0;i<3;i++){var x=parseInt(a[i]||0,10),y=parseInt(b[i]||0,10);if(x!==y)return x-y;}
 return 0;
}
// The registry index lists one entry per <id>/<version>; collapse to one card
// per plugin (highest version) so multi-version plugins don't render duplicates.
function latestById(list){
 var m={},order=[];
 (list||[]).forEach(function(p){
  var id=p.id;if(!(id in m)){order.push(id);m[id]=p;}
  else if(cmpVer(p.version,m[id].version)>0)m[id]=p;
 });
 return order.map(function(id){return m[id];});
}
var CACHE=null;
function load(){
 var grid=document.getElementById('grid');
 grid.innerHTML='<div class="msg">'+esc(I18N[lang].loading)+'</div>';
 fetch('/api/index.json').then(function(r){return r.json();}).then(function(j){
  CACHE=latestById((j&&j.plugins)||[]);renderPlugins(CACHE);
 }).catch(function(){
  grid.innerHTML='<div class="msg">'+esc(I18N[lang].err)+'</div>';
 });
}
document.querySelectorAll('.lang-sw a').forEach(function(a){
 a.addEventListener('click',function(){
  lang=a.getAttribute('data-lang');
  var u=new URL(location.href);u.searchParams.set('lang',lang);history.replaceState(null,'',u);
  applyStatic();if(CACHE)renderPlugins(CACHE);else load();
 });
});
applyStatic();
load();
})();
</script>
</body>
</html>`
