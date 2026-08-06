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
    /// Script in the task directory that decides whether the run is worth
    /// starting at all. Exit 0 = run; anything else skips with its output.
    #[serde(default)]
    pub precheck: Option<String>,
    /// OKF `type` for the file this task delivers, used only by the fallback
    /// stamp in `engine`/`okf` when the model forgot its own frontmatter.
    /// Unset ⇒ [`crate::okf::DEFAULT_TYPE`]. Must be a type registered in
    /// `src/lib/okf/concept.ts`.
    #[serde(default)]
    pub okf_type: Option<String>,
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
            (
                "task.json",
                include_str!("../templates/selfcheck/task.json"),
            ),
            (
                "CLAUDE.md",
                include_str!("../templates/selfcheck/CLAUDE.md"),
            ),
            (
                ".claude/settings.json",
                include_str!("../templates/selfcheck/settings.json"),
            ),
        ],
    ),
    (
        "answer-note-question",
        &[
            (
                "task.json",
                include_str!("../templates/answer-note-question/task.json"),
            ),
            (
                "CLAUDE.md",
                include_str!("../templates/answer-note-question/CLAUDE.md"),
            ),
            (
                ".claude/settings.json",
                include_str!("../templates/answer-note-question/settings.json"),
            ),
            (
                ".claude/settings.scoped.json",
                include_str!("../templates/answer-note-question/settings.scoped.json"),
            ),
            (
                "precheck.sh",
                include_str!("../templates/answer-note-question/precheck.sh"),
            ),
        ],
    ),
    (
        "ai-read-ebook",
        &[
            (
                "task.json",
                include_str!("../templates/ai-read-ebook/task.json"),
            ),
            (
                "CLAUDE.md",
                include_str!("../templates/ai-read-ebook/CLAUDE.md"),
            ),
            (
                ".claude/settings.json",
                include_str!("../templates/ai-read-ebook/settings.json"),
            ),
            (
                ".claude/settings.scoped.json",
                include_str!("../templates/ai-read-ebook/settings.scoped.json"),
            ),
        ],
    ),
];

/// Built-in tasks that have been renamed, oldest name first. Without a
/// migration a vault that already has the old directory shows BOTH tasks in the
/// list and splits its run history across the two names.
const RENAMES: &[(&str, &str)] = &[("annotation-sweep", "answer-note-question")];

/// Move a renamed built-in task — template AND run history — to its new id.
/// Skipped whenever the new name already exists: what the user has now wins,
/// and nothing they wrote gets clobbered.
pub fn migrate_renamed_tasks(vault: &Path) -> Vec<String> {
    let mut moved = Vec::new();
    for (old, new) in RENAMES {
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

/// Tools these templates used to deny and no longer do. Denying them was aimed
/// at keeping a run on one document, but they fetch information rather than
/// reach into the vault — the file scope is held by `Bash`/`Grep`/`Glob`, which
/// stay denied.
const RETIRED_DENIES: [&str; 4] = ["WebSearch", "WebFetch", "Task", "Skill"];

/// Drop `RETIRED_DENIES` from the built-in templates' deny lists, once.
///
/// It has to be a rewrite of the template: a `deny` in the project layer is
/// final, and `settings.local.json` cannot take it back. Guarded by a marker so
/// a user who deliberately re-denies one of these keeps their decision — and so
/// tasks the user wrote themselves are never rewritten at all.
pub fn retire_information_denies(vault: &Path) -> Vec<String> {
    let marker = tasks_root(vault).join(".migrations/retired-information-denies");
    if marker.exists() {
        return Vec::new();
    }
    let mut changed = Vec::new();
    for (id, files) in BUILTIN {
        for (rel, _) in *files {
            if !rel.ends_with(".json") || !rel.starts_with(".claude/") {
                continue;
            }
            let p = task_dir(vault, id).join(rel);
            let Ok(body) = std::fs::read_to_string(&p) else {
                continue;
            };
            let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&body) else {
                continue;
            };
            let Some(deny) = v
                .get_mut("permissions")
                .and_then(|p| p.get_mut("deny"))
                .and_then(|d| d.as_array_mut())
            else {
                continue;
            };
            let before = deny.len();
            deny.retain(|e| !e.as_str().is_some_and(|s| RETIRED_DENIES.contains(&s)));
            if deny.len() == before {
                continue;
            }
            if let Ok(s) = serde_json::to_string_pretty(&v) {
                if std::fs::write(&p, s + "\n").is_ok() {
                    changed.push(format!("{id}/{rel}"));
                }
            }
        }
    }
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&marker, "1\n");
    changed
}

