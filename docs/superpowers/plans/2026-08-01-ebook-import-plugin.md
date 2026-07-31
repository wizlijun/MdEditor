# 电子书导入插件(notemd.ebook-import)实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 bookread.sh 的电子书归档流程重写为 note.md v2 插件:epub/pdf/docx → markdown 归档进 vault `ssot/ebooks/YYYY-MM/<书名>/`,含微信OCR/百度 Unlimited-OCR 双 provider、拖放、队列、CLI。

**Architecture:** 后端+前端插件(样板 claude-agent):Rust 后端跑转换流水线(Calibre 子进程 + htmd + pdfium + reqwest),经 `host.ui.post` 流式推进度;Svelte 窗口做队列 UI;一处核心改动(windows.rs 拖放转发)。Spec:`docs/superpowers/specs/2026-08-01-ebook-import-plugin-design.md`。

**Tech Stack:** Rust(notemd-plugin-sdk、zip、quick-xml、htmd、regex、reqwest/rustls、base64、pdfium-render、image、chrono)、Svelte 5 + Vite、vitest。

## Global Constraints

- 后端 stdout 只写协议行;人读日志一律 `host.log_*`/stderr(SDK 约定)
- 落盘产物 `config.txt + book.md + images/` 与 bookread.sh 逐字段兼容;月份目录用 `YYYY-MM`
- 百度密钥与 Calibre 手动路径只进 `<data_dir>/device.json`,绝不进 vault
- vault 设置文件 `.notemd/ebook-import.json`,解析必须宽容(外部 agent 可能手改)
- `manifest.v2.json` 写多余字段会被 `deny_unknown_fields` 拒;改完必跑 protocol 校验测试
- 主 worktree 多会话共享:每次 commit 只精确 add 本任务文件,绝不 `add -A`
- 插件 UI 是隔离 webview:不 import 主程序 `src/`,宿主能力只走 `window.notemd`(bridge.ts)
- `engines.notemd` 设 `">=6.801.1"`(携带 Task 1 拖放转发的下一版;实际发版号若不同,上架前改一致)
- OCR 相关网络测试全部打本地 mock server,不打真实服务

## 文件结构总览

```
src-tauri/src/plugin_runtime/windows.rs        # Task 1: 拖放转发(唯一核心改动)
plugins-src/ebook-import/
├── manifest.v2.json                           # Task 2
├── package.json / vite.config.ts / tsconfig.json / index.html
├── src/main.ts / App.svelte                   # Task 9
├── src/lib/bridge.ts                          # Task 2(照抄 roam-import)
├── src/lib/strings.ts                         # Task 9(i18n en/zh/ja/de)
├── src/lib/queue.ts + queue.test.ts           # Task 9(队列状态机,纯 TS 可测)
└── backend/
    ├── Cargo.toml                             # Task 2
    ├── src/main.rs                            # Task 2(serve 入口)
    ├── src/plugin.rs                          # Task 8(trait 实现 + 命令分发 + jobs)
    ├── src/settings.rs                        # Task 3(vault/device 两层配置)
    ├── src/bookconf.rs                        # Task 3(config.txt + sanitize_dirname)
    ├── src/calibre.rs                         # Task 4(探测 + 转 HTMLZ)
    ├── src/htmlz.rs                           # Task 5(解包 + opf 元数据 + html→md + 清理)
    ├── src/ocr/mod.rs                         # Task 6(OcrEngine trait + PageRenderer trait)
    ├── src/ocr/wechat.rs                      # Task 6(微信OCR:逐页渲染+POST+断点)
    ├── src/ocr/pdfium.rs                      # Task 6(PdfiumRenderer)
    ├── src/ocr/baidu.rs                       # Task 7(OAuth+提交+轮询+图片本地化)
    └── src/pipeline.rs                        # Task 8(单书流水线 + 落盘 + 冲突后缀)
scripts/fetch-pdfium.sh                        # Task 10
scripts/dev-install-plugin.sh                  # Task 10(加分支)
scripts/release-plugins.sh                     # Task 10(加 case)
```

---

### Task 1: 核心——拖放事件转发给插件窗口

**Files:**
- Modify: `src-tauri/src/plugin_runtime/windows.rs`

**Interfaces:**
- Produces: 插件窗口经 `window.notemd.onMessage(cb)` 收到 `{ type:"drag-drop", phase:"enter"|"leave"|"drop", paths:[...] }`(enter/drop 带 paths,leave 为空数组)。UI(Task 9)依赖此形状。

- [ ] **Step 1: 写纯函数 + 失败测试**。在 `windows.rs` 底部 tests 模块加:

```rust
#[test]
fn drag_drop_payload_shapes() {
    let p = drag_drop_payload("drop", &[std::path::PathBuf::from("/a/b.epub")]);
    assert_eq!(p["type"], "drag-drop");
    assert_eq!(p["phase"], "drop");
    assert_eq!(p["paths"][0], "/a/b.epub");
    let e = drag_drop_payload("leave", &[]);
    assert_eq!(e["phase"], "leave");
    assert!(e["paths"].as_array().unwrap().is_empty());
}
```

- [ ] **Step 2: 跑测试确认编译失败**(函数不存在):`cd src-tauri && cargo test drag_drop_payload` → FAIL
- [ ] **Step 3: 实现**。`windows.rs` 加纯函数,并在 `open_plugin_window` 现有 `window.on_window_event` 闭包里(`WindowEvent::Destroyed` 分支旁)接 DragDrop。闭包需要 window 句柄自身,改为先 clone 一个 `app_handle` + `label`,事件里用 `app.get_webview_window(&label)` 取:

