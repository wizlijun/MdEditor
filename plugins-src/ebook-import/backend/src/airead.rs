//! "AI 先读"队列:done 的书逐本转给 claude-agent(任务锁是 per-task 的,
//! 只能串行),轮询 run 到收尾,经 host.notify 推托盘提醒。
//! 本模块只放可单测的纯逻辑;拉起 tokio 任务的粘合在 plugin.rs。
use std::collections::VecDeque;

pub const TASK_ID: &str = "ai-read-ebook";

#[derive(Debug, Clone, PartialEq)]
pub struct AiJob {
    pub job_id: u64,
    pub dest_rel: String,
    pub name: String,
}

/// FIFO + 单 worker 标志。所有方法都要在 Inner 的锁内调用,保证原子。
#[derive(Debug, Default)]
pub struct AiQueue {
    q: VecDeque<AiJob>,
    running: bool,
}

impl AiQueue {
    /// 入队;同 job_id 已在队中(重复点击)返回 false。
    pub fn enqueue(&mut self, job: AiJob) -> bool {
        if self.q.iter().any(|j| j.job_id == job.job_id) {
            return false;
        }
        self.q.push_back(job);
        true
    }
    /// 入队后是否要拉起 worker(已有 worker 在跑则不拉)。
    pub fn claim_worker(&mut self) -> bool {
        if self.running {
            return false;
        }
        self.running = true;
        true
    }
    /// worker 取下一本;队空时放下 running 标志并返回 None(worker 退出)。
    pub fn next(&mut self) -> Option<AiJob> {
        let j = self.q.pop_front();
        self.running = j.is_some();
        j
    }
}

pub fn summary_name(date: chrono::NaiveDate) -> String {
    format!("{}-summary.md", date.format("%Y-%m-%d"))
}

/// 附加给 run-task 的定位 prompt(任务模板自带总 prompt,这里只给坐标)。
pub fn run_prompt(dest_rel: &str, summary_rel: &str) -> String {
    format!(
        "本次只读这一本书:`{dest_rel}/book.md`。\n\
         摘要写到 `{summary_rel}`(同名文件已存在则直接覆盖)。\n\
         不要读、不要改 vault 里的其它文件 —— 权限也已按此限定。"
    )
}

/// 一次 host.agent.status 应答的解读。
#[derive(Debug, PartialEq)]
pub enum RunPoll {
    Running,
    Succeeded,
    Failed(String),
}

pub fn interpret_status(v: &serde_json::Value) -> RunPoll {
    match v.get("state").and_then(|s| s.as_str()) {
        Some("running") => RunPoll::Running,
        Some("done") => {
            let rec = v.get("record");
            let status = rec
                .and_then(|r| r.get("status"))
                .and_then(|s| s.as_str())
                .unwrap_or("error");
            if status == "success" {
                RunPoll::Succeeded
            } else {
                let tail = rec
                    .and_then(|r| r.get("result"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                RunPoll::Failed(format!("{status}: {tail}"))
            }
        }
        // 无 record 也无锁:进程死了,或任务锁被别处(窗口/CLI)抢占后我们的
        // run 没跑成 —— 都按失败重试处理。
        Some("lost") => RunPoll::Failed("run lost".into()),
        _ => RunPoll::Failed("unrecognized run status".into()),
    }
}

/// 托盘提醒标题;locale 来自 $initialize(InitializeParams.locale)。
pub fn reminder_title(locale: &str, name: &str, ok: bool) -> String {
    let (done, fail) = match locale.split('-').next().unwrap_or("en") {
        "zh" => (format!("《{name}》AI 摘要已生成"), format!("《{name}》AI 阅读失败")),
        "ja" => (
            format!("『{name}』AI 要約ができました"),
            format!("『{name}』AI リーディングに失敗しました"),
        ),
        "de" => (
            format!("KI-Zusammenfassung für „{name}“ ist fertig"),
            format!("KI-Lektüre von „{name}“ fehlgeschlagen"),
        ),
        _ => (
            format!("AI digest ready for \u{201c}{name}\u{201d}"),
            format!("AI reading failed for \u{201c}{name}\u{201d}"),
        ),
    };
    if ok { done } else { fail }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: u64) -> AiJob {
        AiJob { job_id: id, dest_rel: format!("ssot/ebooks/2026-08/b{id}"), name: format!("b{id}") }
    }

    #[test]
    fn enqueue_dedups_by_job_id() {
        let mut q = AiQueue::default();
        assert!(q.enqueue(job(1)));
        assert!(!q.enqueue(job(1)), "duplicate click must not double-queue");
        assert!(q.enqueue(job(2)));
    }

    #[test]
    fn claim_worker_only_once_until_drained() {
        let mut q = AiQueue::default();
        q.enqueue(job(1));
        assert!(q.claim_worker());
        assert!(!q.claim_worker(), "second start while running must not spawn");
        assert_eq!(q.next(), Some(job(1)));
        assert_eq!(q.next(), None); // 队空 → running 落下
        assert!(q.claim_worker(), "after drain a new worker may start");
    }

    #[test]
    fn summary_name_is_date_stamped() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 8, 4).unwrap();
        assert_eq!(summary_name(d), "2026-08-04-summary.md");
    }

    #[test]
    fn interpret_status_variants() {
        assert_eq!(interpret_status(&serde_json::json!({"state": "running", "steps": 3})), RunPoll::Running);
        assert_eq!(
            interpret_status(&serde_json::json!({"state": "done", "record": {"status": "success", "result": "ok"}})),
            RunPoll::Succeeded
        );
        assert!(matches!(
            interpret_status(&serde_json::json!({"state": "done", "record": {"status": "timeout", "result": "x"}})),
            RunPoll::Failed(e) if e.starts_with("timeout")
        ));
        assert!(matches!(interpret_status(&serde_json::json!({"state": "lost"})), RunPoll::Failed(_)));
        assert!(matches!(interpret_status(&serde_json::json!({})), RunPoll::Failed(_)));
    }

    #[test]
    fn reminder_titles_are_localized_and_distinct() {
        for ok in [true, false] {
            let all: Vec<String> = ["en", "zh", "ja", "de"]
                .iter()
                .map(|l| reminder_title(l, "深度工作", ok))
                .collect();
            for t in &all {
                assert!(t.contains("深度工作"));
            }
            let uniq: std::collections::HashSet<_> = all.iter().collect();
            assert_eq!(uniq.len(), 4, "each locale must differ: {all:?}");
        }
        assert_eq!(reminder_title("zh-CN", "x", true), reminder_title("zh", "x", true));
    }
}
