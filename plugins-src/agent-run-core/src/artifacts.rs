//! What did this run actually deliver? The window turns the answer into links
//! that open in an editor tab, so it must be vault-RELATIVE paths and it must
//! be strict: a file the run merely READ or mentioned is not a result, and
//! offering it as one makes the list useless.
//!
//! So: markdown WRITTEN during this run, under the two places a task delivers
//! to — the task's own `output/`, and the vault's `answers/` — plus the ONE
//! file the caller declared this run's target (`RunSpec::deliverable`, e.g. an
//! ebook digest written beside the book), which lives wherever its caller says
//! and would otherwise be invisible here. Sidecar notes are excluded; they're
//! the note itself, already one click away in the panel.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Cap what a runaway task can push into the window.
pub const MAX: usize = 20;

/// Where a task is allowed to leave a deliverable, relative to the vault.
pub const VAULT_OUTPUT_DIR: &str = "answers";

/// This run's markdown deliverables, sorted and deduped. `deliverable` is the
/// caller-declared target file (absolute); it counts only if it is inside the
/// vault and was written by THIS run, same mtime gate as everything else.
pub fn collect(
    vault: &Path,
    task_dir: &Path,
    since: SystemTime,
    deliverable: Option<&Path>,
) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for root in [task_dir.join("output"), vault.join(VAULT_OUTPUT_DIR)] {
        for p in written_markdown(&root, since) {
            if let Some(rel) = vault_relative(vault, &p) {
                out.insert(rel);
            }
        }
    }
    if let Some(d) = deliverable.filter(|d| is_deliverable(d) && is_fresh(d, since)) {
        if let Some(rel) = vault_relative(vault, d) {
            out.insert(rel);
        }
    }
    out.into_iter().take(MAX).collect()
}

/// `*.md` under `root` touched at or after `since`. The mtime gate is what
/// keeps a previous run's leftovers from being presented as this one's.
fn written_markdown(root: &Path, since: SystemTime) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, 0, &mut |p| {
        if is_deliverable(p) && is_fresh(p, since) {
            found.push(p.to_path_buf());
        }
    });
    found
}

/// Touched at or after `since` — what keeps a previous run's leftovers out.
fn is_fresh(p: &Path, since: SystemTime) -> bool {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .map(|m| m >= since)
        .unwrap_or(false)
}

fn walk(dir: &Path, depth: usize, f: &mut impl FnMut(&Path)) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, depth + 1, f);
        } else {
            f(&p);
        }
    }
}

fn is_deliverable(p: &Path) -> bool {
    let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase());
    let Some(name) = name else { return false };
    // A sidecar note is where the answers live, not a deliverable to link.
    name.ends_with(".md") && !name.ends_with(".note.md") && !name.ends_with(".notes.md")
}

/// Strip the vault prefix, canonicalizing both sides so a symlinked vault
/// (or `/tmp` → `/private/tmp` on macOS) still matches. Returns None for a
/// path outside the vault — which is also how the engine decides whether a
/// declared deliverable is ours to stamp.
pub fn vault_relative(vault: &Path, path: &Path) -> Option<String> {
    let root = vault.canonicalize().ok()?;
    let abs = path.canonicalize().ok()?;
    let rel = abs.strip_prefix(&root).ok()?;
    let s = rel.to_string_lossy().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn touch(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn just_before() -> SystemTime {
        SystemTime::now() - Duration::from_secs(5)
    }

    #[test]
    fn picks_up_markdown_the_run_wrote_to_output() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        touch(&task.join("output/selfcheck.md"), "# hi");
        touch(&task.join("output/nested/more.md"), "# hi");
        assert_eq!(
            collect(v.path(), &task, just_before(), None),
            vec![
                ".notemd/agent-tasks/t/output/nested/more.md",
                ".notemd/agent-tasks/t/output/selfcheck.md",
            ]
        );
    }

    #[test]
    fn picks_up_a_long_answer_written_into_the_vault() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        touch(&v.path().join("answers/2026-07-31-kv-cache.md"), "# a");
        assert_eq!(
            collect(v.path(), &task, just_before(), None),
            vec!["answers/2026-07-31-kv-cache.md"]
        );
    }

    #[test]
    fn ignores_files_the_run_only_read_or_mentioned() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        // Written during the run, but nowhere a task delivers to: the source
        // document, and a note elsewhere in the vault.
        touch(&v.path().join("docs/source.md"), "# read me");
        touch(&v.path().join("inbox/scratch.md"), "# touched");
        assert!(collect(v.path(), &task, just_before(), None).is_empty());
    }

    #[test]
    fn ignores_the_sidecar_note_itself() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        touch(&v.path().join("answers/a.note.md"), "- x");
        touch(&task.join("output/b.notes.md"), "- x");
        assert!(collect(v.path(), &task, just_before(), None).is_empty());
    }

    #[test]
    fn ignores_output_left_over_from_an_earlier_run() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        touch(&task.join("output/old.md"), "# old");
        touch(&v.path().join("answers/old.md"), "# old");
        // This run started after those were written.
        let since = SystemTime::now() + Duration::from_secs(5);
        assert!(collect(v.path(), &task, since, None).is_empty());
    }

    #[test]
    fn ignores_non_markdown_output() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        touch(&task.join("output/data.json"), "{}");
        touch(&task.join("output/notes.txt"), "x");
        assert!(collect(v.path(), &task, just_before(), None).is_empty());
    }

    #[test]
    fn is_empty_when_the_run_delivered_nothing() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        assert!(collect(v.path(), &task, just_before(), None).is_empty());
    }

    /// The declared target lives outside `output/` and `answers/` (an ebook
    /// digest sits beside its book), so without this it would never show up as
    /// something the window can open.
    #[test]
    fn picks_up_the_declared_deliverable_wherever_it_lives() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        let summary = v.path().join("ssot/ebooks/2026-08/深度工作/2026-08-04-summary.md");
        touch(&summary, "# 深度工作 — 摘要");
        assert_eq!(
            collect(v.path(), &task, just_before(), Some(&summary)),
            vec!["ssot/ebooks/2026-08/深度工作/2026-08-04-summary.md"]
        );
    }

    #[test]
    fn a_deliverable_that_is_missing_stale_or_outside_the_vault_is_not_reported() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");

        // Never written by this run.
        let missing = v.path().join("ssot/gone-summary.md");
        assert!(collect(v.path(), &task, just_before(), Some(&missing)).is_empty());

        // There, but left over from an earlier run.
        let stale = v.path().join("ssot/old-summary.md");
        touch(&stale, "# old");
        let since = SystemTime::now() + Duration::from_secs(5);
        assert!(collect(v.path(), &task, since, Some(&stale)).is_empty());

        // Fresh, but not in this vault.
        let elsewhere = tempfile::tempdir().unwrap();
        let outside = elsewhere.path().join("summary.md");
        touch(&outside, "# x");
        assert!(collect(v.path(), &task, just_before(), Some(&outside)).is_empty());
    }
}