```rust
/// Payload pushed into a plugin window for OS drag-drop (spec §8).
pub(crate) fn drag_drop_payload(phase: &str, paths: &[std::path::PathBuf]) -> Value {
    serde_json::json!({
        "type": "drag-drop",
        "phase": phase,
        "paths": paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
    })
}
```

事件接线(在既有 `on_window_event` 闭包里加一个分支;`tauri::DragDropEvent` 的 Enter/Drop 带 `paths`,Leave 无):

```rust
let app2 = app.clone();
let label2 = label.clone();
window.on_window_event(move |event| {
    match event {
        WindowEvent::DragDrop(dd) => {
            let payload = match dd {
                tauri::DragDropEvent::Enter { paths, .. } => drag_drop_payload("enter", paths),
                tauri::DragDropEvent::Drop { paths, .. } => drag_drop_payload("drop", paths),
                tauri::DragDropEvent::Leave => drag_drop_payload("leave", &[]),
                _ => return, // Over: 高频,不转发
            };
            if let Some(w) = app2.get_webview_window(&label2) {
                let _ = w.eval(dispatch_eval(&payload));
            }
        }
        WindowEvent::Destroyed => { /* 既有逻辑原样保留 */ }
        _ => {}
    }
});
```

注意:是**合并进既有闭包**(Destroyed 的 clear_grants + deactivate 逻辑不动),不是再挂一个 handler。
- [ ] **Step 4: 跑测试**:`cargo test drag_drop_payload` → PASS;`cargo test -p mdeditor plugin_runtime 2>/dev/null || cargo test windows` 无回归
- [ ] **Step 5: Commit**:`git add src-tauri/src/plugin_runtime/windows.rs && git commit -m "feat(plugins): forward OS drag-drop events into plugin windows"`

---

### Task 2: 插件脚手架(manifest + 前端工程壳 + 后端 crate 壳)

**Files:**
- Create: `plugins-src/ebook-import/manifest.v2.json`、`package.json`、`vite.config.ts`、`tsconfig.json`、`index.html`、`src/main.ts`、`src/App.svelte`(占位)、`src/lib/bridge.ts`
- Create: `plugins-src/ebook-import/backend/Cargo.toml`、`backend/src/main.rs`、`backend/src/plugin.rs`(骨架)、`backend/.gitignore`(`target/`)

**Interfaces:**
- Produces: 插件 id `notemd.ebook-import`、bin 名 `notemd-ebook-import`、pnpm 包名 `ebook-import-plugin`、窗口 id `main`、CLI 子命令 `ebook`/command `import`。后续所有任务沿用这些名字。

- [ ] **Step 1: manifest.v2.json**(全文):

```json
{
  "manifest_version": 2,
  "id": "notemd.ebook-import",
  "name": "Ebook Import",
  "version": "1.0.0",
  "kind": "native",
  "engines": { "notemd": ">=6.801.1" },
  "description": "Import ebooks (epub/pdf/docx) into the vault as markdown: Calibre conversion, OCR for scanned PDFs (WeChat OCR / Baidu Unlimited-OCR), archived to ssot/ebooks/YYYY-MM/<title>/ with config.txt + book.md + images.",
  "binary": {
    "aarch64-apple-darwin": "bin/notemd-ebook-import",
    "x86_64-apple-darwin": "bin/notemd-ebook-import"
  },
  "ui": "ui/",
  "activation": { "events": ["onCommand:open", "onCli:ebook"] },
  "contributes": {
    "menus": [
      { "location": "plugins", "label": "Import Ebooks (epub/pdf/docx)…", "command": "open" }
    ],
    "windows": [
      { "id": "main", "entry": "index.html", "title": "Ebook Import",
        "width": 900, "height": 640, "min_width": 640, "min_height": 480,
        "open_command": "open" }
    ],
    "cli": [
      { "subcommand": "ebook", "command": "import",
        "summary": "Import an ebook into the vault as markdown",
        "args": [
          { "name": "file", "type": "string", "required": true, "help": "Path to .epub/.pdf/.docx" }
        ],
        "flags": [
          { "long": "--ocr", "type": "boolean", "help": "OCR mode for scanned PDFs" },
          { "long": "--ocr-provider", "type": "string", "help": "wechat | baidu (default: settings)" },
          { "long": "--root", "type": "string", "help": "Vault-relative ebooks root (default: settings)" }
        ] }
    ]
  },
  "capabilities": ["ui", "toast", "dialog", "vault.read", "vault.write", "editor.open"],
  "request_timeout_seconds": 300,
  "i18n": {
    "zh": { "name": "导入电子书", "menus": { "open": "导入电子书(epub、pdf、docx)…" } },
    "ja": { "name": "電子書籍を取り込む", "menus": { "open": "電子書籍を取り込む(epub・pdf・docx)…" } },
    "de": { "name": "E-Books importieren", "menus": { "open": "E-Books importieren (epub/pdf/docx)…" } }
  }
}
```

