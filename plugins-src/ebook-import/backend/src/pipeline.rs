//! The end-to-end import pipeline: validate the input file, produce
//! markdown (via Calibre+HTMLZ or OCR), write `config.txt`, and land the
//! result in the vault under `<ebooks_root>/<YYYY-MM>/<Title>/`.
//!
//! Every dependency (Calibre binary path, OCR engine, log/progress sinks,
//! the cancel flag) is injected through [`PipelineCtx`]/parameters rather
//! than looked up globally, so [`run_import`] is fully testable without a
//! real Calibre install or network access -- the only paths exercised by
//! `cargo test` are the ones that fail (or are cancelled) before touching
//! either.

use crate::bookconf::{self, BookMeta};
use crate::calibre;
use crate::htmlz;
use crate::ocr::{OcrEngine, OcrProgress};
use crate::settings::validate_ebooks_root;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Extensions [`run_import`] accepts as input, case-insensitively. OCR
/// narrows this further to `pdf` only (checked separately).
const ACCEPTED_EXTENSIONS: &[&str] = &["epub", "pdf", "docx"];

/// Everything one [`run_import`] call needs beyond the input file itself.
/// `log`/`progress` are trait objects (not generics) so the same
/// `PipelineCtx` shape serves both the UI job path (closures that call
/// `host.ui_post`) and the CLI path (closures that push into a `Vec`) --
/// see plugin.rs.
pub struct PipelineCtx<'a> {
    pub vault_root: &'a Path,
    pub ebooks_root: &'a str,
    /// Scratch directory for this run, e.g. `<data_dir>/work/<stem>_temp`.
    /// Reused across retries on purpose: an OCR engine resumes from
    /// whatever `pageNNNN.md` files a prior interrupted run already wrote
    /// here (see `ocr/wechat.rs`).
    pub work: &'a Path,
    /// One log line at a time; shared by the UI push path and the CLI's
    /// collected-log-lines path.
    pub log: &'a mut dyn FnMut(String),
    /// `(stage, Some((done, total)))` for page-granular progress (OCR),
    /// `(stage, None)` for a stage with no sub-progress to report.
    pub progress: &'a mut dyn FnMut(&str, Option<(usize, usize)>),
    /// Polled between every stage; a cancelled run stops with `Err("cancelled")`
    /// rather than continuing to burn time (or money, for the Baidu path) on
    /// a result nobody wants anymore.
    pub cancelled: &'a AtomicBool,
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("cancelled".to_string())
    } else {
        Ok(())
    }
}

fn lowercase_extension(input: &Path) -> String {
    input
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default()
}

