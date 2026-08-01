//! md2pdf — one-shot CLI plugin that takes a host-rendered inline HTML body
//! plus an output path, and writes a typographically-clean A4 PDF.
//!
//! Protocol mirrors mdshare and the rest of note.md's plugins:
//!  - read one line of JSON Request from stdin
//!  - perform the work
//!  - emit one line of JSON Response on stdout
//!  - exit
//!
//! Task 10 lands the skeleton with a stub run_export that writes the wrapped
//! HTML to disk so the smoke test verifies stdin/stdout plumbing. Task 11
//! replaces the stub with the real WKWebView + PDFKit pipeline.

mod ipc;
mod template;

#[cfg(target_os = "macos")]
mod pdf;
#[cfg(not(target_os = "macos"))]
mod pdf {
    pub fn render_to_path(_html: &str, _path: &str) -> Result<(), String> {
        Err("md2pdf is macOS-only".into())
    }
}

use std::io::{self, Read, Write};
use ipc::{Request, Response};

const PLUGIN_NAME: &str = "md2pdf";

fn main() {
    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        emit(Response::fail(vec![ipc::toast_error(
            format!("❌ {PLUGIN_NAME}: 无法读取 stdin"),
            Some(e.to_string()),
        )]));
        return;
    }
    let req: Request = match serde_json::from_str(input.trim()) {
        Ok(r) => r,
        Err(e) => {
            emit(Response::fail(vec![ipc::toast_error(
                format!("❌ {PLUGIN_NAME}: 请求 JSON 解析失败"),
                Some(e.to_string()),
            )]));
            return;
        }
    };

    let resp = match req.command.as_str() {
        "export" => run_export(&req),
        other => Response::fail(vec![ipc::toast_error(
            format!("❌ {PLUGIN_NAME}: 未知命令"),
            Some(other.to_string()),
        )]),
    };
    emit(resp);
}

fn run_export(req: &Request) -> Response {
    let html = template::wrap_html(&req.context.rendered_html, &req.context.tab.title);
    match pdf::render_to_path(&html, &req.context.output_path) {
        Ok(()) => {
            let _ = std::process::Command::new("open")
                .args(["-R", &req.context.output_path])
                .spawn();
            let mut data = serde_json::Map::new();
            data.insert("path".to_string(), serde_json::Value::String(req.context.output_path.clone()));
            Response::ok(vec![
                ipc::toast_success(format!("✅ 已导出到 {}", req.context.output_path)),
                ipc::Action::CliResult { data },
            ])
        }
        Err(e) => Response::fail(vec![ipc::toast_error(
            format!("❌ {PLUGIN_NAME}: 渲染失败"),
            Some(e),
        )]),
    }
}

fn emit(resp: Response) {
    let s = serde_json::to_string(&resp).expect("serialize response");
    let stdout = io::stdout();
    let mut h = stdout.lock();
    h.write_all(s.as_bytes()).expect("write stdout");
    h.write_all(b"\n").expect("write newline");
}
