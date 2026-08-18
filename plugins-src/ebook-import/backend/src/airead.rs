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
    /// Which agent should read the book. `None` = whatever the host would pick.
    /// Chosen in the window (the `by X ▾` picker beside the AI-read button) and
    /// carried here so the choice survives the queue: a job can sit behind
    /// others for a long time, and it must run on the agent it was queued for.
    pub harness: Option<String>,
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
    /// worker 退出时无条件放下标志(见 plugin.rs 的 WorkerSlot)。正常收尾时
    /// `next` 已经放过了,这里是幂等的补刀:worker panic 掉而标志还举着,
    /// 之后所有「AI 先读」都只入队不执行,且没有任何办法恢复。
    pub fn release_worker(&mut self) {
        self.running = false;
    }
    /// 队里还剩几本 —— worker 异常退出后判断要不要再拉一个。
    pub fn pending(&self) -> usize {
        self.q.len()
    }
}

pub fn summary_name(date: chrono::NaiveDate) -> String {
    format!("{}-summary.md", date.format("%Y-%m-%d"))
}

/// 摘要正文该用哪种语言写。跟随**用户界面语言**($initialize 给的 locale),
/// 不跟随书的语言:读一本俄语书的人未必读得下俄语摘要,他要的是自己母语的
/// 「这本书讲什么」。书名与引文另说,由模板约定保留原文。
pub fn output_language(locale: &str) -> &'static str {
    match locale.split('-').next().unwrap_or("en") {
        "zh" => "简体中文",
        "ja" => "日本語",
        "de" => "Deutsch",
        _ => "English",
    }
}

/// 附加给 run-task 的定位 prompt(任务模板自带总 prompt,这里只给坐标)。
pub fn run_prompt(dest_rel: &str, summary_rel: &str, locale: &str) -> String {
    let lang = output_language(locale);
    format!(
        "本次只读这一本书:`{dest_rel}/book.md`。\n\
         摘要写到 `{summary_rel}`(同名文件已存在则直接覆盖)。\n\
         摘要正文一律用 {lang} 书写 —— 与原书语言无关;书名、专有名词、直接引文\n\
         可保留原文并在需要处附 {lang} 译文。\n\
         不要读、不要改 vault 里的其它文件 —— 权限也已按此限定。"
    )
}

/// 一次 host.agent.status 应答的解读。
#[derive(Debug, PartialEq)]
pub enum RunPoll {
    /// 还在跑;`steps` 是 run 自报的推进步数 —— 只要它在涨就是活的,
    /// 轮询侧据此把「无进展」上限往后推(见 plugin.rs 的轮询循环)。
    Running { steps: u64 },
    Succeeded,
    Failed(String),
}

pub fn interpret_status(v: &serde_json::Value) -> RunPoll {
    match v.get("state").and_then(|s| s.as_str()) {
        Some("running") => RunPoll::Running {
            steps: v.get("steps").and_then(|s| s.as_u64()).unwrap_or(0),
        },
        Some("done") => {
            let rec = v.get("record");
            let status = rec
                .and_then(|r| r.get("status"))
                .and_then(|s| s.as_str())
                .unwrap_or("error");
            if status == "success" {
                RunPoll::Succeeded
            } else {
                // `result` is claude's own closing message — empty whenever the
                // process never got that far (not logged in, spawn failure).
                // The real reason is in stderr_tail then, and "AI 阅读失败
                // error:" with nothing after the colon helps nobody.
                let detail = ["result", "stderr_tail"]
                    .iter()
                    .filter_map(|k| rec.and_then(|r| r.get(*k)).and_then(|s| s.as_str()))
                    .map(str::trim)
                    .find(|s| !s.is_empty())
                    .unwrap_or("no detail reported");
                RunPoll::Failed(format!("{status}: {detail}"))
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
        AiJob {
            job_id: id,
            dest_rel: format!("ssot/ebooks/2026-08/b{id}"),
            name: format!("b{id}"),
            harness: None,
        }
    }

    /// A job can sit behind others for a long time. It has to run on the agent
    /// chosen when it was queued — not on whatever the picker says by the time
    /// the worker reaches it.
    #[test]
    fn a_queued_job_carries_the_agent_it_was_queued_for() {
        let mut q = AiQueue::default();
        let mut with_agent = job(1);
        with_agent.harness = Some("notemd.deepseek-agent".into());
        assert!(q.enqueue(with_agent));
        assert!(q.enqueue(job(2)));
        assert_eq!(
            q.next().unwrap().harness.as_deref(),
            Some("notemd.deepseek-agent")
        );
        assert_eq!(q.next().unwrap().harness, None, "unset means the host picks");
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

    /// worker panic 后必须能重新拉起,否则「AI 先读」永久失灵。
    #[test]
    fn release_worker_lets_a_new_worker_take_over() {
        let mut q = AiQueue::default();
        q.enqueue(job(1));
        q.enqueue(job(2));
        assert!(q.claim_worker());
        assert_eq!(q.next(), Some(job(1)));
        // worker 在处理 job1 时炸了:标志还举着,队里还有 job2。
        assert!(!q.claim_worker());
        q.release_worker();
        assert_eq!(q.pending(), 1);
        assert!(q.claim_worker(), "a new worker must be able to take over");
        assert_eq!(q.next(), Some(job(2)));
    }

    #[test]
    fn summary_name_is_date_stamped() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 8, 4).unwrap();
        assert_eq!(summary_name(d), "2026-08-04-summary.md");
    }

    #[test]
    fn interpret_status_variants() {
        assert_eq!(
            interpret_status(&serde_json::json!({"state": "running", "steps": 3})),
            RunPoll::Running { steps: 3 }
        );
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

    /// claude not logged in / failing to start leaves `result` empty and the
    /// real reason in `stderr_tail`; without the fallback the user reads
    /// "AI 阅读失败 error:" and nothing else.
    #[test]
    fn a_failure_with_no_result_falls_back_to_stderr() {
        let got = interpret_status(&serde_json::json!({
            "state": "done",
            "record": {"status": "error", "result": "  ", "stderr_tail": "Invalid API key\n"},
        }));
        assert_eq!(got, RunPoll::Failed("error: Invalid API key".into()));

        // Nothing anywhere: still say something rather than a bare colon.
        let got = interpret_status(&serde_json::json!({
            "state": "done",
            "record": {"status": "error"},
        }));
        assert_eq!(got, RunPoll::Failed("error: no detail reported".into()));

        // result wins when it has content.
        let got = interpret_status(&serde_json::json!({
            "state": "done",
            "record": {"status": "error", "result": "ran out of turns", "stderr_tail": "noise"},
        }));
        assert_eq!(got, RunPoll::Failed("error: ran out of turns".into()));
    }

    /// 摘要语言跟界面走,不跟书走 —— 俄语书 + 中文界面 = 中文摘要。
    #[test]
    fn run_prompt_pins_the_output_language_to_the_ui_locale() {
        let p = run_prompt("ssot/books/2026-08/x", "ssot/books/2026-08/x/2026-08-04-summary.md", "zh-CN");
        assert!(p.contains("简体中文"), "got: {p}");
        assert!(run_prompt("d", "s", "ja").contains("日本語"));
        assert!(run_prompt("d", "s", "de").contains("Deutsch"));
        // 未知/未设 locale 落到英文,而不是静默跟随书的语言。
        assert!(run_prompt("d", "s", "").contains("English"));
        assert!(run_prompt("d", "s", "pt-BR").contains("English"));
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