/// Runs the whole pipeline for one book: validates the input, converts it
/// to markdown (Calibre+HTMLZ, or OCR when `ocr` is true), writes
/// `config.txt`, and copies the result into the vault. Returns the
/// absolute path of the created destination directory.
pub fn run_import(
    ctx: &mut PipelineCtx,
    input: &Path,
    ocr: bool,
    engine: Option<Box<dyn OcrEngine>>,
    calibre_bin: Option<&str>,
) -> Result<PathBuf, String> {
    // Defense in depth (Finding 4): `apply_vault_patch` already rejects a
    // bad `ebooks_root` at save time, but `ctx.ebooks_root` here is whatever
    // `.notemd/ebook-import.json` currently holds on disk -- reachable by a
    // hand edit, an external agent, or a file written before this guard
    // existed. An absolute path or a `..` component would otherwise escape
    // the vault once joined onto `ctx.vault_root` below.
    validate_ebooks_root(ctx.ebooks_root)?;

    std::fs::create_dir_all(ctx.work)
        .map_err(|e| format!("create work dir {}: {e}", ctx.work.display()))?;
    check_cancelled(ctx.cancelled)?;

    let ext = lowercase_extension(input);
    if ocr && ext != "pdf" {
        return Err(format!("OCR only supports PDF input, got .{ext}"));
    }
    if !ACCEPTED_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!(
            "unsupported file extension '.{ext}' (expected one of: {})",
            ACCEPTED_EXTENSIONS.join(", ")
        ));
    }
    check_cancelled(ctx.cancelled)?;

    let (meta, method) = if ocr {
        let engine = engine.ok_or_else(|| "no OCR engine available".to_string())?;
        (ctx.log)(format!("OCR: {}", input.display()));
        (ctx.progress)("ocr", None);

        let stem = input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("book")
            .to_string();

        let markdown = {
            // Disjoint reborrows of `ctx.log`/`ctx.progress` -- both live only
            // for this call, so ctx's fields are free again right after.
            let log = &mut *ctx.log;
            let progress = &mut *ctx.progress;
            let mut adapter = move |p: OcrProgress| match p {
                OcrProgress::Page { done, total } => progress("ocr", Some((done, total))),
                OcrProgress::Status(s) => log(s),
            };
            engine.ocr_pdf(input, ctx.work, &mut adapter)?
        };
        check_cancelled(ctx.cancelled)?;

        std::fs::write(ctx.work.join("input.md"), markdown)
            .map_err(|e| format!("write input.md: {e}"))?;

        (
            BookMeta {
                title: Some(stem),
                ..Default::default()
            },
            "ocr",
        )
    } else {
        let calibre_bin = calibre_bin.ok_or_else(|| "calibre not found".to_string())?;
        (ctx.log)(format!("converting {} to htmlz", input.display()));
        (ctx.progress)("convert", None);
        let htmlz_path = ctx.work.join("book.htmlz");
        calibre::convert_to_htmlz(calibre_bin, input, &htmlz_path)?;
        check_cancelled(ctx.cancelled)?;

        (ctx.progress)("extract", None);
        let extracted = htmlz::extract(&htmlz_path, ctx.work)?;
        if let Some(images_dir) = &extracted.images_dir {
            copy_dir_recursive(images_dir, &ctx.work.join("images"))?;
        }
        check_cancelled(ctx.cancelled)?;

        (ctx.progress)("markdown", None);
        let html = std::fs::read_to_string(&extracted.html)
            .map_err(|e| format!("read {}: {e}", extracted.html.display()))?;
        let markdown = htmlz::html_to_markdown(&html)?;
        std::fs::write(ctx.work.join("input.md"), markdown)
            .map_err(|e| format!("write input.md: {e}"))?;

        (extracted.meta, "calibre_htmlz")
    };
    check_cancelled(ctx.cancelled)?;

    bookconf::write_config_txt(
        &ctx.work.join("config.txt"),
        &input.to_string_lossy(),
        method,
        &meta,
    )
    .map_err(|e| format!("write config.txt: {e}"))?;
    check_cancelled(ctx.cancelled)?;

    let stem_fallback = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let dirname = {
        let from_title = meta
            .title
            .as_deref()
            .map(bookconf::sanitize_dirname)
            .unwrap_or_default();
        if !from_title.is_empty() {
            from_title
        } else {
            bookconf::sanitize_dirname(stem_fallback)
        }
    };
    if dirname.is_empty() {
        return Err("could not derive a directory name for this book".to_string());
    }

    let month = month_dir(chrono::Local::now().date_naive());
    let month_parent = ctx.vault_root.join(ctx.ebooks_root).join(month);
    std::fs::create_dir_all(&month_parent)
        .map_err(|e| format!("create {}: {e}", month_parent.display()))?;
    let dest = unique_dest(&month_parent, &dirname);
    check_cancelled(ctx.cancelled)?;

    (ctx.progress)("finalize", None);
    finalize(ctx.work, &dest)?;

    Ok(dest)
}

