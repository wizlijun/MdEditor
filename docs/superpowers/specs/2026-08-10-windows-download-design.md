# notemd.net 的 Windows 下载支持 — 设计

日期:2026-08-10 · 状态:已确认,待实现

## 背景

Windows x64 版已随 v6.808.3 上线(资产 `note.md_6.808.3_x64-setup.exe` + `.sig`)。
但网站 notemd.net 至今只认 macOS:

- `/download` 路由(`website/src/index.js`)对非 mac 访客一律 302 到 GitHub releases 列表页;
- 首页与全部落地页的 CTA 文案写死「Download for macOS」;
- `llms.txt` / `llms-full.txt` 对 agent 声明本产品是 macOS 应用。

另有一个结构性约束:**Windows 包由另一台机器在 mac 发版之后补进同一个 tag**
(见 `docs/windows-agent-brief.md`)。因此存在一个天然的滞后窗口——最新 Release
的 `latest.json` 里可能只有 `darwin-*` 两个平台条目,`windows-*` 要等 Windows 机器
跑完 `scripts/release-windows.ps1` 才会被合并进去。网站必须优雅处理这个窗口。

## 目标

1. Windows 访客点「下载」能直接拿到 `.exe` 安装包,而不是被丢到 releases 列表页。
2. Windows 版滞后时,自动回退到**最近一个真的带 Windows 包的版本**。
3. 缓存最后一次成功解析出的下载地址;GitHub 抖动时用它兜底,绝不让下载按钮空转。
4. 页面文案按访客系统自动切换,并始终提供跨平台的显式链接。

非目标:Linux 构建;Windows ARM64 原生包(尚未构建);站内展示版本号。

## 一、`/download` 路由

### 平台与架构判定

优先级:显式 `?os=` > User-Agent > 兜底。

| 输入 | 结果 |
|---|---|
| `?os=windows` / `?os=win` | windows |
| `?os=mac` / `?os=macos` / `?os=darwin` | mac |
| UA 含 `Windows NT`(且非 `Android`) | windows |
| UA 含 `Macintosh` / `Mac OS X`(且非 iPhone/iPad/iPod) | mac |
| 其余(Linux、iOS、爬虫…) | 302 → releases 列表页 |

架构:

- mac:`?arch=` > `Sec-CH-UA-Arch` 客户端提示 > `aarch64`。**逻辑不变**——Safari
  在 Apple Silicon 上也报 Intel,所以页面另给显式的 Intel 链接。
- windows:`?arch=` > `Sec-CH-UA-Arch` > `x86_64`。仅当 arm64 包**确实存在**时才
  下发 arm64;当前没有 arm64 构建,Windows on ARM 走 x64(系统自带 x64 模拟)。

### 下载地址解析

**mac** 保持现状:从 `latest.json` 取 `version`,从 `platforms["darwin-<arch>"].url`
里反解出 tag,拼 `note.md-<version>-<arch>.dmg`。(updater 产物是 tarball,不是 dmg,
所以必须拼。)

**windows** 三级解析:

1. `latest.json` 的 `platforms["windows-<arch>"].url`。Tauri 2 直接对 NSIS 安装包
   签名(没有 Tauri 1 那个单独的 `.nsis.zip`),所以 updater 的 url **就是**
   `setup.exe` 本身,直接用,不拼文件名。
2. 上一步没命中(滞后窗口):调 GitHub releases 列表 API
   `https://api.github.com/repos/wizlijun/note.md/releases?per_page=100&page=<n>`,
   跳过 draft,逐页找**最新一个**带匹配 `*_<x64|arm64>-setup.exe` 资产的 release,
   取其 `browser_download_url`。必须翻页到命中或遇到最后一个短页，不能假设最近
   30/100 个 Release 一定带 Windows 包。
3. 仍失败(GitHub 5xx / 限流 / 网络):见下节的 last-known-good 缓存。

全部落空才 302 到 releases 列表页。

## 二、缓存

三层,由外到内:

| 层 | 键 | TTL | 作用 |
|---|---|---|---|
| isolate 内存 | manifest / 解析结果 | 与对应 Cache 层同 | 热 isolate 零查表 |
| Cache API `fresh` | `/__cache/dl/<os>-<arch>` | 10 min | 常规命中,直接 302 |
| Cache API `lkg` | `/__cache/dl-lkg/<os>-<arch>` | 30 天 | 最后已知可用地址,仅失败时读 |

解析成功 → 同时写 `fresh` 与 `lkg`。解析失败 → 读 `lkg`;有就用它 302(宁可给一个
稍旧但真实存在的安装包,也不把用户丢到列表页),没有才回列表页。

`latest.json` 自身的 5 分钟缓存保持不变。

## 三、代码结构

纯逻辑抽到 `website/src/resolve-download.js`,不做任何 I/O:

```js
detectPlatform(headers, searchParams) -> { os, arch } | null
macDownloadUrl(manifest, arch)        -> url | null
windowsUrlFromManifest(manifest, arch)-> url | null
windowsUrlFromReleases(releases, arch)-> url | null
```

`website/src/index.js` 只负责路由、fetch、缓存编排。这与
`scripts/merge-latest-json-core.mjs` + `merge-latest-json.mjs` 的拆法一致。

测试:`website/src/resolve-download.test.ts`(vitest,`include` 增加
`website/src/**/*.test.ts`)。覆盖 UA 判定、arch 归一化、manifest 命中、
滞后回退挑版本、跨页查找、资产名不匹配、空列表与 Worker 路由直链。

## 四、页面

### 首页(`public/index.html` + 三份译文)

hero 与底部下载区的主按钮各带一个 `data-dl-cta`。一段内联脚本(无依赖、无网络请求)
按 `navigator.userAgentData?.platform` 或 UA 判定 Windows,若是:

- 图标换成 Windows 四格徽标;
- 文案换成对应语言的「Download for Windows」;
- `href` 换成 `/download?os=windows`。

非 Windows / 禁用 JS → 保持现有 macOS 形态(默认值写在 HTML 里,不闪烁)。

小字始终给跨平台链接:macOS 视角显示「Windows?」,Windows 视角显示「macOS?」;
Intel Mac 链接保留。

四语言:英文母版在 `public/index.html`,德/日/中经 `build_i18n.py` 的 STRINGS 表
生成——新增串必须进表,否则构建会报未匹配。

### 落地页(`build_pages.py`)

落地页是纯静态、不带 JS,CTA 文案由 `CHROME[lang]["cta_btn"]` 提供。把它从
「Download for macOS」改成中性的「Download note.md」(四语言同改),链接仍指
`/download`——路由自己会按 UA 分流。同时更新 `cta_p` 里的「on your Mac」措辞。

### 给 agent 的公共约定

`public/llms.txt` 与 `public/llms-full.txt` 中的平台描述由「macOS app」/
「Platform: macOS 13+」改为同时声明 macOS 13+(Apple Silicon & Intel)与
Windows 10/11 x64,并注明 Windows 包可能比 macOS 晚一到两个版本。

## 五、验证

- `pnpm vitest run website/src/resolve-download.test.ts` 全绿;
- `npx wrangler dev` 本地起 worker,用伪造 UA 打 `/download`:mac / Windows /
  Linux / `?os=windows` / `?arch=x86_64` 各验一次 302 目标;
- 断网/伪造 GitHub 失败,验证 `lkg` 兜底;
- `python3 build_i18n.py && python3 build_pages.py` 无未匹配报错,生成物入库;
- 部署本地执行(`npx wrangler deploy`),不走 CI。