- [ ] **Step 2: 前端工程壳**。`package.json`(name `"ebook-import-plugin"`,scripts/devDeps 照抄 `plugins-src/claude-agent/package.json`,不要 jsdom 以外多余项);`vite.config.ts`、`tsconfig.json`、`index.html`、`src/main.ts` 照抄 claude-agent 同名文件(标题改 Ebook Import);`src/lib/bridge.ts` **整文件照抄** `plugins-src/roam-import/src/lib/bridge.ts`;`src/App.svelte` 先放占位 `<h1>Ebook Import</h1>`。
- [ ] **Step 3: 后端 crate 壳**。`backend/Cargo.toml`:

```toml
[package]
name = "notemd-ebook-import"
version = "1.0.0"
edition = "2021"

[[bin]]
name = "notemd-ebook-import"
path = "src/main.rs"

[dependencies]
notemd-plugin-sdk = { path = "../../../notemd-plugin-sdk" }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "io-util", "sync"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = "0.4"
regex = "1"
zip = { version = "2", default-features = false, features = ["deflate"] }
quick-xml = "0.37"
htmd = "0.2"
reqwest = { version = "0.12", default-features = false, features = ["blocking", "multipart", "json", "rustls-tls"] }
base64 = "0.22"
pdfium-render = "0.8"
image = { version = "0.25", default-features = false, features = ["png"] }

[dev-dependencies]
tempfile = "3"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
strip = true
```

`src/main.rs`:

```rust
mod bookconf; mod calibre; mod htmlz; mod ocr; mod pipeline; mod plugin; mod settings;

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2).enable_all().build().expect("tokio runtime");
    rt.block_on(notemd_plugin_sdk::serve(plugin::EbookImportPlugin::new()));
}
```

`src/plugin.rs` 骨架(编译得过即可;Task 8 填肉):

```rust
use notemd_plugin_sdk as sdk;
use serde_json::{json, Value};

pub struct EbookImportPlugin { pub data_dir: std::path::PathBuf }
impl EbookImportPlugin { pub fn new() -> Self { Self { data_dir: std::env::temp_dir() } } }

impl sdk::NotemdPlugin for EbookImportPlugin {
    fn initialize(&mut self, _h: &sdk::Host, p: &sdk::InitializeParams) {
        self.data_dir = std::path::PathBuf::from(&p.data_dir);
    }
    fn activate(&mut self, _h: &sdk::Host, _p: &sdk::plugin_protocol::ActivateParams) -> Result<(), String> { Ok(()) }
    fn deactivate(&mut self, _h: &sdk::Host) {}
    fn execute_command(&mut self, _h: &sdk::Host, p: &sdk::ExecuteCommandParams) -> Result<Value, String> {
        Err(format!("unknown command '{}'", p.command))
    }
    fn on_ui_request(&mut self, _h: &sdk::Host, m: &str, _p: Value) -> Result<Value, String> {
        Err(format!("unknown ui method '{m}'"))
    }
}
```

其余 mod 文件先建空文件(`pub(crate) fn _placeholder() {}` 都不要,空文件即可)。
- [ ] **Step 4: 验证**。`pnpm install`(workspace 收新包)→ `pnpm --filter ebook-import-plugin build` 出 dist;`cargo build --manifest-path plugins-src/ebook-import/backend/Cargo.toml`;manifest 校验:`cargo test -p plugin-protocol` 全绿(顺手在 `plugin-protocol` 的 validate 测试无需加例,运行现有套件即可)。
- [ ] **Step 5: Commit**:精确 add `plugins-src/ebook-import/`(注意 backend/Cargo.lock 一起进)+ `pnpm-lock.yaml`。`git commit -m "feat(ebook-import): plugin scaffold (manifest, ui shell, backend crate)"`

---

### Task 3: 后端——两层配置 + config.txt + sanitize_dirname

**Files:**
- Create: `backend/src/settings.rs`、`backend/src/bookconf.rs`(含 tests)

**Interfaces:**
- Produces:
  - `settings::VaultSettings { ebooks_root: String, provider: String, wechat_url: String }`,`load_vault(vault_root) -> VaultSettings`(缺失/坏 JSON → 默认值:`"ssot/ebooks"` / `"wechat"` / `"http://10.17.0.123:8092/ocr"`)、`save_vault(vault_root, &VaultSettings)`(写 `.notemd/ebook-import.json`,建父目录)
  - `settings::DeviceSettings { calibre_path: Option<String>, baidu_api_key: String, baidu_secret_key: String }`,`load_device(data_dir)` / `save_device(data_dir, &DeviceSettings)`(`<data_dir>/device.json`)
  - `bookconf::BookMeta { title, creator, publisher, language: Option<String> ×4 }`
  - `bookconf::write_config_txt(path, input_file, method: &str, meta: &BookMeta)`(method = `"calibre_htmlz"` | `"ocr"`;input_lang=auto、output_lang=zh 固定)
  - `bookconf::sanitize_dirname(&str) -> String`

- [ ] **Step 1: 失败测试**(`bookconf.rs` tests):

