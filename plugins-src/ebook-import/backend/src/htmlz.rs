//! Unpacks Calibre-produced HTMLZ archives (a zip containing an HTML body,
//! an optional images directory, and an OPF metadata sidecar), recovers
//! book metadata from that sidecar, and converts the HTML to Markdown with
//! Calibre's own markup debris stripped out. Ports `01_convert_to_htmlz.py`
//! verbatim -- both the file-discovery heuristics (the python script used
//! `glob`, so it never assumed a fixed HTMLZ layout) and the cleaning-rule
//! order -- so this Rust port produces byte-comparable output.

#[cfg(test)]
mod tests;

use crate::bookconf::BookMeta;
use quick_xml::events::Event;
use quick_xml::Reader;
use regex::Regex;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Result of unpacking one HTMLZ archive: where its HTML body landed on
/// disk, its images directory (if the archive shipped one), and whatever
/// metadata could be recovered from `metadata.opf`.
pub struct Extracted {
    pub html: PathBuf,
    pub images_dir: Option<PathBuf>,
    pub meta: BookMeta,
}

/// Directory names (case-insensitive) that HTMLZ producers use for their
/// image assets. Calibre itself emits `images/`; the others are kept for
/// HTMLZ files hand-rolled or produced by other tools that the original
/// python pipeline also had to tolerate.
const IMAGE_DIR_NAMES: &[&str] = &["images", "image", "pics", "pictures"];

/// Unzips `htmlz` into `work/htmlz/`, then locates the HTML entry point,
/// image directory, and OPF metadata by walking the extracted tree by file
/// name rather than assuming a fixed structure.
pub fn extract(htmlz: &Path, work: &Path) -> Result<Extracted, String> {
    let dest = work.join("htmlz");
    fs::create_dir_all(&dest).map_err(|e| format!("create {}: {e}", dest.display()))?;

    let file = fs::File::open(htmlz).map_err(|e| format!("open {}: {e}", htmlz.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("read htmlz zip: {e}"))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("read htmlz zip entry {i}: {e}"))?;

        // `enclosed_name()` is `zip`'s own guard against zip-slip: it
        // returns `None` for absolute paths or entries whose `..`
        // components would escape the extraction root. Such entries are
        // simply skipped rather than trusted.
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(&enclosed);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .map_err(|e| format!("mkdir {}: {e}", out_path.display()))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        let mut out_file = fs::File::create(&out_path)
            .map_err(|e| format!("create {}: {e}", out_path.display()))?;
        io::copy(&mut entry, &mut out_file)
            .map_err(|e| format!("write {}: {e}", out_path.display()))?;
    }

    let html =
        find_html(&dest).ok_or_else(|| format!("no .html/.htm file found in {}", htmlz.display()))?;
    let images_dir = find_images_dir(&dest);
    let meta = find_opf(&dest)
        .and_then(|opf| fs::read_to_string(opf).ok())
        .map(|xml| parse_opf(&xml))
        .unwrap_or_default();

    Ok(Extracted {
        html,
        images_dir,
        meta,
    })
}

/// Recursively collects every regular file under `root`.
fn walk_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Recursively collects every directory under `root` (root itself excluded).
fn walk_dirs(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.push(path.clone());
            walk_dirs(&path, out);
        }
    }
}

fn eq_ignore_case(name: Option<&std::ffi::OsStr>, candidate: &str) -> bool {
    name.and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case(candidate))
}

/// Finds the HTMLZ's HTML body: prefers a file literally named
/// `index.html` (case-insensitive, matching Calibre's own convention);
/// falls back to the first `*.html`/`*.htm` found anywhere in the archive
/// for HTMLZ files that don't follow that convention.
fn find_html(root: &Path) -> Option<PathBuf> {
    let mut files = Vec::new();
    walk_files(root, &mut files);

    if let Some(p) = files
        .iter()
        .find(|p| eq_ignore_case(p.file_name(), "index.html"))
    {
        return Some(p.clone());
    }

    files.into_iter().find(|p| {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"))
    })
}

fn find_images_dir(root: &Path) -> Option<PathBuf> {
    let mut dirs = Vec::new();
    walk_dirs(root, &mut dirs);
    dirs.into_iter().find(|d| {
        IMAGE_DIR_NAMES
            .iter()
            .any(|candidate| eq_ignore_case(d.file_name(), candidate))
    })
}

fn find_opf(root: &Path) -> Option<PathBuf> {
    let mut files = Vec::new();
    walk_files(root, &mut files);
    files
        .into_iter()
        .find(|p| eq_ignore_case(p.file_name(), "metadata.opf"))
}

