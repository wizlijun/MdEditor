# 电子书导入插件(notemd.ebook-import)设计

> 2026-08-01。将 `~/git/claude_translater/bookread.sh` 的电子书归档流程完全用插件语言(Rust 后端 + Svelte 窗口)重写为 note.md v2 插件。ExLibris(独立 app + v2 插件)已于同日整体删除(commit 6a97d84),本插件是 vault 内电子书导入的唯一入口。

## 0. 一句话

把 epub/pdf/docx 转成 markdown,连同元数据与插图,归档进 vault 的 `ssot/ebooks/YYYY-MM/<书名>/`;扫描版 PDF 走 OCR(微信OCR 或 百度 Unlimited-OCR);窗口拖放/多选逐个处理,成功失败都有结果与详情日志;CLI 同能力。

## 1. 已拍板的决策

| 决策点 | 结论 |
|---|---|
| OCR 默认服务 | 微信OCR(内网),默认 URL `http://10.17.0.123:8092/ocr` |
| 第二个内置 OCR | 百度智能云「文档解析(Unlimited-OCR)」(https://cloud.baidu.com/doc/OCR/s/fmr1p39gb) |
| 转换引擎 | 保持调用 Calibre `ebook-convert`(外部依赖,自动扫描+界面显示+手动指定);HTML→MD 用 Rust 库(htmd)替代 pandoc;OCR 页渲染内置 pdfium |
| 落盘产物 | `config.txt + book.md + images/`,与 bookread.sh / translatebook.sh 流水线逐字兼容 |
| 拖放 | 做,宿主通用转发 drag-drop 事件给插件窗口(一次核心改动,所有插件受益) |
| 目录月份格式 | `YYYY-MM`(用户指定;注意 bookread.sh 原为 `YYYY.MM`,不沿用) |

## 2. 形态与 manifest

**后端 + 前端**形态(样板:openclaw / claude-agent)。

- `plugins-src/ebook-import/`:Svelte/Vite 窗口工程;`backend/`:独立 Cargo crate → 二进制 `notemd-ebook-import`(aarch64 + x86_64)
- id `notemd.ebook-import`;name "Ebook Import";i18n:zh「导入电子书」、ja「電子書籍を取り込む」、de "E-Books importieren"
- `engines.notemd`:≥ 携带拖放转发的主程序版本(见 §8)
- `activation`:`["onCommand:open", "onCli:ebook"]`
- menus:`location:"plugins"`,label "Import Ebooks (epub/pdf/docx)…",zh「导入电子书(epub、pdf、docx)…」,`command:"open"`
- windows:`main`,entry `index.html`,`open_command:"open"`,900×640(min 640×480),singleton
- capabilities:`["ui", "toast", "dialog", "vault.read", "vault.write", "editor.open"]`
- `request_timeout_seconds: 300`(仅影响桥上的短请求;长任务不走同步请求,见 §6)

## 3. 转换流水线(后端 Rust,bookread.sh 全过程移植)

每个文件一个 job,后端工作目录:`<插件数据目录>/work/<文件名 stem>_temp/`(可断点续跑,完成后保留供排查,新 job 同名时先清)。

### 3a. 普通路径(epub / pdf / docx)— 移植 01_convert_to_htmlz.py

1. **Calibre 转 HTMLZ**:`ebook-convert <input> <tmp>.htmlz`(超时 600s)。Calibre 路径解析见 §5。
2. **解包 + 元数据**:zip crate 解 HTMLZ;找 `index.html`(退化:任意 .html)与 `images/` 类目录;`metadata.opf` 用 quick-xml 抽 `dc:title / dc:creator / dc:publisher / dc:language`。
3. **HTML→Markdown**:htmd(不换行,等价 pandoc `--wrap=none`);随后逐条移植清理规则:
   - 去 `{.calibre…}`、`(#calibre_link-N)`
   - 删 `:::` 开头行、纯数字行、`.ct}`/`.cn}` 结尾行
   - 去 BOM(﻿)、nbsp( )→ 空格,≥3 连续空行折叠为 2
4. 产出 `input.md` + `images/`(相对链接保持 `images/…`)。

### 3b. OCR 路径(仅 PDF;checkbox 勾选)— 移植 01_ocr_to_md.py

入口校验:非 PDF 报错;交给所选 OcrEngine(§4)得到合并 markdown → `input.md`。

### 3c. 落盘 — 移植 bookread.sh 主体

1. `config.txt` 生成(格式逐字兼容):`input_file / input_lang=auto / output_lang=zh / conversion_method=calibre_htmlz|ocr` + `original_title / creator / publisher / source_language`(有则写)。
2. 目录名 = `original_title` sanitize(移植 shell 规则:`/\:*?"<>|` → `_`、去控制字符、空白折叠、去首尾空格与点、截 200 字符;空则退回输入文件名 stem)。
3. 目标:`<vault>/<ebooks_root>/<YYYY-MM>/<目录名>/`;**已存在则追加 ` (2)`、` (3)` 后缀,不覆盖**。
4. 写 `config.txt`、`book.md`(即 input.md)、`images/`(有则整目录拷)。后端为原生进程,直接写文件系统(vault 根经 UI 传入或 CLI 上下文获得);host.vault.write 的 10MB/文本限制不适用于本路径。
5. 不做 pageNNNN 分片 —— 那是翻译流水线临时目录的事,不进 vault(YAGNI)。

## 4. OcrEngine 抽象

粒度定在**整份文档**(百度是异步整档解析,逐页抽象容纳不下):

```rust
pub trait OcrEngine {
    /// 整份 PDF → 合并 markdown;进度经回调上报;work 目录用于断点缓存
    fn ocr_pdf(&self, pdf: &Path, work: &Path, on: &mut dyn FnMut(OcrProgress)) -> Result<String>;
}
```

### ① WeChatOcr(id `wechat`,默认)— 微信OCR

- pdfium(插件包内置各架构 dylib,~8MB)逐页 2x 渲染 PNG → 逐页 multipart POST `file=<page>.png` → 期待 `{"success": true, "content": "<markdown>"}`,120s/页超时
- 页结果落 `work/pageNNNN.md`,已存在跳过(断点续跑);失败页记录页码,末尾汇总告警但不中断整书
- 进度:`第 x/y 页`;默认 URL `http://10.17.0.123:8092/ocr`(设置可改)

### ② BaiduUnlimitedOcr(id `baidu`)— 百度文档解析(Unlimited-OCR)

- 鉴权:API Key + Secret Key → `POST https://aip.baidubce.com/oauth/2.0/token` 换 access_token,内存缓存至过期
- 提交:`POST …/rest/2.0/brain/online/v2/unlimited-ocr-parser/task`(form-urlencoded,`file_data`=整份 PDF base64,`file_name` 带后缀);**无需本地渲染**
- 轮询:`…/task/query`,间隔 7s(文档建议 5–10s;QPS 提交 2 / 查询 5),状态 pending/running/success/failed
- 成功:下载 `markdown_url` 内容;**图片本地化**——markdown 内引用的远端图片(百度链接 30 天过期)逐个下载进 `images/`、改写为相对链接(file-over-app:归档必须自包含)
- 前置校验照文档限制:PDF ≤100MB、≤500 页,超限直接报错不提交

未来新 OCR 服务 = 新 `OcrEngine` 实现 + 设置里新增 provider 项,流水线零改动。

## 5. Calibre 路径:扫描 + 界面显示 + 手动指定

探测顺序(`detect_env` 返回命中项与版本号,UI 展示):

1. 设备本地配置的手动覆盖(§6 device.json)
2. SharedConfig 的 `calibre_path`(`~/Library/Application Support/com.laobu.mdeditor-shared/config.json`,现成字段)
3. `/Applications/calibre.app/Contents/MacOS/ebook-convert`
4. `/usr/local/bin/ebook-convert`、`/opt/homebrew/bin/ebook-convert`、`/usr/bin/ebook-convert`、PATH

每个候选跑 `--version`(10s 超时)验证。UI 设置区显示:`✓ Calibre @ <路径>(<版本>)` 或 `✗ 未找到 — [安装引导 calibre-ebook.com] [手动选择…]`;手动选择走 host.dialog.open,存设备本地配置。未找到时普通路径导入置灰(OCR-百度路径不依赖 Calibre,仍可用;OCR-微信路径也不依赖)。

## 6. 设置与密钥(两层,按语义分家)

**vault 内** `.notemd/ebook-import.json`(git 同步、agent 可读、跨设备):

```json
{ "ebooks_root": "ssot/ebooks",
  "ocr": { "provider": "wechat", "wechat": { "url": "http://10.17.0.123:8092/ocr" } } }
```

**设备本地** `<插件数据目录>/device.json`(不进 vault):

```json
{ "calibre_path": "/Applications/calibre.app/Contents/MacOS/ebook-convert",
  "baidu": { "api_key": "…", "secret_key": "…" } }
```

百度密钥与 Calibre 路径都是设备/账号相关,**绝不进 git 同步的 vault**。UI 经后端命令读写两份配置(后端直读文件系统;vault 侧文件也可被外部 agent 直接编辑,写入端保持宽容解析)。缺省值在代码内,文件缺失即用默认。

## 7. 窗口 UX 与后端协议

### 布局

- 顶部:拖放区(真拖放,§8)+「添加文件…」按钮(host.dialog.open,multiple,filters epub/pdf/docx)
- 选项行:**OCR checkbox(默认不勾选)**,勾选后展开 provider 下拉(微信OCR / 百度 Unlimited-OCR)
- 设置区(可折叠):电子书根路径(默认 `ssot/ebooks`)、微信OCR URL、百度 API Key/Secret、Calibre 状态行(§5)
- 队列:每文件一行 = 文件名 + 状态徽标(等待 / 转换中·阶段 / OCR x/y 页 / ✓ 成功 / ✗ 失败)+ 展开箭头
  - 成功:显示 vault 相对落盘路径 + 「在编辑器打开」(host.editor.open 打开 book.md)
  - 失败:错误摘要一行
  - 展开 = **详情日志**(后端全过程日志逐条留存,成功失败都可查)
- **逐个处理**:队列串行,前一个完成(成功或失败)才起下一个;可取消当前/清空队列

### 后端命令(UI 经 `plugin.` 前缀调用,均快速返回)

| 命令 | 参数 → 返回 | 说明 |
|---|---|---|
| `detect_env` | — → `{ calibre: {path, version}?, settings, device }` | 启动时调,Calibre 扫描 + 两份配置 |
| `save_settings` | `{ vault?, device? }` → `{ok}` | 写回对应层 |
| `import_start` | `{ path, ocr: bool, provider?, vault_root, ebooks_root }` → `{ job_id }` | 立即返回,后台线程跑流水线 |
| `import_cancel` | `{ job_id }` → `{ok}` | 取消当前 job |

进度/日志/结果**不走同步请求**(转换分钟级):后端经 `host.ui.post` 推送 `{ type: "job", job_id, event: "log"|"progress"|"done"|"failed", … }`,UI `onMessage` 收。

## 8. 核心改动(唯一一处):拖放转发给插件窗口

现状:Tauri OS 级 drag-drop 处理器吃掉插件窗口内 HTML5 拖放,且插件窗口零 Tauri IPC 收不到 `tauri://drag-drop`(exlibris 当年因此退回按钮选文件)。

改法:`src-tauri/src/plugin_runtime/windows.rs` 的 `open_plugin_window` 在现有 `on_window_event` 里增接 `WindowEvent::DragDrop`,把 Enter/Over 的悬停态与 Drop 的**文件路径列表**经现成的 `push_to_window` → `window.__notemd_dispatch` 推给插件 UI:

```json
{ "type": "drag-drop", "phase": "enter"|"leave"|"drop", "paths": ["/abs/a.epub"] }
```

通用机制,所有插件受益;不加新 capability(路径只是字符串,读文件仍归后端/授权路径管)。本插件 `engines.notemd` 以携带此改动的版本为门槛。随下一版主程序发布。

## 9. CLI

```
notemd ebook <file> [--ocr] [--ocr-provider wechat|baidu] [--root <vault相对路径>]
```

- `contributes.cli`:subcommand `ebook`,command `import`;`onCli:ebook` 激活,同一后端同一流水线
- 参数缺省读 §6 配置;日志打 stdout,失败非零退出;多文件 = 多次调用(宿主 CliEntry 单文件位置参数)
- 已知限制:宿主 CLI 桥 300s 上限(claude-agent 教训)。普通转换通常够;大部头 OCR 建议走 GUI。首版不做 detach,超时给明确提示

## 10. 测试

- 后端单测:sanitize_dirname(移植原 shell 用例)、calibre 标记清理、metadata.opf 解析、config.txt 生成/读回、落盘冲突后缀、OCR 断点跳页、百度轮询状态机与图片本地化改写(mock HTTP:微信/百度/OAuth 各一)
- Calibre 探测:fixture 假 ebook-convert 脚本(成功/挂起/崩溃)
- UI:队列状态机 vitest(串行调度、取消、日志累积)
- manifest:`cargo test -p plugin-protocol` 校验通过
- windows.rs 拖放转发:payload 形状单测(纯函数);实机 GUI 验证由用户执行(惯例)

## 11. 构建与发布

- `scripts/dev-install-plugin.sh` 加 `ebook-import` 分支(backend cargo + pnpm filter build,照 openclaw 形状;pdfium dylib 随 bin/ 拷贝)
- `scripts/release-plugins.sh` 复用 `release_native_ui` 加 case(注意打包时带 pdfium dylib)
- 市场上架照既有流程(merge 式 index,勿 clobber)

## 12. 非目标(首版明确不做)

- 翻译(只归档源文;翻译仍归 translatebook.sh 流水线)
- pageNNNN 分片入 vault、mobi/azw3 等其他格式、CLI detach 长任务、OCR 图片版面还原、自动生成 .note.md(手记按意图保存原则)
