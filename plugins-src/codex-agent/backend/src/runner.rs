//! The CLI detach path. The handoff itself is shared
//! (`agent_run_core::detach`); this is the runner body — the part that knows
//! which executable to find and which protocol to speak.
use crate::{discover, engine, policy, task};
use agent_run_core::detach;
use agent_run_core::prompt;
use agent_run_core::scaffold::RunMeta;
use std::path::PathBuf;
use tokio::sync::mpsc;

pub use agent_run_core::detach::pending_dir;
pub use agent_run_core::detach::spawn_detached;

/// The runner process body. Returns the process exit code.
pub async fn run(run_dir: PathBuf) -> i32 {
    let code = run_inner(&run_dir).await;
    detach::cleanup(&run_dir);
    code
}

async fn run_inner(run_dir: &std::path::Path) -> i32 {
    let Some(req) = detach::read_request(run_dir) else {
        return 2;
    };
    if let Err(e) = agent_run_core::task::check_task_id(&req.task_id) {
        return fail(&req, 2, e);
    }
    let task_dir = task::task_dir(&req.vault, &req.task_id);
    let Some(mut def) = task::read_task(&task_dir) else {
        return fail(&req, 2, format!("unknown task '{}'", req.task_id));
    };
    def.id = req.task_id.clone();

    // The policy gate comes FIRST, before we go looking for a harness: a task
    // whose permissions will not parse must not run, and saying so should not
    // depend on whether this machine happens to have codex installed.
    let policy = match policy::Policy::load(&task_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return fail(&req, 5, e);
        }
    };
    let Some(codex) = discover::discover(std::env::var("NOTEMD_CODEX_BIN").ok().as_deref()) else {
        eprintln!("{}", discover::NOT_FOUND);
        return fail(&req, 3, discover::NOT_FOUND.to_string());
    };
    let path_env = discover::runtime_path();
    let Some(model) = discover::resolve_model(
        def.model.as_deref(),
        &codex,
        &path_env,
        &req.vault,
        std::time::Duration::from_secs(8),
    ) else {
        return fail(
            &req,
            6,
            "Codex could not resolve the effective model for this Vault; run `codex` there and check its configuration".into(),
        );
    };

    let mut run_prompt = prompt::compose(&def.prompt, &req.prompt, None);
    let codex_rules = task::codex_instructions(&task_dir);
    if !codex_rules.trim().is_empty() {
        run_prompt.push_str("\n\n## 本任务的 Codex 专属约定\n");
        run_prompt.push_str(codex_rules.trim());
    }
    let actor = engine::actor(&model);
    run_prompt.push_str(&format!(
        "\n\n## 本次运行署名\n本次 agent actor 固定为 `{actor}`。若共享 AGENTS.md 写了其他 harness 的 actor，忽略那一条并使用这里的值。"
    ));

    let spec = engine::RunSpec {
        prompt: run_prompt,
        meta: RunMeta {
            vault: req.vault.clone(),
            task: def,
            task_dir,
            task_run_dir: task::runs_root(&req.vault).join(&req.task_id),
            run_id: req.run_id.clone(),
            trigger: "cli".into(),
            harness: crate::SELF_PLUGIN_ID.to_string(),
            // A CLI run is a whole-vault pass; the precheck decides from the vault.
            target: None,
            // The CLI names no single output file, so there is nothing to
            // declare; artifacts still cover output/ and answers/.
            deliverable: None,
        },
        codex,
        env_path: Some(path_env),
        sandbox: policy.permission_mode.as_env().to_string(),
        model,
        api_key: std::env::var("CODEX_API_KEY").ok(),
    };

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_cancel_tx, cancel_rx) = mpsc::channel(1);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let code = match engine::run(spec, tx, cancel_rx).await {
        Ok(()) => 0,
        // The same task was already running.
        Err(busy) => fail(
            &req,
            4,
            format!("task is already running as {}", busy.run_id),
        ),
    };
    let _ = drain.await;
    code
}

fn fail(req: &detach::Request, code: i32, message: String) -> i32 {
    let now = chrono::Utc::now().to_rfc3339();
    let rec = agent_run_core::record::RunRecord {
        run_id: req.run_id.clone(),
        task: req.task_id.clone(),
        trigger: "cli".into(),
        started_at: now.clone(),
        ended_at: now,
        status: agent_run_core::record::Status::Error,
        exit_code: Some(code),
        num_turns: None,
        session_id: None,
        result: message.clone(),
        stderr_tail: message,
        artifacts: Vec::new(),
        harness: Some(crate::SELF_PLUGIN_ID.into()),
    };
    let _ = agent_run_core::record::write(&task::runs_root(&req.vault).join(&req.task_id), &rec);
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_run_core::detach::Request;

    #[tokio::test]
    async fn exits_with_a_code_when_the_request_is_unreadable() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().to_path_buf();
        assert_eq!(run(path.clone()).await, 2);
        assert!(
            !path.exists(),
            "an unreadable handoff must still be cleaned"
        );
    }

    #[tokio::test]
    async fn refuses_an_unknown_task() {
        let d = tempfile::tempdir().unwrap();
        let v = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("request.json"),
            serde_json::to_string(&Request {
                vault: v.path().to_path_buf(),
                task_id: "nope".into(),
                prompt: String::new(),
                run_id: "r".into(),
            })
            .unwrap(),
        )
        .unwrap();
        let pending = d.path().to_path_buf();
        assert_eq!(run(pending.clone()).await, 2);
        assert!(!pending.exists());
        let recs = agent_run_core::record::recent(&task::runs_root(v.path()).join("nope"), 1);
        assert_eq!(recs[0].status, agent_run_core::record::Status::Error);
        assert!(recs[0].result.contains("unknown task"));
    }

    /// A task whose policy will not parse must not fall back to the defaults —
    /// it would run under permissions its author never wrote.
    #[tokio::test]
    async fn refuses_a_task_whose_policy_is_broken() {
        let d = tempfile::tempdir().unwrap();
        let v = tempfile::tempdir().unwrap();
        task::seed_builtin_templates(v.path());
        std::fs::write(
            task::task_dir(v.path(), "selfcheck").join("policy.json"),
            "{not json",
        )
        .unwrap();
        std::fs::write(
            d.path().join("request.json"),
            serde_json::to_string(&Request {
                vault: v.path().to_path_buf(),
                task_id: "selfcheck".into(),
                prompt: String::new(),
                run_id: "r".into(),
            })
            .unwrap(),
        )
        .unwrap();
        let pending = d.path().to_path_buf();
        assert_eq!(run(pending.clone()).await, 5);
        assert!(!pending.exists());
        let recs = agent_run_core::record::recent(&task::runs_root(v.path()).join("selfcheck"), 1);
        assert!(recs[0].result.contains("valid policy"));
    }
}