/// The four `BookMeta` fields an OPF's `dc:` elements can populate.
#[derive(Clone, Copy)]
enum Field {
    Title,
    Creator,
    Publisher,
    Language,
}

fn field_for(local_name: &[u8]) -> Option<Field> {
    match local_name {
        b"title" => Some(Field::Title),
        b"creator" => Some(Field::Creator),
        b"publisher" => Some(Field::Publisher),
        b"language" => Some(Field::Language),
        _ => None,
    }
}

/// Parses `metadata.opf` with a quick-xml event stream, reading only the
/// first text found under each `dc:title`/`dc:creator`/`dc:publisher`/
/// `dc:language` element (matched on *local* name, so any namespace prefix
/// or default namespace works). An unparsable or absent OPF simply yields
/// an all-`None` `BookMeta` -- metadata recovery is always best-effort.
fn parse_opf(xml: &str) -> BookMeta {
    let mut meta = BookMeta::default();
    let mut reader = Reader::from_str(xml);
    let mut current: Option<Field> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(e)) => {
                current = field_for(e.local_name().as_ref());
            }
            Ok(Event::End(_)) => {
                current = None;
            }
            Ok(Event::Text(t)) => {
                let Some(field) = current else { continue };
                let Ok(text) = t.unescape() else { continue };
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                let slot = match field {
                    Field::Title => &mut meta.title,
                    Field::Creator => &mut meta.creator,
                    Field::Publisher => &mut meta.publisher,
                    Field::Language => &mut meta.language,
                };
                // Only the *first* text under each field name is kept, so
                // a repeated element (e.g. multiple `dc:creator`s) doesn't
                // clobber the first author with a later one.
                if slot.is_none() {
                    *slot = Some(text.to_string());
                }
            }
            _ => {}
        }
    }

    meta
}

/// Converts HTML to Markdown via `htmd` (skipping `<script>`/`<style>`
/// bodies, which never belong in reading content), then strips Calibre's
/// HTMLZ markup debris via [`clean_calibre_markers`].
pub fn html_to_markdown(html: &str) -> Result<String, String> {
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style"])
        .build();
    let md = converter.convert(html).map_err(|e| e.to_string())?;
    Ok(clean_calibre_markers(&md))
}

/// Strips Calibre's HTMLZ markup debris out of converted markdown, in the
/// exact order the original `01_convert_to_htmlz.py` script applied them so
/// output stays byte-comparable across the python -> rust port:
///
/// 1. `{.calibreN}` pandoc-style class annotations
/// 2. `(#calibre_link-N)` internal anchor targets
/// 3. whole lines that are pure structural noise: `:::` fence markers,
///    digit-only page-number lines, and lines ending in `.ct}`/`.cn}`
///    (Calibre caption-class remnants)
/// 4. stray BOM (`\u{feff}`) removal and NBSP (`\u{a0}`) -> space
/// 5. collapsing runs of 3+ newlines (left behind by the drops above) down
///    to a single blank line
pub fn clean_calibre_markers(md: &str) -> String {
    static CALIBRE_CLASS: OnceLock<Regex> = OnceLock::new();
    static CALIBRE_LINK: OnceLock<Regex> = OnceLock::new();
    static DIGITS_ONLY: OnceLock<Regex> = OnceLock::new();
    static BLANK_RUN: OnceLock<Regex> = OnceLock::new();

    let calibre_class = CALIBRE_CLASS.get_or_init(|| Regex::new(r"\{\.calibre[^}]*\}").unwrap());
    let calibre_link = CALIBRE_LINK.get_or_init(|| Regex::new(r"\(#calibre_link-\d+\)").unwrap());
    let digits_only = DIGITS_ONLY.get_or_init(|| Regex::new(r"^\s*\d+\s*$").unwrap());
    let blank_run = BLANK_RUN.get_or_init(|| Regex::new(r"\n{3,}").unwrap());

    let step1 = calibre_class.replace_all(md, "");
    let step2 = calibre_link.replace_all(&step1, "");

    let step3 = step2
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with(":::")
                || digits_only.is_match(line)
                || trimmed.ends_with(".ct}")
                || trimmed.ends_with(".cn}"))
        })
        .collect::<Vec<_>>()
        .join("\n");

    let step4: String = step3
        .chars()
        .filter(|&c| c != '\u{feff}')
        .map(|c| if c == '\u{a0}' { ' ' } else { c })
        .collect();

    blank_run.replace_all(&step4, "\n\n").into_owned()
}