```rust
#[test]
fn sanitize_ports_shell_rules() {
    assert_eq!(sanitize_dirname("a/b\\c:d*e?f\"g<h>i|j"), "a_b_c_d_e_f_g_h_i_j");
    assert_eq!(sanitize_dirname("  many   spaces  "), "many spaces");
    assert_eq!(sanitize_dirname("..dots.."), "dots");        // 首尾点和空格都剥
    assert_eq!(sanitize_dirname("x\u{0007}y"), "xy");         // 控制字符
    assert_eq!(sanitize_dirname(&"字".repeat(300)).chars().count(), 200);
    assert_eq!(sanitize_dirname("   "), "");                  // 空→调用方退回文件名
}

#[test]
fn config_txt_matches_bookread_format() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.txt");
    let meta = BookMeta { title: Some("7 Powers".into()), creator: Some("Hamilton".into()),
                          publisher: None, language: Some("en".into()) };
    write_config_txt(&p, "/in/7powers.epub", "calibre_htmlz", &meta).unwrap();
    let s = std::fs::read_to_string(&p).unwrap();
    assert!(s.contains("input_file=/in/7powers.epub"));
    assert!(s.contains("input_lang=auto"));
    assert!(s.contains("output_lang=zh"));
    assert!(s.contains("conversion_method=calibre_htmlz"));
    assert!(s.contains("original_title=7 Powers"));
    assert!(s.contains("creator=Hamilton"));
    assert!(!s.contains("publisher="));
    assert!(s.contains("source_language=en"));
}
```

`settings.rs` tests:默认值(文件缺失)、round-trip、坏 JSON 回默认。
- [ ] **Step 2: 跑测确认失败**:`cargo test --manifest-path plugins-src/ebook-import/backend/Cargo.toml sanitize` → 编译失败
- [ ] **Step 3: 实现**。sanitize 逐条移植 shell 规则(顺序同 bookread.sh):禁字符→`_`;去 `\x00-\x1f`;空白折叠为单空格;trim 首尾 ` ` 和 `.`;截 200 **字符**。config.txt 输出格式(与 python 脚本逐字对齐,含注释行):

```text
# Translation Configuration
input_file={input_file}
input_lang=auto
output_lang=zh
conversion_method={method}

# Book Metadata
original_title={title}
creator={creator}
publisher={publisher}
source_language={language}
```

(`# Book Metadata` 块只在至少一个元数据存在时写;每个键只在有值时写。)settings 两个 struct 全部 `#[serde(default)]` 宽容解析,`serde_json::from_str` 失败即返默认。
- [ ] **Step 4: 跑测**:该 crate `cargo test` 全绿
- [ ] **Step 5: Commit**:`git add plugins-src/ebook-import/backend/src/{settings,bookconf}.rs && git commit -m "feat(ebook-import): settings layers + config.txt + dirname sanitize"`

---

### Task 4: 后端——Calibre 探测与 HTMLZ 转换

**Files:**
- Create: `backend/src/calibre.rs`、`backend/tests/fixtures/ebook-convert-ok.sh`(可执行:`#!/bin/sh\n[ "$1" = "--version" ] && { echo "ebook-convert (calibre 7.0)"; exit 0; }\ncp /dev/null "$2"; exit 0`)、`fixtures/ebook-convert-hang.sh`(`sleep 60`)

**Interfaces:**
- Consumes: `settings::DeviceSettings.calibre_path`
- Produces:
  - `calibre::detect(device_override: Option<&str>) -> Option<Detected>`;`Detected { path: String, version: String }`。候选顺序:override → SharedConfig `calibre_path`(`~/Library/Application Support/com.laobu.mdeditor-shared/config.json` 的 `calibre_path` 键,直接 serde_json 读)→ `/Applications/calibre.app/Contents/MacOS/ebook-convert` → `/usr/local/bin/…`、`/opt/homebrew/bin/…`、`/usr/bin/…` → `PATH` 里的 `ebook-convert`。每个候选跑 `--version`(10s 超时)取版本串。
  - `calibre::convert_to_htmlz(calibre: &str, input: &Path, out_htmlz: &Path) -> Result<(), String>`(600s 超时,失败带 stderr 摘要)
- 测试注入点:`detect_with_candidates(paths: &[PathBuf], timeout)` 纯逻辑函数,`detect` 是壳。

- [ ] **Step 1: 失败测试**:fixture ok 脚本被探中且版本串含 "calibre 7.0";hang 脚本超时(用 2s 短超时参数)返回 None;不存在路径跳过。convert:ok 脚本产出文件 → Ok;`false` 脚本 → Err。
- [ ] **Step 2: 跑测失败** → **Step 3: 实现**(`std::process::Command` + 手写超时:spawn + 轮询 `try_wait` 100ms 步进,超时 kill)→ **Step 4: 跑测过**
- [ ] **Step 5: Commit**:`git add plugins-src/ebook-import/backend/src/calibre.rs plugins-src/ebook-import/backend/tests && git commit -m "feat(ebook-import): calibre detection + htmlz conversion"`

---

### Task 5: 后端——HTMLZ 解包 / 元数据 / HTML→MD / Calibre 标记清理

**Files:**
- Create: `backend/src/htmlz.rs`(含 tests;fixture 用代码内联字符串构造 zip,不放二进制文件)

**Interfaces:**
- Consumes: `bookconf::BookMeta`
- Produces:
  - `htmlz::extract(htmlz: &Path, work: &Path) -> Result<Extracted, String>`;`Extracted { html: PathBuf, images_dir: Option<PathBuf>, meta: BookMeta }`(解 zip 到 `work/htmlz/`;找 `index.html`,退化任意 `.html`;找名为 images/image/pics/pictures 的目录;`metadata.opf` 无则 meta 全 None)
  - `htmlz::html_to_markdown(html: &str) -> String`(htmd 转换 + `clean_calibre_markers`)
  - `htmlz::clean_calibre_markers(md: &str) -> String`(pub 供单测)

- [ ] **Step 1: 失败测试**:

