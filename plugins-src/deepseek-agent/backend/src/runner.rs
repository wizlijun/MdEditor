//! The CLI detach path. The handoff itself is shared
//! (`agent_run_core::detach`); this is the runner body — the part that knows
//! which executable to find and which protocol to speak.
use crate::{composition, discover, engine, policy, task};
use agent_run_core::detach;
use agent_run_core::prompt;
use agent_run_core::scaffold::RunMeta;
use std::path::PathBuf;
use tokio::sync::mpsc;

pub use agent_run_core::detach::spawn_detached;

/// The runner process body. Returns the process exit code.
pub async fn run(run_dir: PathBuf) -> i32 {
    let Some(req) = detach::read_request(&run_dir) else {
        return 2;
    };
    let task_dir = task::task_dir(&req.vault, &req.task_id);
    let Some(mut def) = task::read_task(&task_dir) else {
        return 2;
    };
    def.id = req.task_id.clone();

    // The policy gate comes FIRST, before we go looking for a harness: a task
    // whose permissions will not parse must not run, and saying so should not
    // depend on whether this machine happens to have dsh installed.
    let policy = match policy::Policy::load(&task_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return 5;
        }
    };
    let Some(launcher) = discover::discover(None, None) else {
        eprintln!("{}", discover::NOT_FOUND);
        return 3;
    };
    let Ok(config) = composition::resolve_config(&req.vault, None) else {
        return 3;
    };

    let spec = engine::RunSpec {
        prompt: prompt::compose(&def.prompt, &req.prompt, None),
        meta: RunMeta {
            vault: req.vault.clone(),
            task: def,
            task_dir,
            task_run_dir: task::runs_root(&req.vault).join(&req.task_id),
            run_id: req.run_id.clone(),
            trigger: "cli".into(),
            // A CLI run is a whole-vault pass; the precheck decides from the vault.
            target: None,
            // The CLI names no single output file, so there is nothing to
            // declare; artifacts still cover output/ and answers/.
            deliverable: None,
        },
        launcher,
        config,
        env_path: None,
        sessions_dir: composition::sessions_dir(&req.vault),
        policy,
        // A detached runner has no window, so an `ask` policy fails closed.
        window_open: false,
        api_key: std::env::var("DEEPSEEK_API_KEY").ok(),
    };

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_cancel_tx, cancel_rx) = mpsc::channel(1);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let code = match engine::run(spec, tx, cancel_rx).await {
        Ok(()) => 0,
        // The same task was already running.
        Err(_busy) => 4,
    };
    let _ = drain.await;
    detach::cleanup(&run_dir);
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_run_core::detach::Request;

    #[tokio::test]
    async fn exits_with_a_code_when_the_request_is_unreadable() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(run(d.path().to_path_buf()).await, 2);
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
        assert_eq!(run(d.path().to_path_buf()).await, 2);
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
        assert_eq!(run(d.path().to_path_buf()).await, 5);
    }
}
