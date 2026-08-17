//! claude-agent's own task templates. The template MACHINERY — discovery,
//! seeding, rename migration, the gitignore lines — is shared and lives in
//! `agent-run-core`; what stays here is the table of files this plugin compiles
//! in, plus one claude-only migration (`retire_information_denies`, which edits
//! `.claude/settings.json` deny lists that no other harness has).
pub use agent_run_core::task::{discover, read_task, runs_root, task_dir, tasks_root, TaskDef};
use agent_run_core::task::{self as core, Templates};
use std::path::Path;

/// The built-in templates, compiled into the binary and seeded on first run.
const BUILTIN: &Templates = &[
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

/// Move claude-agent's renamed built-ins — template AND run history — to their
/// new ids, using the shared migration.
pub fn migrate_renamed_tasks(vault: &Path) -> Vec<String> {
    core::migrate_renamed_tasks(vault, RENAMES)
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
    core::seed_templates(vault, BUILTIN)
}

/// Keep derived data out of the vault's git history.
pub fn ensure_gitignore(vault: &Path) {
    core::ensure_gitignore(
        vault,
        &[
            ".notemd/agent-runs/",
            ".notemd/agent-tasks/*/.claude/settings.local.json",
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;






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