```rust
#[test]
fn cleans_calibre_markers() {
    let md = "Title{.calibre1}\n[x](#calibre_link-12)\n::: div\n42\nfoo\nbar .ct}\n\n\n\nend";
    let out = clean_calibre_markers(md);
    assert!(!out.contains("{.calibre"));
    assert!(!out.contains("#calibre_link"));
    assert!(!out.contains(":::"));
    assert!(!out.lines().any(|l| l.trim() == "42"));   // 纯数字行删
    assert!(!out.contains(".ct}"));
    assert!(out.contains("foo"));
    assert!(!out.contains("\n\n\n"));                   // ≥3 空行折叠
}

#[test]
fn extract_reads_opf_metadata() {
    // 代码里用 zip::ZipWriter 现造一个含 index.html + images/a.png + metadata.opf 的 htmlz
    // opf 内容含 <dc:title>七力</dc:title><dc:creator>H</dc:creator><dc:language>zh</dc:language>
    // 断言 Extracted.meta.title == Some("七力"), images_dir 命中, html 以 index.html 结尾
}
```

- [ ] **Step 2: 跑测失败** → **Step 3: 实现**。清理顺序照 python:regex 去 `\{\.calibre[^}]*\}`、`\(#calibre_link-\d+\)`;逐行删 `:::` 开头/纯数字/`.ct}`/`.cn}` 结尾行;`\u{feff}` 删、`\u{a0}`→空格;`\n{3,}`→`\n\n`。opf 解析用 quick-xml 事件流,只认 local name `title/creator/publisher/language` 的首个文本。htmd 用默认 builder,skip `script/style`。
- [ ] **Step 4: 跑测过** → **Step 5: Commit**:`git commit -m "feat(ebook-import): htmlz extraction + metadata + html-to-markdown"`

---

### Task 6: 后端——OcrEngine 抽象 + 微信OCR(pdfium 渲染)

**Files:**
- Create: `backend/src/ocr/mod.rs`、`backend/src/ocr/wechat.rs`、`backend/src/ocr/pdfium.rs`

**Interfaces:**
- Produces(mod.rs):

```rust
pub enum OcrProgress { Page { done: usize, total: usize }, Status(String) }

pub trait OcrEngine {
    fn ocr_pdf(&self, pdf: &Path, work: &Path, on: &mut dyn FnMut(OcrProgress))
        -> Result<String, String>;   // Ok(merged markdown)
}

/// PDF→逐页 PNG。生产实现 PdfiumRenderer;测试用 Fake。
pub trait PageRenderer {
    fn render_pages(&self, pdf: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, String>;
}
```

  - `wechat::WeChatOcr { url: String, renderer: Box<dyn PageRenderer>, timeout: Duration }` 实现 `OcrEngine`
  - `pdfium::PdfiumRenderer::new() -> Result<Self, String>`(dylib 探测:exe 同目录 `libpdfium.dylib` → env `NOTEMD_PDFIUM_PATH`;2x 缩放渲染 `page_%04d.png`)
- Consumes: 无(独立模块)

- [ ] **Step 1: 失败测试**(`wechat.rs` tests;mock server 用 std::net::TcpListener 手写:accept 后读到 `\r\n\r\n` 即回 `HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: …\r\n\r\n{"success":true,"content":"# p"}`,循环 N 次;FakeRenderer 往 out_dir 写 3 个 1x1 PNG):
  - 3 页全成功 → 返回 md 为 3 段 `# p` 以 `\n\n` 连接;progress 收到 Page{1..3,3}
  - `work/page0002.md` 预置内容 "cached" → 只发 2 次请求,合并结果含 "cached"(断点续跑)
  - server 回 `{"success":false}` → 该页记失败,其余页照走,末尾 Ok 但结果只含成功页;全部页失败 → Err
- [ ] **Step 2: 跑测失败** → **Step 3: 实现**。逐页:已存在 `pageNNNN.md` 跳过;否则 reqwest blocking multipart(`file` 部件,mime `image/png`)POST,期待 `{"success":true,"content":…}`;页结果写 `work/pageNNNN.md`;全失败(0 页成功)才 Err。合并按文件名排序、`\n\n` 分隔(移植 01_ocr_to_md.py)。`PdfiumRenderer` 单独文件,**不进单测**(需 dylib;实机验证覆盖):`Pdfium::bind_to_library(探测路径)`,`PdfRenderConfig::new().scale_page_by_factor(2.0)`,每页 `as_image()` 存 PNG。
- [ ] **Step 4: 跑测过**(`cargo test --manifest-path … ocr`)→ **Step 5: Commit**:`git commit -m "feat(ebook-import): OcrEngine abstraction + WeChat OCR provider"`

---

### Task 7: 后端——百度 Unlimited-OCR provider

**Files:**
- Create: `backend/src/ocr/baidu.rs`

**Interfaces:**
- Produces: `baidu::BaiduOcr { api_key, secret_key, oauth_url, submit_url, query_url, poll_interval: Duration }` 实现 `OcrEngine`;`new(api_key, secret_key)` 用生产 URL:
  - oauth `https://aip.baidubce.com/oauth/2.0/token`
  - submit `https://aip.baidubce.com/rest/2.0/brain/online/v2/unlimited-ocr-parser/task`
  - query 同前缀 `…/task/query`
