//! Which markdown files did this run produce? The window turns them into links
//! that open in an editor tab, so the answer has to be vault-RELATIVE paths
//! (`host.editor.open` rejects anything else) and it has to be honest: a stale
//! file from last week's run is worse than no link at all.
//!
//! Two sources, because tasks split into two kinds:
//!  - files under the task's `output/` written during this run (selfcheck)
//!  - `.md` paths named in the final answer (annotation-sweep writes straight
//!    into `answers/` and never touches `output/`)
use std::collections::BTreeSet;
use std::path::Path;
use std::time::SystemTime;

/// Cap what a runaway task can push into the window.
pub const MAX: usize = 20;

/// Collect this run's markdown, newest-first-ish (sorted, deduped).
pub fn collect(vault: &Path, task_dir: &Path, result_text: &str, since: SystemTime) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for p in output_markdown(task_dir, since) {
        if let Some(rel) = vault_relative(vault, &p) {
            out.insert(rel);
        }
    }
    for rel in mentioned_markdown(vault, result_text) {
        out.insert(rel);
    }
    out.into_iter().take(MAX).collect()
}

/// `<task_dir>/output/**/*.md` touched at or after `since`. The mtime filter is
/// what keeps a previous run's leftovers from being presented as this one's.
fn output_markdown(task_dir: &Path, since: SystemTime) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    walk(&task_dir.join("output"), 0, &mut |p| {
        if !is_markdown(p) {
            return;
        }
        let fresh = std::fs::metadata(p)
            .and_then(|m| m.modified())
            .map(|m| m >= since)
            .unwrap_or(false);
        if fresh {
            found.push(p.to_path_buf());
        }
    });
    found
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

/// `.md` paths named in the answer text — absolute, vault-relative, or
/// wiki-style — kept only when the file actually exists inside the vault.
fn mentioned_markdown(vault: &Path, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '<' | '>' | '"' | '\'' | '`' | ',' | ';')) {
        let t = token.trim_end_matches(['.', ':', '!', '?', '。', ',']);
        if !t.to_ascii_lowercase().ends_with(".md") {
            continue;
        }
        let candidate = Path::new(t);
        let abs = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            vault.join(candidate)
        };
        if abs.is_file() {
            if let Some(rel) = vault_relative(vault, &abs) {
                out.push(rel);
            }
        }
    }
    out
}

fn is_markdown(p: &Path) -> bool {
    p.extension()
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

/// Strip the vault prefix, canonicalizing both sides so a symlinked vault
/// (or `/tmp` → `/private/tmp` on macOS) still matches. Anything outside the
/// vault is dropped: `host.editor.open` would only reject it later.
fn vault_relative(vault: &Path, path: &Path) -> Option<String> {
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

    #[test]
    fn picks_up_markdown_the_run_wrote_to_output() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        let since = SystemTime::now() - Duration::from_secs(5);
        touch(&task.join("output/selfcheck.md"), "# hi");
        touch(&task.join("output/nested/more.md"), "# hi");
        let got = collect(v.path(), &task, "", since);
        assert_eq!(
            got,
            vec![
                ".notemd/agent-tasks/t/output/nested/more.md",
                ".notemd/agent-tasks/t/output/selfcheck.md",
            ]
        );
    }

    #[test]
    fn ignores_output_left_over_from_an_earlier_run() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        touch(&task.join("output/old.md"), "# old");
        // This run started after that file was written.
        let since = SystemTime::now() + Duration::from_secs(5);
        assert!(collect(v.path(), &task, "", since).is_empty());
    }

    #[test]
    fn ignores_non_markdown_output() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        touch(&task.join("output/data.json"), "{}");
        touch(&task.join("output/notes.txt"), "x");
        let since = SystemTime::now() - Duration::from_secs(5);
        assert!(collect(v.path(), &task, "", since).is_empty());
    }

    #[test]
    fn picks_up_markdown_named_in_the_answer() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        touch(&v.path().join("answers/2026-07-30-kv-cache.md"), "# a");
        let since = SystemTime::now();
        let text = "答案写进了 answers/2026-07-30-kv-cache.md,请查收。";
        assert_eq!(
            collect(v.path(), &task, text, since),
            vec!["answers/2026-07-30-kv-cache.md"]
        );
    }

    #[test]
    fn accepts_an_absolute_path_inside_the_vault() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        let abs = v.path().join("answers/deep.md");
        touch(&abs, "# a");
        let text = format!("wrote {}", abs.display());
        assert_eq!(
            collect(v.path(), &task, &text, SystemTime::now()),
            vec!["answers/deep.md"]
        );
    }

    #[test]
    fn drops_paths_that_do_not_exist_or_sit_outside_the_vault() {
        let v = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        touch(&outside.path().join("secret.md"), "# nope");
        let text = format!(
            "imagined.md and answers/ghost.md and {}",
            outside.path().join("secret.md").display()
        );
        assert!(collect(v.path(), &task, &text, SystemTime::now()).is_empty());
    }

    #[test]
    fn strips_markdown_link_punctuation_around_a_path() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        touch(&v.path().join("answers/a.md"), "# a");
        for text in [
            "see [the answer](answers/a.md)",
            "see `answers/a.md`",
            "see answers/a.md.",
            "see <answers/a.md>",
        ] {
            assert_eq!(
                collect(v.path(), &task, text, SystemTime::now()),
                vec!["answers/a.md"],
                "failed for {text}"
            );
        }
    }

    #[test]
    fn deduplicates_a_file_that_is_both_written_and_mentioned() {
        let v = tempfile::tempdir().unwrap();
        let task = v.path().join(".notemd/agent-tasks/t");
        touch(&task.join("output/selfcheck.md"), "# hi");
        let since = SystemTime::now() - Duration::from_secs(5);
        let got = collect(
            v.path(),
            &task,
            "报告写入 .notemd/agent-tasks/t/output/selfcheck.md",
            since,
        );
        assert_eq!(got.len(), 1);
    }
}
