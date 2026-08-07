//! Two-stage import: prepare_import extracts a zip to a tempdir and returns
//! a report; install_prepared copies the report's staged files into the
//! user themes dir.

use crate::themes::appearance::{resolve_appearance, title_case_from_stem, Appearance};
use crate::themes::compiler::compile_theme_css;
use crate::themes::header::parse_header;
use crate::themes::id::is_valid_theme_id;
use crate::themes::zip_safety::{extract_zip_safely, ExtractError, ExtractLimits};
use serde::{Serialize, Deserialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImportTheme {
    pub id: String,
    pub name: String,
    pub appearance: Appearance,
    pub source_file: String,    // basename inside the staging dir
    pub conflict: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImportError {
    pub file: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImportReport {
    pub themes: Vec<ImportTheme>,
    pub asset_dirs: Vec<String>,
    pub errors: Vec<ImportError>,
    pub staging_dir: PathBuf,
}

pub fn prepare_import(zip_path: &Path, existing_ids: &[String]) -> Result<ImportReport, String> {
    let staging = tempfile::tempdir().map_err(|e| e.to_string())?;
    let staging_path = staging.path().to_path_buf();
    let limits = ExtractLimits::default();
    extract_zip_safely(zip_path, &staging_path, limits).map_err(|e: ExtractError| e.to_string())?;
    let _ = staging.keep(); // detach lifetime; cleanup is explicit

    let mut themes: Vec<ImportTheme> = Vec::new();
    let mut errors: Vec<ImportError> = Vec::new();
    let mut asset_dirs: Vec<String> = Vec::new();

    let entries = std::fs::read_dir(&staging_path).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() { continue }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let Some(ext) = path.extension().and_then(|s| s.to_str()) else { continue };
        if ext.to_ascii_lowercase() != "css" { continue }
        if is_valid_theme_id(stem).is_err() {
            errors.push(ImportError {
                file: format!("{stem}.css"),
                message: "invalid theme id (must match [a-z0-9][a-z0-9._-]*)".into(),
            });
            continue;
        }
        let css = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => { errors.push(ImportError { file: format!("{stem}.css"), message: e.to_string() }); continue }
        };
        if let Err(e) = compile_theme_css(&css, stem, &staging_path.join(stem).to_string_lossy()) {
            errors.push(ImportError { file: format!("{stem}.css"), message: e });
            continue;
        }
        let header = parse_header(&css);
        let name = header.name.unwrap_or_else(|| title_case_from_stem(stem));
        let appearance = resolve_appearance(header.appearance.as_deref(), stem);
        themes.push(ImportTheme {
            id: stem.to_string(),
            name,
            appearance,
            source_file: format!("{stem}.css"),
            conflict: existing_ids.iter().any(|e| e == stem),
        });
    }

    themes.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let theme_id_set: std::collections::HashSet<&str> = themes.iter().map(|t| t.id.as_str()).collect();
    let entries = std::fs::read_dir(&staging_path).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() { continue }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else { continue };
        if theme_id_set.contains(name) {
            asset_dirs.push(name.to_string());
        }
    }
    asset_dirs.sort();

    Ok(ImportReport { themes, asset_dirs, errors, staging_dir: staging_path })
}

/// Copy staged files into `themes_dir`, then compile each. Returns the
/// number of themes installed. The staging dir is removed regardless of
/// outcome.
pub fn install_prepared(
    report: &ImportReport,
    themes_dir: &Path,
    overwrite: bool,
) -> Result<usize, String> {
    std::fs::create_dir_all(themes_dir).map_err(|e| e.to_string())?;
    let compiled_dir = themes_dir.join(".compiled");
    std::fs::create_dir_all(&compiled_dir).map_err(|e| e.to_string())?;
    let mut installed = 0usize;
    for theme in &report.themes {
        let dst = themes_dir.join(&theme.source_file);
        if dst.exists() && !overwrite { continue }
        let src = report.staging_dir.join(&theme.source_file);
        std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
        let asset_src = report.staging_dir.join(&theme.id);
        if asset_src.exists() && asset_src.is_dir() {
            copy_dir_all(&asset_src, &themes_dir.join(&theme.id)).map_err(|e| e.to_string())?;
        }
        let css = std::fs::read_to_string(&dst).map_err(|e| e.to_string())?;
        let assets = themes_dir.join(&theme.id);
        let out = compile_theme_css(&css, &theme.id, assets.to_str().unwrap_or(""))?;
        std::fs::write(compiled_dir.join(&theme.source_file), out).map_err(|e| e.to_string())?;
        installed += 1;
    }
    cleanup_staging(&report.staging_dir);
    Ok(installed)
}

pub fn cleanup_staging(staging: &Path) {
    let _ = std::fs::remove_dir_all(staging);
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}