- `baidu::localize_images(md: &str, fetch: &dyn Fn(&str) -> Result<Vec<u8>, String>, images_dir: &Path) -> Result<String, String>`(pub 供单测):把 md 里 `![…](http…)` 逐个下载存 `images/baidu_NNN.<ext>`、改写为 `images/baidu_NNN.<ext>`

- [ ] **Step 1: 失败测试**:
  - `localize_images`:输入含两个远端图链的 md + fake fetch(返回 PNG magic bytes)→ 输出链接改写为 `images/baidu_001.png`/`baidu_002.png`,文件落盘;fetch 失败的链保留原样并继续
  - 整流程(mock server,单线程 TcpListener 按序应答):①oauth → `{"access_token":"T","expires_in":2592000}` ②submit → `{"error_code":0,"result":{"task_id":"t1"}}` ③query 第一次 → `{"error_code":0,"result":{"status":"running"}}` ④query 第二次 → `{"…":0,"result":{"status":"success","markdown_url":"http://127.0.0.1:PORT/md"}}` ⑤GET /md → `# book`。`poll_interval=10ms`。断言返回 "# book",progress 收到 Status("pending/running…")
  - 前置校验:>100MB 文件、submit 返回 `error_code!=0` → Err 带 error_msg
- [ ] **Step 2: 跑测失败** → **Step 3: 实现**。PDF 页数校验(≤500)在 pipeline 层跳过(pdfium 才能数页,而百度路径可能没 dylib——只做体积校验,页数超限交给百度报错)。file_data=base64(整文件),file_name 带原后缀。`ocr_pdf` 里最后调 `localize_images`(fetch = reqwest GET),images_dir = `work/images/`(pipeline 会把它拷进落盘目录)。
- [ ] **Step 4: 跑测过** → **Step 5: Commit**:`git commit -m "feat(ebook-import): Baidu Unlimited-OCR provider with image localization"`

---

### Task 8: 后端——流水线 + job 编排 + 命令分发 + CLI

**Files:**
- Create: `backend/src/pipeline.rs`
- Modify: `backend/src/plugin.rs`(替换骨架)

**Interfaces:**
- Consumes: Task 3–7 全部 Produces
- Produces(UI 与 CLI 依赖):
  - on_ui_request 方法表:
    - `detect_env` `{}` → `{ calibre: {path,version}|null, vault: {root}|null, settings: VaultSettings, device: { calibre_path, baidu_api_key_set: bool, baidu_secret_key_set: bool } }`(**密钥本体不回传 UI**,只回是否已设;保存时空串=不改、`"-"`=清除)
    - `save_settings` `{ vault?: VaultSettings, device?: { calibre_path?, baidu_api_key?, baidu_secret_key? } }` → `{ok:true}`
    - `import_start` `{ path, ocr: bool, provider?: string }` → `{ job_id: number }`
    - `import_cancel` `{ job_id }` → `{ok:true}`
  - `host.ui.post` 推送(window_id 固定 `"main"`):`{ type:"job", job_id, event:"log", line }` / `{…, event:"progress", stage, page?, total? }` / `{…, event:"done", dest_rel }` / `{…, event:"failed", error }`
  - execute_command:`import` = CLI 入口,同步跑完(单文件),Ok 返回 `{ dest, log: [String] }`,Err 非零退出由宿主处理

- [ ] **Step 1: 失败测试**(pipeline 纯逻辑部分):

```rust
#[test]
fn dest_dir_collision_appends_suffix() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("2026-08/Seven Powers")).unwrap();
    std::fs::create_dir_all(tmp.path().join("2026-08/Seven Powers (2)")).unwrap();
    let d = unique_dest(&tmp.path().join("2026-08"), "Seven Powers");
    assert!(d.ends_with("Seven Powers (3)"));
}

#[test]
fn month_dir_is_dash_format() {
    assert_eq!(month_dir(chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap()), "2026-08");
}

#[test]
fn finalize_copies_config_book_and_images() { /* work 里造 input.md/config.txt/images/,finalize 到 dest,断言三者齐 + book.md 内容==input.md */ }
```

- [ ] **Step 2: 跑测失败** → **Step 3: 实现 pipeline.rs**。核心函数(全部纯参数注入,便于测):

```rust
pub struct PipelineCtx<'a> {
    pub vault_root: &'a Path, pub ebooks_root: &'a str,
    pub work: &'a Path,                      // <data_dir>/work/<stem>_temp
    pub log: &'a mut dyn FnMut(String),      // 逐条日志(UI push + CLI 收集共用)
    pub progress: &'a mut dyn FnMut(&str, Option<(usize, usize)>),
    pub cancelled: &'a std::sync::atomic::AtomicBool,
}
pub fn run_import(ctx: &mut PipelineCtx, input: &Path, ocr: bool,
                  engine: Option<Box<dyn OcrEngine>>, calibre: Option<&str>)
    -> Result<PathBuf, String>               // Ok(dest 绝对路径)
```