/// `<parent>/<name>`, or the first `<parent>/<name> (N)` (N = 2, 3, ...)
/// that doesn't already exist -- so re-importing a book with the same
/// title never clobbers a previous import.
pub fn unique_dest(parent: &Path, name: &str) -> PathBuf {
    let base = parent.join(name);
    if !base.exists() {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = parent.join(format!("{name} ({n})"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// The vault's per-month bucket name for a given date, e.g. `2026-08`.
pub fn month_dir(d: chrono::NaiveDate) -> String {
    d.format("%Y-%m").to_string()
}

/// Copies the finished work dir's outputs into `dest`: `config.txt` as-is,
/// `input.md` renamed to `book.md` (the vault-facing name), and `images/`
/// (if the run produced one -- Calibre HTMLZ extraction and Baidu's
/// remote-image localization both write to `work/images/`) recursively.
pub fn finalize(work: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("create {}: {e}", dest.display()))?;

    let config_src = work.join("config.txt");
    if config_src.exists() {
        std::fs::copy(&config_src, dest.join("config.txt"))
            .map_err(|e| format!("copy config.txt: {e}"))?;
    }

    let input_md = work.join("input.md");
    std::fs::copy(&input_md, dest.join("book.md")).map_err(|e| {
        format!(
            "copy {} -> {}: {e}",
            input_md.display(),
            dest.join("book.md").display()
        )
    })?;

    let images_src = work.join("images");
    if images_src.exists() {
        copy_dir_recursive(&images_src, &dest.join("images"))?;
    }

    Ok(())
}

/// Recursively copies `src`'s contents into `dst` (creating `dst` and any
/// nested directories as needed). Plain `fs::copy`/`fs::create_dir_all`
/// walk, not a rename -- a rename would fail across filesystems/mounts,
/// and `work`/the vault destination aren't guaranteed to share one.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("mkdir {}: {e}", dst.display()))?;
    for entry in
        std::fs::read_dir(src).map_err(|e| format!("read dir {}: {e}", src.display()))?
    {
        let entry = entry.map_err(|e| format!("read dir entry in {}: {e}", src.display()))?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)
                .map_err(|e| format!("copy {} -> {}: {e}", path.display(), target.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub [`OcrEngine`] that returns fixed markdown without touching a
    /// real pdfium renderer or network -- exercises `run_import`'s full OCR
    /// success path (previously untested end-to-end; only the pre-engine
    /// rejection paths had coverage).
    struct StubOcrEngine {
        markdown: &'static str,
    }

    impl OcrEngine for StubOcrEngine {
        fn ocr_pdf(
            &self,
            _pdf: &Path,
            _work: &Path,
            on: &mut dyn FnMut(OcrProgress),
        ) -> Result<String, String> {
            on(OcrProgress::Page { done: 1, total: 1 });
            Ok(self.markdown.to_string())
        }
    }

    #[test]
    fn run_import_ocr_success_path_lands_in_vault_with_config_and_book_md() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = tmp.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        let input = tmp.path().join("My Book.pdf");
        std::fs::write(&input, b"%PDF-1.4 fake").unwrap();

        let mut log_lines: Vec<String> = Vec::new();
        let mut log = |line: String| log_lines.push(line);
        let mut progress_calls: Vec<(String, Option<(usize, usize)>)> = Vec::new();
        let mut progress =
            |stage: &str, pt: Option<(usize, usize)>| progress_calls.push((stage.to_string(), pt));
        let cancelled = AtomicBool::new(false);
        let work = tmp.path().join("work");

        let mut ctx = PipelineCtx {
            vault_root: &vault,
            ebooks_root: "ssot/ebooks",
            work: &work,
            log: &mut log,
            progress: &mut progress,
            cancelled: &cancelled,
        };

        let engine: Box<dyn OcrEngine> = Box::new(StubOcrEngine {
            markdown: "# Stub Content",
        });
        let dest = run_import(&mut ctx, &input, true, Some(engine), None)
            .expect("a stubbed OCR run must succeed");

        let month = month_dir(chrono::Local::now().date_naive());
        assert_eq!(
            dest,
            vault.join("ssot/ebooks").join(&month).join("My Book"),
            "dest must be <vault>/<ebooks_root>/<YYYY-MM>/<Title>"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("book.md")).unwrap(),
            "# Stub Content"
        );
        let cfg = std::fs::read_to_string(dest.join("config.txt")).unwrap();
        assert!(cfg.contains("conversion_method=ocr"), "got: {cfg}");
        assert!(cfg.contains("original_title=My Book"), "got: {cfg}");
        assert!(
            progress_calls.iter().any(|(stage, _)| stage == "finalize"),
            "expected a finalize progress stage, got {progress_calls:?}"
        );
    }

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
        assert_eq!(
            month_dir(chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap()),
            "2026-08"
        );
    }

    #[test]
    fn finalize_copies_config_book_and_images() {
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        std::fs::create_dir_all(work.join("images")).unwrap();
        std::fs::write(work.join("config.txt"), "input_file=x\n").unwrap();
        std::fs::write(work.join("input.md"), "# Hello\n").unwrap();
        std::fs::write(work.join("images/pic.png"), [1, 2, 3]).unwrap();

        let dest = tmp.path().join("dest/Some Book");
        finalize(&work, &dest).unwrap();

        assert!(dest.join("config.txt").exists());
        assert!(dest.join("book.md").exists());
        assert!(dest.join("images/pic.png").exists());
        assert_eq!(
            std::fs::read_to_string(dest.join("book.md")).unwrap(),
            std::fs::read_to_string(work.join("input.md")).unwrap()
        );
    }

    #[test]
    fn run_import_rejects_an_ebooks_root_that_could_escape_the_vault() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = tmp.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        let input = tmp.path().join("book.epub");
        std::fs::write(&input, "not really an epub").unwrap();

        let mut log = |_: String| {};
        let mut progress = |_: &str, _: Option<(usize, usize)>| {};
        let cancelled = AtomicBool::new(false);
        let mut ctx = PipelineCtx {
            vault_root: &vault,
            ebooks_root: "../escape",
            work: &tmp.path().join("work"),
            log: &mut log,
            progress: &mut progress,
            cancelled: &cancelled,
        };
        let err = run_import(&mut ctx, &input, false, None, None).unwrap_err();
        assert!(err.contains("ebooks_root"), "got: {err}");
    }

    #[test]
    fn rejects_unsupported_extensions_before_touching_calibre_or_ocr() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("book.txt");
        std::fs::write(&input, "not a book").unwrap();

        let mut log = |_: String| {};
        let mut progress = |_: &str, _: Option<(usize, usize)>| {};
        let cancelled = AtomicBool::new(false);
        let mut ctx = PipelineCtx {
            vault_root: tmp.path(),
            ebooks_root: "ssot/ebooks",
            work: &tmp.path().join("work"),
            log: &mut log,
            progress: &mut progress,
            cancelled: &cancelled,
        };
        let err = run_import(&mut ctx, &input, false, None, None).unwrap_err();
        assert!(err.contains("unsupported file extension"), "got: {err}");
    }

    #[test]
    fn ocr_rejects_non_pdf_input() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("book.epub");
        std::fs::write(&input, "not really an epub").unwrap();

        let mut log = |_: String| {};
        let mut progress = |_: &str, _: Option<(usize, usize)>| {};
        let cancelled = AtomicBool::new(false);
        let mut ctx = PipelineCtx {
            vault_root: tmp.path(),
            ebooks_root: "ssot/ebooks",
            work: &tmp.path().join("work"),
            log: &mut log,
            progress: &mut progress,
            cancelled: &cancelled,
        };
        let err = run_import(&mut ctx, &input, true, None, None).unwrap_err();
        assert!(err.contains("OCR only supports PDF"), "got: {err}");
    }

    #[test]
    fn a_cancelled_flag_short_circuits_before_any_work() {
        let tmp = tempfile::tempdir().unwrap();
        // A valid-looking input that would otherwise proceed past the
        // extension check -- proving cancellation is checked before that,
        // not relying on the extension check to fail first.
        let input = tmp.path().join("book.epub");
        std::fs::write(&input, "not really an epub").unwrap();

        let mut log = |_: String| {};
        let mut progress = |_: &str, _: Option<(usize, usize)>| {};
        let cancelled = AtomicBool::new(true);
        let mut ctx = PipelineCtx {
            vault_root: tmp.path(),
            ebooks_root: "ssot/ebooks",
            work: &tmp.path().join("work"),
            log: &mut log,
            progress: &mut progress,
            cancelled: &cancelled,
        };
        let err = run_import(&mut ctx, &input, false, None, None).unwrap_err();
        assert_eq!(err, "cancelled");
    }
}
