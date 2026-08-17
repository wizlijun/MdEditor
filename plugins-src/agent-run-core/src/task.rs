//! Task templates: `<vault>/.notemd/agent-tasks/<id>/` holding task.json, the
//! instruction file, and whatever policy files the harness wants. Plain text,
//! git-tracked, editable by hand — running one by `cd <task> && <harness>`
//! outside note.md is exactly equivalent.
//!
//! Both agent plugins share ONE tasks root and ONE runs root on purpose: a task
//! is a job description, not a harness binding, so the same template can be run
//! by whichever agent the user points at it.
//!
//! Generalized from claude-agent: the built-in template TABLE is a parameter
//! ([`Templates`]), because each plugin compiles its own instruction files in.
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
    /// Script in the task directory that decides whether the run is worth
    /// starting at all. Exit 0 = run; anything else skips with its output.
    #[serde(default)]
    pub precheck: Option<String>,
    /// OKF `type` for the file this task delivers, used only by the fallback
    /// stamp when the model forgot its own frontmatter. Unset ⇒
    /// [`crate::okf::DEFAULT_TYPE`]. Must be registered in `src/lib/okf/concept.ts`.
    #[serde(default)]
    pub okf_type: Option<String>,
}

fn default_timeout() -> u64 {
    1800
}

/// One plugin's compiled-in templates: `(task id, [(relative path, body)])`.
pub type Templates = [(&'static str, &'static [(&'static str, &'static str)])];

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

/// A task id names ONE directory under `.notemd/agent-tasks/` — nothing else.
///
/// This is a security check, not tidiness: [`task_dir`] joins the id straight
/// onto the tasks root, and a task directory IS the permission policy of the run
/// it starts. An id like `../../evil` would let a caller point that policy at any
/// directory on disk. Rejects separators, `.`/`..` and absolute paths.
pub fn valid_task_id(id: &str) -> bool {
    if id.is_empty() || id.contains('\\') {
        // Backslash is a separator on Windows and merely a legal filename char
        // on unix — refuse it either way rather than depend on the platform.
        return false;
    }
    let mut comps = Path::new(id).components();
    matches!(comps.next(), Some(std::path::Component::Normal(_))) && comps.next().is_none()
}

pub fn check_task_id(id: &str) -> Result<(), String> {
    valid_task_id(id)
        .then_some(())
        .ok_or_else(|| format!("invalid task id '{id}'"))
}

/// Move a renamed built-in task — template AND run history — to its new id.
/// Skipped whenever the new name already exists: what the user has now wins,
/// and nothing they wrote gets clobbered.
pub fn migrate_renamed_tasks(vault: &Path, renames: &[(&str, &str)]) -> Vec<String> {
    let mut moved = Vec::new();
    for (old, new) in renames {
        for root in [tasks_root(vault), runs_root(vault)] {
            let (from, to) = (root.join(old), root.join(new));
            if from.is_dir() && !to.exists() && std::fs::rename(&from, &to).is_ok() {
                moved.push(format!("{old} → {new}"));
            }
        }
        // The records carry the task id too; leaving it stale would label
        // history rows with a name that no longer exists.
        retag_records(&runs_root(vault).join(new), old, new);
    }
    moved
}

fn retag_records(task_run_dir: &Path, old: &str, new: &str) {
    let Ok(rd) = std::fs::read_dir(task_run_dir.join("runs")) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(body) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&body) else {
            continue;
        };
        if v.get("task").and_then(|t| t.as_str()) != Some(old) {
            continue;
        }
        v["task"] = serde_json::Value::String(new.to_string());
        if let Ok(s) = serde_json::to_string_pretty(&v) {
            let _ = std::fs::write(&p, s + "\n");
        }
    }
}

/// Seed AND refresh a plugin's built-in task templates. The built-ins are owned
/// by the plugin, not by the vault: their prompts encode the `.note.md` write
/// protocol, so a stale copy is not a harmless preference — it makes the agent
/// produce answers the outline parser shreds on the next save. Every plugin
/// update therefore rewrites them from the binary.
///
/// Identical content is left untouched (no mtime churn, no git-sync noise), so
/// this stays idempotent. To customise a prompt, copy the task to a new id
/// instead of editing a built-in. Returns the relative paths actually written.
pub fn seed_templates(vault: &Path, templates: &Templates) -> Vec<String> {
    let mut wrote = Vec::new();
    for (id, files) in templates {
        for (rel, body) in *files {
            let p = task_dir(vault, id).join(rel);
            if std::fs::read_to_string(&p).is_ok_and(|cur| cur == *body) {
                continue;
            }
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&p, body).is_ok() {
                // A precheck that isn't executable is a precheck that silently
                // never runs.
                if rel.ends_with(".sh") {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
                }
                wrote.push(format!("{id}/{rel}"));
            }
        }
    }
    wrote
}