流程:扩展名校验(epub/pdf/docx;ocr 时仅 pdf)→ ocr 分支调 engine.ocr_pdf(meta 只有 title=文件名 stem,method="ocr")/普通分支 calibre::convert_to_htmlz + htmlz::extract + html_to_markdown(method="calibre_htmlz")→ bookconf::write_config_txt → 目录名 = sanitize(title).非空 else sanitize(stem) → `unique_dest(<vault>/<root>/<YYYY-MM>, name)` → finalize 拷贝(config.txt、input.md→book.md、images/ 整目录;OCR-百度的 `work/images/` 同样拷)。每步之间查 `cancelled`,取消即 Err("cancelled")。
- [ ] **Step 4: 实现 plugin.rs**。状态:`Arc<Mutex<Inner{ vault, vault_checked, jobs: HashMap<u64, Arc<AtomicBool>>, next_job }>>`。vault 解析**照抄 claude-agent 模式**:activate 时 spawn `host.request("host.vault.info")` 重试 + shared-config `sotvault` 兜底(把 `vault_from_host`/`shared_config_vault` 两个函数从 claude-agent/backend/src/plugin.rs 复制过来,注释注明出处)。`import_start`:分配 job_id,`std::thread::spawn` 里建 engine(wechat→`WeChatOcr{url: settings, renderer: PdfiumRenderer::new()?}`;baidu→`BaiduOcr::new(device keys)`,缺 key 即 failed 事件),`log` 闭包 = `host.ui_post("main", …)` + `host.log_info`。CLI `import`:cli_str/cli_flag 辅助函数从 claude-agent 复制,同线程直跑 `run_import`,log 收集进 Vec 随结果返回。
- [ ] **Step 5: 跑全后端测试**:`cargo test --manifest-path plugins-src/ebook-import/backend/Cargo.toml` 全绿
- [ ] **Step 6: Commit**:`git add plugins-src/ebook-import/backend/src && git commit -m "feat(ebook-import): pipeline, job orchestration, ui commands, cli"`

---

### Task 9: 前端窗口(队列 UI + 拖放 + 设置)

**Files:**
- Create: `src/lib/queue.ts`、`src/lib/queue.test.ts`、`src/lib/strings.ts`
- Modify: `src/App.svelte`(替换占位)

**Interfaces:**
- Consumes: bridge(`request`/`onMessage`)、Task 8 的方法表与推送形状、Task 1 的 drag-drop 推送
- Produces: `queue.ts`:

```ts
export type ItemStatus = "pending" | "running" | "done" | "failed";
export interface QueueItem { id: number; path: string; name: string; status: ItemStatus;
  stage?: string; page?: number; total?: number; destRel?: string; error?: string;
  jobId?: number; logs: string[]; }
export interface Queue { items: QueueItem[]; activeId: number | null; }
export function addPaths(q: Queue, paths: string[]): Queue;            // 去重(同路径 pending/running 不重复加)、仅收 epub/pdf/docx
export function nextToStart(q: Queue): QueueItem | null;               // activeId==null 时首个 pending
export function onJobEvent(q: Queue, jobId: number, ev: JobEvent): Queue; // log/progress/done/failed 归位
```

- [ ] **Step 1: queue.test.ts 失败测试**:addPaths 过滤非法后缀+去重;串行调度(nextToStart 在有 active 时返回 null;done/failed 后 activeId 清空);onJobEvent 把 log 追加、done 写 destRel、failed 写 error。
- [ ] **Step 2: 跑测失败**:`pnpm --filter ebook-import-plugin test` → **Step 3: 实现 queue.ts** → **Step 4: 跑测过**
- [ ] **Step 5: strings.ts**。照 openclaw 结构(MessageKey 联合 + en/zh/ja/de catalog + `t()`),键至少:`title, drop.hint, drop.pick, ocr.label, ocr.onlyPdf, ocr.provider.wechat, ocr.provider.baidu, settings.toggle, settings.root, settings.wechatUrl, settings.baiduKey, settings.baiduSecret, settings.calibre.found, settings.calibre.missing, settings.calibre.pick, settings.calibre.install, settings.save, queue.empty, status.pending, status.running, status.done, status.failed, action.openInEditor, action.cancel, action.clear, log.toggle`。zh 文案:OCR 复选框 =「OCR(扫描版 PDF)」、provider =「微信OCR」/「百度 Unlimited-OCR」、菜单同 manifest。
- [ ] **Step 6: App.svelte**。结构:
  - onMount:`request("plugin.detect_env")` 填 calibre 状态/设置;`window.notemd.onMessage`:`type=="drag-drop"` → phase enter/leave 切 highlight class,drop → `addPaths`;`type=="job"` → `onJobEvent` 并在 done/failed 后触发调度
  - 调度函数:`const n = nextToStart(q); if (n) { const { job_id } = await request("plugin.import_start", { path: n.path, ocr, provider }); …标记 running }`
  - 拖放区 + 「添加文件…」(`request("host.dialog.open", { multiple: true, filters: [{ name: "Ebooks", extensions: ["epub","pdf","docx"] }] })`)
  - OCR checkbox(默认 false)→ 勾选显示 provider `<select>`;设置区(可折叠):root 输入、微信 URL、百度 Key/Secret(`type="password"`,已设时 placeholder「已设置」)、Calibre 状态行 + 手动选择(dialog.open 单选)+ 保存按钮 → `plugin.save_settings`
  - 队列行:状态徽标(`status.*`)、OCR 进度 `page/total`、done 行 destRel + 「在编辑器打开」→ `request("host.editor.open", { path: destRel + "/book.md" })`、failed 行 error;展开箭头切 logs `<pre>`
  - 样式贴主程序简洁风(系统字体、`color-scheme: light dark`)
- [ ] **Step 7: 构建/检查**:`pnpm --filter ebook-import-plugin build && pnpm --filter ebook-import-plugin check && pnpm --filter ebook-import-plugin test` 全绿
- [ ] **Step 8: Commit**:`git add plugins-src/ebook-import/src plugins-src/ebook-import/index.html && git commit -m "feat(ebook-import): queue window with drag-drop, ocr options, settings"`

