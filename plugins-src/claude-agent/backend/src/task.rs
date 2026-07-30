//! Task templates: `<vault>/.notemd/agent-tasks/<id>/` holding task.json,
//! CLAUDE.md and .claude/. Plain text, git-tracked, editable by hand — running
//! one by `cd <task> && claude -p …` outside note.md is exactly equivalent.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskDef {
    /// The directory name. Filled in from disk; serialized out to the window.
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub max_turns: Option<u64>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub model: Option<String>,
}

fn default_timeout() -> u64 {
    1800
}

pub fn tasks_root(vault: &Path) -> PathBuf {
    vault.join(".notemd/agent-tasks")
}

pub fn runs_root(vault: &Path) -> PathBuf {
    vault.join(".notemd/agent-runs")
}

pub fn task_dir(vault: &Path, id: &str) -> PathBuf {
    tasks_root(vault).join(id)
}

/// Scan the task directory. A template whose task.json won't parse is skipped
/// rather than fatal — one broken template shouldn't blank the whole list.
/// Sorted by id so the window's list order is stable.
pub fn discover(vault: &Path) -> Vec<TaskDef> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(tasks_root(vault)) else {
        return out;
    };
    for e in rd.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let id = e.file_name().to_string_lossy().to_string();
        if let Some(mut t) = read_task(&e.path()) {
            t.id = id;
            out.push(t);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn read_task(dir: &Path) -> Option<TaskDef> {
    let s = std::fs::read_to_string(dir.join("task.json")).ok()?;
    serde_json::from_str(&s).ok()
}

/// The built-in templates, compiled into the binary and seeded on first run.
const BUILTIN: &[(&str, &[(&str, &str)])] = &[
    (
        "selfcheck",
        &[
            ("task.json", include_str!("../templates/selfcheck/task.json")),
            ("CLAUDE.md", include_str!("../templates/selfcheck/CLAUDE.md")),
            (
                ".claude/settings.json",
                include_str!("../templates/selfcheck/settings.json"),
            ),
        ],
    ),
    (
        "annotation-sweep",
        &[
            (
                "task.json",
                include_str!("../templates/annotation-sweep/task.json"),
            ),
            (
                "CLAUDE.md",
                include_str!("../templates/annotation-sweep/CLAUDE.md"),
            ),
            (
                ".claude/settings.json",
                include_str!("../templates/annotation-sweep/settings.json"),
            ),
        ],
    ),
];

/// Idempotent: fill in what's missing, never overwrite. A template the user
/// edited is theirs. Returns the relative paths actually written, for logging.
pub fn seed_builtin_templates(vault: &Path) -> Vec<String> {
    let mut wrote = Vec::new();
    for (id, files) in BUILTIN {
        for (rel, body) in *files {
            let p = task_dir(vault, id).join(rel);
            if p.exists() {
                continue;
            }
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&p, body).is_ok() {
                wrote.push(format!("{id}/{rel}"));
            }
        }
    }
    wrote
}

/// Keep derived data out of the vault's git history. Same idempotent
/// line-append shape as `agents_sync::ensure_gitignore` in the host.
pub fn ensure_gitignore(vault: &Path) {
    const LINES: [&str; 2] = [
        ".notemd/agent-runs/",
        ".notemd/agent-tasks/*/.claude/settings.local.json",
    ];
    let gi = vault.join(".gitignore");
    let cur = std::fs::read_to_string(&gi).unwrap_or_default();
    let missing: Vec<&str> = LINES
        .iter()
        .copied()
        .filter(|l| !cur.lines().any(|e| e.trim() == *l))
        .collect();
    if missing.is_empty() {
        return;
    }
    let mut next = cur;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    for l in missing {
        next.push_str(l);
        next.push('\n');
    }
    let _ = std::fs::write(&gi, next);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_task(root: &Path, id: &str, json: &str) {
        let d = tasks_root(root).join(id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("task.json"), json).unwrap();
    }

    #[test]
    fn discovers_tasks_sorted_by_id_with_the_dir_name_as_id() {
        let v = tempfile::tempdir().unwrap();
        write_task(v.path(), "zeta", r#"{"name":"Z"}"#);
        write_task(v.path(), "alpha", r#"{"name":"A","description":"d","prompt":"p"}"#);
        let got = discover(v.path());
        assert_eq!(
            got.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
        assert_eq!(got[0].name, "A");
        assert_eq!(got[0].prompt, "p");
    }

    #[test]
    fn defaults_timeout_to_half_an_hour() {
        let v = tempfile::tempdir().unwrap();
        write_task(v.path(), "t", r#"{"name":"T"}"#);
        assert_eq!(discover(v.path())[0].timeout_seconds, 1800);
    }

    #[test]
    fn skips_dirs_whose_task_json_is_broken_or_missing() {
        let v = tempfile::tempdir().unwrap();
        write_task(v.path(), "good", r#"{"name":"G"}"#);
        write_task(v.path(), "broken", "{not json");
        std::fs::create_dir_all(tasks_root(v.path()).join("empty")).unwrap();
        assert_eq!(discover(v.path()).len(), 1);
    }

    #[test]
    fn returns_empty_when_the_vault_has_no_tasks_dir() {
        let v = tempfile::tempdir().unwrap();
        assert!(discover(v.path()).is_empty());
    }

    #[test]
    fn seeds_both_builtin_templates_on_a_fresh_vault() {
        let v = tempfile::tempdir().unwrap();
        let wrote = seed_builtin_templates(v.path());
        assert_eq!(wrote.len(), 6);
        assert!(task_dir(v.path(), "selfcheck").join("CLAUDE.md").exists());
        assert!(task_dir(v.path(), "annotation-sweep")
            .join(".claude/settings.json")
            .exists());
        let ids: Vec<String> = discover(v.path()).into_iter().map(|t| t.id).collect();
        assert_eq!(ids, vec!["annotation-sweep", "selfcheck"]);
    }

    #[test]
    fn never_overwrites_a_template_the_user_edited() {
        let v = tempfile::tempdir().unwrap();
        seed_builtin_templates(v.path());
        let mine = task_dir(v.path(), "selfcheck").join("CLAUDE.md");
        std::fs::write(&mine, "MINE").unwrap();
        let wrote = seed_builtin_templates(v.path());
        assert!(wrote.is_empty());
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), "MINE");
    }

    #[test]
    fn gitignore_appends_once_and_preserves_existing_lines() {
        let v = tempfile::tempdir().unwrap();
        std::fs::write(v.path().join(".gitignore"), "node_modules\n").unwrap();
        ensure_gitignore(v.path());
        ensure_gitignore(v.path());
        let gi = std::fs::read_to_string(v.path().join(".gitignore")).unwrap();
        assert!(gi.starts_with("node_modules\n"));
        assert_eq!(gi.matches(".notemd/agent-runs/").count(), 1);
        assert_eq!(gi.matches("settings.local.json").count(), 1);
    }

    #[test]
    fn builtin_task_json_files_actually_parse() {
        let v = tempfile::tempdir().unwrap();
        seed_builtin_templates(v.path());
        let tasks = discover(v.path());
        assert_eq!(tasks.len(), 2);
        assert!(tasks.iter().all(|t| !t.name.is_empty() && !t.prompt.is_empty()));
    }
}