/// Seed AND refresh the built-in task templates. The built-ins are owned by the
/// plugin, not by the vault: their prompts encode the `.note.md` write protocol,
/// so a stale copy is not a harmless preference — it makes the agent produce
/// answers the outline parser shreds on the next save. Every plugin update
/// therefore rewrites them from the binary.
///
/// Identical content is left untouched (no mtime churn, no git-sync noise), so
/// this stays idempotent. To customise a prompt, copy the task to a new id
/// instead of editing a built-in; if you did edit one, the vault is git-backed
/// and auto-committed, so the previous text is recoverable from history.
/// Returns the relative paths actually written, for logging.
pub fn seed_builtin_templates(vault: &Path) -> Vec<String> {
    let mut wrote = Vec::new();
    for (id, files) in BUILTIN {
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
        write_task(
            v.path(),
            "alpha",
            r#"{"name":"A","description":"d","prompt":"p"}"#,
        );
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
        // 3 files each + answer-note-question's precheck + ai-read-ebook's 4 files.
        assert_eq!(wrote.len(), 12, "seeded: {wrote:?}");
        assert!(task_dir(v.path(), "selfcheck").join("CLAUDE.md").exists());
        assert!(task_dir(v.path(), "answer-note-question")
            .join(".claude/settings.json")
            .exists());
        let ids: Vec<String> = discover(v.path()).into_iter().map(|t| t.id).collect();
        assert_eq!(
            ids,
            vec!["ai-read-ebook", "answer-note-question", "selfcheck"]
        );
    }

    fn deny_of(v: &Path, id: &str, file: &str) -> Vec<String> {
        let body = std::fs::read_to_string(task_dir(v, id).join(".claude").join(file)).unwrap();
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["permissions"]["deny"]
            .as_array()
            .map(|a| a.iter().map(|x| x.as_str().unwrap().to_string()).collect())
            .unwrap_or_default()
    }

    fn write_settings(v: &Path, id: &str, file: &str, body: &str) {
        let d = task_dir(v, id).join(".claude");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(file), body).unwrap();
    }

    #[test]
    fn retires_the_information_denies_from_a_vault_seeded_before_the_change() {
        // A `deny` in the project layer cannot be undone by settings.local.json,
        // so an old vault would keep the run offline no matter what code grants.
        let v = tempfile::tempdir().unwrap();
        write_settings(
            v.path(),
            "answer-note-question",
            "settings.scoped.json",
            r#"{"permissions":{"allow":["Read(x)"],"deny":["Bash","Grep","Glob","Task","WebSearch","WebFetch"]}}"#,
        );
        assert_eq!(retire_information_denies(v.path()).len(), 1);
        assert_eq!(
            deny_of(v.path(), "answer-note-question", "settings.scoped.json"),
            vec!["Bash", "Grep", "Glob"],
            "the file-scope denies are the point of the scoped policy and must stay"
        );
    }

    #[test]
    fn the_retirement_happens_once_and_then_respects_the_users_own_denies() {
        let v = tempfile::tempdir().unwrap();
        write_settings(
            v.path(),
            "selfcheck",
            "settings.json",
            r#"{"permissions":{"deny":["WebSearch"]}}"#,
        );
        assert_eq!(retire_information_denies(v.path()).len(), 1);
        // The user decides this task should stay offline after all.
        write_settings(
            v.path(),
            "selfcheck",
            "settings.json",
            r#"{"permissions":{"deny":["WebSearch"]}}"#,
        );
        assert!(retire_information_denies(v.path()).is_empty());
        assert_eq!(
            deny_of(v.path(), "selfcheck", "settings.json"),
            vec!["WebSearch"]
        );
    }

    #[test]
    fn a_task_the_user_wrote_is_never_touched() {
        let v = tempfile::tempdir().unwrap();
        write_settings(
            v.path(),
            "idea-proof",
            "settings.json",
            r#"{"permissions":{"deny":["WebSearch","Bash"]}}"#,
        );
        assert!(retire_information_denies(v.path()).is_empty());
        assert_eq!(
            deny_of(v.path(), "idea-proof", "settings.json"),
            vec!["WebSearch", "Bash"]
        );
    }

    #[test]
    fn migrates_a_renamed_task_with_its_history() {
        let v = tempfile::tempdir().unwrap();
        write_task(
            v.path(),
            "annotation-sweep",
            r#"{"name":"Old","prompt":"mine"}"#,
        );
        let old_runs = runs_root(v.path()).join("annotation-sweep/runs");
        std::fs::create_dir_all(&old_runs).unwrap();
        std::fs::write(
            old_runs.join("20260730T000001Z-a.json"),
            r#"{"run_id":"20260730T000001Z-a","task":"annotation-sweep","trigger":"window",
                "started_at":"a","ended_at":"b","status":"success","exit_code":0,
                "num_turns":1,"session_id":null,"result":"ok","stderr_tail":""}"#,
        )
        .unwrap();

        let moved = migrate_renamed_tasks(v.path());
        assert_eq!(moved.len(), 2, "template and run history both move");

        // The user's own edit rode along.
        let t = read_task(&task_dir(v.path(), "answer-note-question")).unwrap();
        assert_eq!(t.prompt, "mine");
        assert!(!task_dir(v.path(), "annotation-sweep").exists());

        // History followed, and is labelled with the new id.
        let recs = crate::record::recent(&runs_root(v.path()).join("answer-note-question"), 5);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].task, "answer-note-question");
    }

    #[test]
    fn migration_leaves_an_existing_new_task_alone() {
        let v = tempfile::tempdir().unwrap();
        write_task(v.path(), "annotation-sweep", r#"{"name":"Old"}"#);
        write_task(v.path(), "answer-note-question", r#"{"name":"Current"}"#);
        assert!(migrate_renamed_tasks(v.path()).is_empty());
        assert_eq!(
            read_task(&task_dir(v.path(), "answer-note-question"))
                .unwrap()
                .name,
            "Current"
        );
        assert!(task_dir(v.path(), "annotation-sweep").exists());
    }

    #[test]
    fn migration_is_a_no_op_on_a_vault_that_never_had_the_old_name() {
        let v = tempfile::tempdir().unwrap();
        seed_builtin_templates(v.path());
        assert!(migrate_renamed_tasks(v.path()).is_empty());
        let ids: Vec<String> = discover(v.path()).into_iter().map(|t| t.id).collect();
        assert_eq!(
            ids,
            vec!["ai-read-ebook", "answer-note-question", "selfcheck"]
        );
    }

    #[test]
    fn a_seeded_precheck_script_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let v = tempfile::tempdir().unwrap();
        seed_builtin_templates(v.path());
        let sh = task_dir(v.path(), "answer-note-question").join("precheck.sh");
        let mode = std::fs::metadata(&sh).unwrap().permissions().mode();
        assert!(
            mode & 0o111 != 0,
            "precheck.sh must be executable, got {mode:o}"
        );
        assert_eq!(
            read_task(&task_dir(v.path(), "answer-note-question"))
                .unwrap()
                .precheck
                .as_deref(),
            Some("precheck.sh")
        );
    }

    /// 内置模板归插件所有:过期的必须被刷新。老 vault 停在旧 prompt 上会让 agent
    /// 写出解析器会撕碎的答复(围栏被挤到续行),所以每次更新都从二进制重写。
    #[test]
    fn refreshes_a_stale_builtin_template() {
        let v = tempfile::tempdir().unwrap();
        seed_builtin_templates(v.path());
        let p = task_dir(v.path(), "answer-note-question").join("CLAUDE.md");
        std::fs::write(&p, "STALE PROMPT").unwrap();
        let wrote = seed_builtin_templates(v.path());
        assert!(wrote.iter().any(|w| w == "answer-note-question/CLAUDE.md"));
        let cur = std::fs::read_to_string(&p).unwrap();
        assert_ne!(cur, "STALE PROMPT", "过期模板必须被覆盖");
        assert!(cur.contains("绝不手写 `✦`"), "刷新后应带上最新的围栏约束");
    }

    /// 幂等:内容已经一致就一个字节都不写 —— 否则每次启动都刷 mtime,
    /// vault 的 git auto-sync 会被无谓的改动刷屏。
    #[test]
    fn leaves_an_up_to_date_template_untouched() {
        let v = tempfile::tempdir().unwrap();
        seed_builtin_templates(v.path());
        let wrote = seed_builtin_templates(v.path());
        assert!(wrote.is_empty());
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
        assert_eq!(tasks.len(), 3);
        assert!(tasks
            .iter()
            .all(|t| !t.name.is_empty() && !t.prompt.is_empty()));
    }
}