---

### Task 10: 构建脚本(pdfium 拉取 + dev-install + release)

**Files:**
- Create: `scripts/fetch-pdfium.sh`
- Modify: `scripts/dev-install-plugin.sh`、`scripts/release-plugins.sh`、`.gitignore`(加 `plugins-src/ebook-import/backend/vendor/`)

- [ ] **Step 1: fetch-pdfium.sh**:从 https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-mac-{arm64,x64}.tgz 下载解出 `lib/libpdfium.dylib`,缓存到 `plugins-src/ebook-import/backend/vendor/{aarch64,x86_64}-apple-darwin/libpdfium.dylib`(已存在即跳过;`--force` 重取)。
- [ ] **Step 2: dev-install-plugin.sh 加分支**(照 openclaw 形状;usage 两处列表加 `ebook-import`):

```bash
elif [[ "$PLUGIN" == "ebook-import" ]]; then
  SRC="plugins-src/ebook-import"
  bash scripts/fetch-pdfium.sh
  cargo build $([ "$PROFILE" = release ] && echo --release) \
    --manifest-path "$SRC/backend/Cargo.toml" --bin notemd-ebook-import
  pnpm --filter ebook-import-plugin build
  VERSION=$(node -e "console.log(require('./$SRC/manifest.v2.json').version)")
  DEST="$ROOT/notemd.ebook-import/$VERSION"
  rm -rf "$DEST"; mkdir -p "$DEST/bin" "$DEST/ui"
  cp "$SRC/backend/target/$PROFILE/notemd-ebook-import" "$DEST/bin/"
  ARCH_TRIPLE="$(uname -m | sed 's/arm64/aarch64/')-apple-darwin"
  cp "$SRC/backend/vendor/$ARCH_TRIPLE/libpdfium.dylib" "$DEST/bin/"
  cp -R "$SRC/dist/." "$DEST/ui/"
  cp "$SRC/manifest.v2.json" "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.ebook-import/current"
  mark_installed "notemd.ebook-import" "$VERSION"
  echo "✓ installed notemd.ebook-import@$VERSION → $DEST"
```

- [ ] **Step 3: release-plugins.sh**:usage/case 列表加 `ebook-import`;`release_native_ui` 增加可选第 5 参 `extra_bin_dir_by_triple`(打包时若非空,把 `vendor/$triple/libpdfium.dylib` 一并拷进 stage `bin/` 且 codesign dylib),新增:

```bash
release_ebook_import() {
  bash "$REPO_ROOT/scripts/fetch-pdfium.sh"
  release_native_ui "notemd.ebook-import" "$REPO_ROOT/plugins-src/ebook-import" \
    "notemd-ebook-import" "ebook-import-plugin" "vendor"
}
```

case 分发加 `ebook-import) release_ebook_import ;;`。
- [ ] **Step 4: 验证**:`bash -n scripts/*.sh` 语法过;`scripts/dev-install-plugin.sh ebook-import` 完整跑通(产出安装目录含 bin/notemd-ebook-import + libpdfium.dylib + ui/)
- [ ] **Step 5: Commit**:`git add scripts/fetch-pdfium.sh scripts/dev-install-plugin.sh scripts/release-plugins.sh .gitignore && git commit -m "build(ebook-import): pdfium fetch + dev-install + release packaging"`

---

### Task 11: 收尾——全量回归 + dev 实机验证移交

- [ ] **Step 1: 全量测试**:`cd src-tauri && cargo test`(核心)+ `cargo test --manifest-path plugins-src/ebook-import/backend/Cargo.toml` + `pnpm test`(根)+ `pnpm --filter ebook-import-plugin test && pnpm check`,全绿
- [ ] **Step 2: dev-install-plugin.sh 尾部加 E2E 注释块**(照 claude-agent 块格式):安装 → `pnpm tauri dev` → 插件菜单「导入电子书…」开窗 → 拖 epub 进窗/添加文件 → 队列跑完查 `<vault>/ssot/ebooks/YYYY-MM/<书名>/` 三件套 → 勾 OCR 选微信/百度各试一本扫描 PDF → CLI `notemd ebook <file>` 验证
- [ ] **Step 3: Commit + push**,然后**停**:GUI/窗口改动(Task 1 核心 + 插件窗口)按约定必须先 dev 实机验证——由用户执行,提供上面的手动步骤清单;验证过后才走主程序发版(携带 Task 1,版本号即 engines 门槛)与插件上架(release-plugins + gen-plugin-index merge 式发布)。

## Self-Review 记录

- Spec 覆盖:§2 manifest→Task 2;§3a→Task 4/5;§3b/§4→Task 6/7;§3c→Task 3/8;§5→Task 4(探测)+8(detect_env)+9(UI);§6→Task 3(两层)+8(密钥不回传);§7→Task 8/9;§8→Task 1;§9→Task 2(manifest cli)+8(execute_command);§10 测试分布各任务;§11→Task 10;§12 非目标未引入 ✓
- 类型一致:`WeChatOcr{url,renderer,timeout}`、`OcrProgress`、queue.ts 形状、job 事件形状在 Task 6/7/8/9 间逐一核对一致 ✓
- 无占位符:各步含实际代码/命令/断言 ✓