/// Keep derived data out of the vault's git history. Idempotent line-append,
/// same shape as `agents_sync::ensure_gitignore` in the host.
pub fn ensure_gitignore(vault: &Path, lines: &[&str]) {
    let gi = vault.join(".gitignore");
    let cur = std::fs::read_to_string(&gi).unwrap_or_default();
    let missing: Vec<&str> = lines
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

    const T: &Templates = &[
        (
            "alpha",
            &[("task.json", r#"{"name":"A","prompt":"p"}"#), ("AGENTS.md", "# alpha")],
        ),
        (
            "beta",
            &[("task.json", r#"{"name":"B","prompt":"q"}"#), ("check.sh", "#!/bin/sh\nexit 0\n")],
        ),
    ];

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
    fn seeds_every_template_file_on_a_fresh_vault() {
        let v = tempfile::tempdir().unwrap();
        let wrote = seed_templates(v.path(), T);
        assert_eq!(wrote.len(), 4, "seeded: {wrote:?}");
        assert!(task_dir(v.path(), "alpha").join("AGENTS.md").exists());
        let ids: Vec<String> = discover(v.path()).into_iter().map(|t| t.id).collect();
        assert_eq!(ids, vec!["alpha", "beta"]);
    }

    /// 内置模板归插件所有:过期的必须被刷新。老 vault 停在旧 prompt 上会让 agent
    /// 写出解析器会撕碎的答复,所以每次更新都从二进制重写。
    #[test]
    fn refreshes_a_stale_template() {
        let v = tempfile::tempdir().unwrap();
        seed_templates(v.path(), T);
        let p = task_dir(v.path(), "alpha").join("AGENTS.md");
        std::fs::write(&p, "STALE").unwrap();
        let wrote = seed_templates(v.path(), T);
        assert_eq!(wrote, vec!["alpha/AGENTS.md"]);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "# alpha");
    }

    /// 幂等:内容已经一致就一个字节都不写 —— 否则每次启动都刷 mtime,vault 的
    /// git auto-sync 会被无谓的改动刷屏。
    #[test]
    fn leaves_an_up_to_date_template_untouched() {
        let v = tempfile::tempdir().unwrap();
        seed_templates(v.path(), T);
        assert!(seed_templates(v.path(), T).is_empty());
    }

    #[test]
    fn a_seeded_shell_script_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let v = tempfile::tempdir().unwrap();
        seed_templates(v.path(), T);
        let sh = task_dir(v.path(), "beta").join("check.sh");
        let mode = std::fs::metadata(&sh).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "check.sh must be executable, got {mode:o}");
    }

    #[test]
    fn migrates_a_renamed_task_with_its_history() {
        let v = tempfile::tempdir().unwrap();
        write_task(v.path(), "old-name", r#"{"name":"Old","prompt":"mine"}"#);
        let old_runs = runs_root(v.path()).join("old-name/runs");
        std::fs::create_dir_all(&old_runs).unwrap();
        std::fs::write(
            old_runs.join("20260817T000001Z-a.json"),
            r#"{"run_id":"20260817T000001Z-a","task":"old-name","trigger":"window",
                "started_at":"a","ended_at":"b","status":"success","exit_code":0,
                "num_turns":1,"session_id":null,"result":"ok","stderr_tail":""}"#,
        )
        .unwrap();

        let moved = migrate_renamed_tasks(v.path(), &[("old-name", "new-name")]);
        assert_eq!(moved.len(), 2, "template and run history both move");

        // The user's own edit rode along.
        let t = read_task(&task_dir(v.path(), "new-name")).unwrap();
        assert_eq!(t.prompt, "mine");
        assert!(!task_dir(v.path(), "old-name").exists());

        // History followed, and is labelled with the new id.
        let recs = crate::record::recent(&runs_root(v.path()).join("new-name"), 5);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].task, "new-name");
    }

    #[test]
    fn migration_leaves_an_existing_new_task_alone() {
        let v = tempfile::tempdir().unwrap();
        write_task(v.path(), "old-name", r#"{"name":"Old"}"#);
        write_task(v.path(), "new-name", r#"{"name":"Current"}"#);
        assert!(migrate_renamed_tasks(v.path(), &[("old-name", "new-name")]).is_empty());
        assert_eq!(
            read_task(&task_dir(v.path(), "new-name")).unwrap().name,
            "Current"
        );
        assert!(task_dir(v.path(), "old-name").exists());
    }

    #[test]
    fn migration_is_a_no_op_on_a_vault_that_never_had_the_old_name() {
        let v = tempfile::tempdir().unwrap();
        seed_templates(v.path(), T);
        assert!(migrate_renamed_tasks(v.path(), &[("old-name", "alpha")]).is_empty());
    }

    #[test]
    fn gitignore_appends_once_and_preserves_existing_lines() {
        let v = tempfile::tempdir().unwrap();
        std::fs::write(v.path().join(".gitignore"), "node_modules\n").unwrap();
        let lines = [".notemd/agent-runs/", ".notemd/agent-tasks/*/.claude/settings.local.json"];
        ensure_gitignore(v.path(), &lines);
        ensure_gitignore(v.path(), &lines);
        let gi = std::fs::read_to_string(v.path().join(".gitignore")).unwrap();
        assert!(gi.starts_with("node_modules\n"));
        assert_eq!(gi.matches(".notemd/agent-runs/").count(), 1);
        assert_eq!(gi.matches("settings.local.json").count(), 1);
    }

    #[test]
    fn gitignore_on_a_vault_with_no_file_yet_creates_one() {
        let v = tempfile::tempdir().unwrap();
        ensure_gitignore(v.path(), &[".notemd/agent-runs/"]);
        assert_eq!(
            std::fs::read_to_string(v.path().join(".gitignore")).unwrap(),
            ".notemd/agent-runs/\n"
        );
    }

    /// A task directory is the run's permission policy. An id that can leave
    /// `.notemd/agent-tasks/` would let a caller point that policy at a
    /// directory it planted.
    #[test]
    fn a_task_id_may_only_name_one_directory() {
        for good in ["selfcheck", "ai-read-ebook", "答疑", "a.b", "a..b"] {
            assert!(valid_task_id(good), "{good} must be allowed");
        }
        for bad in ["", "..", ".", "../evil", "../../etc", "a/b", "/abs/path", "./a", "a\\b", "..\\evil"] {
            assert!(!valid_task_id(bad), "{bad} must be refused");
        }
        assert!(check_task_id("../evil").unwrap_err().contains("invalid task id"));
    }
}
