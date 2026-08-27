//! "AI 先读"队列:每种 agent provider 有自己的 FIFO 与并行上限,不同
//! provider 可同时读;轮询 run 到收尾,经 host.notify 推托盘提醒。
//! 本模块只放可单测的纯逻辑;拉起 tokio 任务的粘合在 plugin.rs。
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const TASK_ID: &str = "ai-read-ebook";
pub const DEFAULT_PROVIDER: &str = "notemd.claude-agent";
pub const MIN_CONCURRENCY: usize = 1;
pub const MAX_CONCURRENCY: usize = 5;
const UNRESOLVED_PROVIDER: &str = "__host_default__";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSnapshot {
    pub default: String,
    pub limits: BTreeMap<String, usize>,
}

/// `host.agent.limits` / `host.agent.providers` 的调度快照。宿主已做校验,
/// 这里仍兼容旧版/手写应答:
/// number 或 string 都收,缺失/非法按 1,任何值最终都夹在 1..=5。
pub fn provider_snapshot(v: &serde_json::Value) -> ProviderSnapshot {
    let mut out = BTreeMap::new();
    if let Some(providers) = v.get("providers").and_then(|p| p.as_array()) {
        for provider in providers {
            let Some(id) = provider
                .get("id")
                .and_then(|id| id.as_str())
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            let raw = provider.get("max_concurrency");
            let limit = raw
                .and_then(|n| n.as_u64().map(|n| n as usize))
                .or_else(|| raw.and_then(|n| n.as_str()?.parse::<usize>().ok()))
                .unwrap_or(MIN_CONCURRENCY)
                .clamp(MIN_CONCURRENCY, MAX_CONCURRENCY);
            out.insert(id.to_string(), limit);
        }
    }
    let default = v
        .get("default")
        .and_then(|d| d.as_str())
        .filter(|d| !d.is_empty())
        .unwrap_or(DEFAULT_PROVIDER)
        .to_string();
    ProviderSnapshot {
        default,
        limits: out,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiJob {
    pub job_id: u64,
    pub dest_rel: String,
    pub name: String,
    /// Which agent should read the book. `None` is the legacy/unresolved shape;
    /// the scheduler pins it to the host snapshot's default before dispatch.
    /// A picker choice is carried here so it survives a long wait in the queue.
    pub harness: Option<String>,
}

/// 一次 [`AiQueue::enqueue`] 的结果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Enqueue {
    Queued,
    /// 这本书已经在队里、或正在被读。带上那个 job_id,窗口好把这一行绑到
    /// 已有的那次运行上跟着看进度,而不是卡在「排队中」等一个永不到来的推送。
    Duplicate(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerClaim {
    pub provider: String,
    pub worker_id: u64,
}

#[derive(Debug)]
struct ProviderLane {
    q: VecDeque<QueuedJob>,
    active: BTreeMap<u64, AiJob>,
    workers: BTreeSet<u64>,
    limit: usize,
}

#[derive(Debug)]
struct QueuedJob {
    order: u64,
    job: AiJob,
}

impl Default for ProviderLane {
    fn default() -> Self {
        Self {
            q: VecDeque::new(),
            active: BTreeMap::new(),
            workers: BTreeSet::new(),
            limit: MIN_CONCURRENCY,
        }
    }
}

/// 每 provider 一条 FIFO。所有方法都在 [`crate::plugin::Inner`] 的锁内调用,
/// 所以入队、去重、占 worker slot 和取 job 都是原子的。
#[derive(Debug, Default)]
pub struct AiQueue {
    lanes: BTreeMap<String, ProviderLane>,
    next_job_order: u64,
    next_worker_id: u64,
    scheduler_running: bool,
}

impl AiQueue {
    fn provider(job: &AiJob) -> &str {
        // `None` comes from an older host/window, from before the picker
        // existed. It cannot enter a real provider lane until the scheduler
        // reads and pins the host's current default.
        job.harness.as_deref().unwrap_or(UNRESOLVED_PROVIDER)
    }

    /// 入队。见 [`Enqueue`]。
    ///
    /// 身份是**书**(`dest_rel`),不是 job_id:同一本书可以从导入队列的
    /// 「AI 先读」和书库的「重读」两处点进来,job_id 不同,活儿是同一份。
    pub fn enqueue(&mut self, job: AiJob) -> Enqueue {
        let same = |j: &AiJob| j.job_id == job.job_id || j.dest_rel == job.dest_rel;
        let duplicate = self.lanes.values().find_map(|lane| {
            lane.active
                .values()
                .chain(lane.q.iter().map(|queued| &queued.job))
                .find(|j| same(j))
        });
        if let Some(dup) = duplicate {
            return Enqueue::Duplicate(dup.job_id);
        }
        self.next_job_order += 1;
        let order = self.next_job_order;
        self.lanes
            .entry(Self::provider(&job).to_string())
            .or_default()
            .q
            .push_back(QueuedJob { order, job });
        Enqueue::Queued
    }

    /// 把旧窗口未携带 harness 的任务固定到宿主当前 default。先固定再 run,
    /// 后续默认值变化不会把 status 路由到另一个 provider。合并按真实入队顺序,
    /// 所以显式选中 default provider 的 job 也保持同一条 FIFO。
    pub fn resolve_default(&mut self, provider: &str) {
        let Some(mut unresolved) = self.lanes.remove(UNRESOLVED_PROVIDER) else {
            return;
        };
        if unresolved.q.is_empty() {
            return;
        }
        for queued in &mut unresolved.q {
            queued.job.harness = Some(provider.to_string());
        }
        let lane = self.lanes.entry(provider.to_string()).or_default();
        let mut merged: Vec<_> = lane.q.drain(..).chain(unresolved.q).collect();
        merged.sort_by_key(|queued| queued.order);
        lane.q = merged.into();
    }

    /// 原子应用完整快照。应答里缺失的 provider 也降回 1;传空 map 就是读取
    /// 全失败时的 fail-closed,不能让上一次的 5 并行永久残留。
    pub fn apply_limits(&mut self, limits: &BTreeMap<String, usize>) {
        for (provider, lane) in &mut self.lanes {
            if provider == UNRESOLVED_PROVIDER {
                continue;
            }
            lane.limit = limits
                .get(provider)
                .copied()
                .unwrap_or(MIN_CONCURRENCY)
                .clamp(MIN_CONCURRENCY, MAX_CONCURRENCY);
        }
    }

    /// 为每条 lane 原子预占目前缺少的 worker。返回值里的每个 token 必须恰好
    /// 拉起一个 worker;先登记再 spawn,防止并发入队重复拉起超额 worker。
    pub fn claim_workers(&mut self) -> Vec<WorkerClaim> {
        let mut claims = Vec::new();
        for (provider, lane) in &mut self.lanes {
            if provider == UNRESOLVED_PROVIDER {
                continue;
            }
            let work = lane.active.len() + lane.q.len();
            let wanted = lane.limit.min(work);
            while lane.workers.len() < wanted {
                self.next_worker_id += 1;
                let worker_id = self.next_worker_id;
                lane.workers.insert(worker_id);
                claims.push(WorkerClaim {
                    provider: provider.clone(),
                    worker_id,
                });
            }
        }
        claims
    }

    /// 一个已占 slot 的 worker 取本 provider FIFO 的下一本。上限被调低时,
    /// 超额 worker 返回 None 自行退休;active job 从不被中断。
    pub fn next(&mut self, provider: &str, worker_id: u64) -> Option<AiJob> {
        let lane = self.lanes.get_mut(provider)?;
        if !lane.workers.contains(&worker_id)
            || lane.workers.len() > lane.limit
            || lane.active.contains_key(&worker_id)
        {
            return None;
        }
        let job = lane.q.pop_front()?.job;
        lane.active.insert(worker_id, job.clone());
        Some(job)
    }

    /// 正常完成/失败都释放 active 书;slot 保留,worker 可继续取下一本。
    pub fn finish(&mut self, provider: &str, worker_id: u64) {
        if let Some(lane) = self.lanes.get_mut(provider) {
            lane.active.remove(&worker_id);
        }
    }

    /// worker 退出(包括 panic)时只释放自己的 active 书和 slot。别的 worker
    /// 仍在读的书必须继续参与全局去重。
    pub fn release_worker(&mut self, provider: &str, worker_id: u64) {
        if let Some(lane) = self.lanes.get_mut(provider) {
            lane.active.remove(&worker_id);
            lane.workers.remove(&worker_id);
        }
    }

    pub fn claim_scheduler(&mut self) -> bool {
        if self.scheduler_running {
            return false;
        }
        self.scheduler_running = true;
        true
    }

    pub fn release_scheduler(&mut self) {
        self.scheduler_running = false;
    }

    /// pending、active、worker 都清空后 poller 才退出。worker 在队空后还需
    /// 再走一次 `next` + Drop,所以只看 pending 会过早停掉生命周期监督。
    pub fn idle(&self) -> bool {
        self.lanes.values().all(|lane| {
            lane.q.is_empty() && lane.active.is_empty() && lane.workers.is_empty()
        })
    }

    /// 队里还剩几本 —— worker 异常退出后判断要不要再拉一个。
    pub fn pending(&self) -> usize {
        self.lanes.values().map(|lane| lane.q.len()).sum()
    }

    #[cfg(test)]
    pub fn worker_count(&self, provider: &str) -> usize {
        self.lanes
            .get(provider)
            .map(|lane| lane.workers.len())
            .unwrap_or(0)
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
            harness: Some(DEFAULT_PROVIDER.into()),
        }
    }

    fn unresolved_job(id: u64) -> AiJob {
        AiJob {
            harness: None,
            ..job(id)
        }
    }

    fn provider_job(id: u64, provider: &str) -> AiJob {
        AiJob {
            harness: Some(provider.into()),
            ..job(id)
        }
    }

    fn limits(entries: &[(&str, usize)]) -> BTreeMap<String, usize> {
        entries
            .iter()
            .map(|(provider, limit)| ((*provider).to_string(), *limit))
            .collect()
    }

    #[test]
    fn provider_snapshot_accepts_default_current_and_legacy_limit_shapes() {
        let got = provider_snapshot(&serde_json::json!({
            "default": "notemd.deepseek-agent",
            "providers": [
                {"id": "notemd.claude-agent", "max_concurrency": 3},
                {"id": "notemd.codex-agent", "max_concurrency": "5"},
                {"id": "notemd.deepseek-agent", "max_concurrency": 99},
                {"id": "notemd.old-agent"},
                {"max_concurrency": 4}
            ]
        }));
        assert_eq!(got.default, "notemd.deepseek-agent");
        assert_eq!(got.limits["notemd.claude-agent"], 3);
        assert_eq!(got.limits["notemd.codex-agent"], 5);
        assert_eq!(got.limits["notemd.deepseek-agent"], 5);
        assert_eq!(got.limits["notemd.old-agent"], 1);
        assert_eq!(got.limits.len(), 4);

        let legacy = provider_snapshot(&serde_json::json!({}));
        assert_eq!(legacy.default, DEFAULT_PROVIDER);
        assert!(legacy.limits.is_empty());
    }

    #[test]
    fn providers_have_independent_capacity_and_fifo_queues() {
        let mut q = AiQueue::default();
        q.enqueue(provider_job(1, "notemd.claude-agent"));
        q.enqueue(provider_job(2, "notemd.claude-agent"));
        q.enqueue(provider_job(3, "notemd.deepseek-agent"));
        q.enqueue(provider_job(4, "notemd.deepseek-agent"));
        q.apply_limits(&limits(&[
            ("notemd.claude-agent", 1),
            ("notemd.deepseek-agent", 2),
        ]));

        let claims = q.claim_workers();
        assert_eq!(claims.len(), 3, "one Claude and two DeepSeek slots");
        let claude = claims
            .iter()
            .find(|c| c.provider == "notemd.claude-agent")
            .unwrap();
        let mut deepseek: Vec<_> = claims
            .iter()
            .filter(|c| c.provider == "notemd.deepseek-agent")
            .collect();
        deepseek.sort_by_key(|c| c.worker_id);

        assert_eq!(q.next(&claude.provider, claude.worker_id).unwrap().job_id, 1);
        assert_eq!(q.next(&deepseek[0].provider, deepseek[0].worker_id).unwrap().job_id, 3);
        assert_eq!(q.next(&deepseek[1].provider, deepseek[1].worker_id).unwrap().job_id, 4);
        assert!(q.claim_workers().is_empty(), "Claude's second book must wait");
    }

    #[test]
    fn limits_are_clamped_and_can_grow_while_work_is_pending() {
        let mut q = AiQueue::default();
        for id in 1..=7 {
            q.enqueue(provider_job(id, "notemd.claude-agent"));
        }
        q.apply_limits(&limits(&[("notemd.claude-agent", 0)]));
        assert_eq!(q.claim_workers().len(), 1, "zero clamps to one");

        q.apply_limits(&limits(&[("notemd.claude-agent", 99)]));
        assert_eq!(q.claim_workers().len(), 4, "the hard maximum is five");
        assert_eq!(q.worker_count("notemd.claude-agent"), 5);
    }

    #[test]
    fn lowering_a_limit_retires_excess_workers_without_cancelling_active_jobs() {
        let provider = "notemd.deepseek-agent";
        let mut q = AiQueue::default();
        for id in 1..=5 {
            q.enqueue(provider_job(id, provider));
        }
        q.apply_limits(&limits(&[(provider, 3)]));
        let claims = q.claim_workers();
        for c in &claims {
            assert!(q.next(provider, c.worker_id).is_some());
        }

        q.apply_limits(&limits(&[(provider, 1)]));
        for c in &claims {
            q.finish(provider, c.worker_id);
        }
        assert!(q.next(provider, claims[0].worker_id).is_none());
        q.release_worker(provider, claims[0].worker_id);
        assert!(q.next(provider, claims[1].worker_id).is_none());
        q.release_worker(provider, claims[1].worker_id);
        assert_eq!(
            q.next(provider, claims[2].worker_id).unwrap().job_id,
            4,
            "one existing worker keeps consuming the FIFO"
        );
    }

    #[test]
    fn duplicate_books_are_rejected_across_provider_lanes() {
        let mut q = AiQueue::default();
        q.enqueue(provider_job(1, "notemd.claude-agent"));
        let claim = q.claim_workers().pop().unwrap();
        q.next(&claim.provider, claim.worker_id);

        let same_book = AiJob {
            job_id: 9,
            harness: Some("notemd.deepseek-agent".into()),
            ..job(1)
        };
        assert_eq!(q.enqueue(same_book), Enqueue::Duplicate(1));
    }

    #[test]
    fn releasing_one_worker_only_frees_its_own_active_book() {
        let provider = "notemd.codex-agent";
        let mut q = AiQueue::default();
        q.enqueue(provider_job(1, provider));
        q.enqueue(provider_job(2, provider));
        q.apply_limits(&limits(&[(provider, 2)]));
        let claims = q.claim_workers();
        for c in &claims {
            q.next(provider, c.worker_id);
        }

        q.release_worker(provider, claims[0].worker_id);
        assert_eq!(q.worker_count(provider), 1);
        assert_eq!(
            q.enqueue(AiJob { job_id: 8, ..provider_job(1, provider) }),
            Enqueue::Queued,
            "the dead worker's book is retryable"
        );
        assert_eq!(
            q.enqueue(AiJob { job_id: 9, ..provider_job(2, provider) }),
            Enqueue::Duplicate(2),
            "the other worker's active book stays claimed"
        );
    }

    /// A legacy job with no harness is pinned before dispatch. Jobs explicitly
    /// queued for that provider and default-routed jobs share one FIFO.
    #[test]
    fn unresolved_default_is_pinned_and_merged_in_enqueue_order() {
        let mut q = AiQueue::default();
        assert_eq!(q.enqueue(unresolved_job(1)), Enqueue::Queued);
        assert_eq!(
            q.enqueue(provider_job(2, "notemd.deepseek-agent")),
            Enqueue::Queued
        );
        let claims = q.claim_workers();
        assert_eq!(claims.len(), 1, "only the explicit provider may run");
        assert_eq!(claims[0].provider, "notemd.deepseek-agent");

        q.resolve_default("notemd.deepseek-agent");
        q.apply_limits(&limits(&[("notemd.deepseek-agent", 1)]));
        assert!(q.claim_workers().is_empty());
        let claim = &claims[0];
        assert_eq!(
            q.next(&claim.provider, claim.worker_id)
                .map(|job| (job.job_id, job.harness)),
            Some((1, Some("notemd.deepseek-agent".into())))
        );
        q.finish(&claim.provider, claim.worker_id);
        assert_eq!(
            q.next(&claim.provider, claim.worker_id)
                .map(|job| (job.job_id, job.harness)),
            Some((2, Some("notemd.deepseek-agent".into())))
        );
    }

    #[test]
    fn empty_limit_snapshot_fails_existing_lanes_closed_to_one() {
        let provider = "notemd.claude-agent";
        let mut q = AiQueue::default();
        for id in 1..=7 {
            q.enqueue(provider_job(id, provider));
        }
        q.apply_limits(&limits(&[(provider, 5)]));
        let claims = q.claim_workers();
        assert_eq!(claims.len(), 5);
        for claim in &claims {
            q.next(provider, claim.worker_id).unwrap();
        }

        q.apply_limits(&BTreeMap::new());
        for claim in &claims {
            q.finish(provider, claim.worker_id);
        }
        for claim in &claims[..4] {
            assert!(q.next(provider, claim.worker_id).is_none());
            q.release_worker(provider, claim.worker_id);
        }
        assert_eq!(
            q.next(provider, claims[4].worker_id)
                .map(|job| job.job_id),
            Some(6),
            "only one worker may continue after both settings RPCs fail"
        );
    }

    #[test]
    fn explicitly_selected_agent_survives_the_queue() {
        let mut q = AiQueue::default();
        q.enqueue(provider_job(1, "notemd.deepseek-agent"));
        let claim = q.claim_workers().pop().unwrap();
        assert_eq!(
            q.next(&claim.provider, claim.worker_id)
                .unwrap()
                .harness
                .as_deref(),
            Some("notemd.deepseek-agent")
        );
    }

    #[test]
    fn enqueue_dedups_by_job_id() {
        let mut q = AiQueue::default();
        assert_eq!(q.enqueue(job(1)), Enqueue::Queued);
        assert_eq!(
            q.enqueue(job(1)),
            Enqueue::Duplicate(1),
            "duplicate click must not double-queue"
        );
        assert_eq!(q.enqueue(job(2)), Enqueue::Queued);
    }

    /// The window can reach one book from two places — the import queue's
    /// "AI 先读" and the library's "重读". Both are the same work on the same
    /// `book.md`, writing the same summary file; queueing it twice burns a
    /// second run's tokens for nothing. Identity is the book, not the job id.
    #[test]
    fn one_book_queued_from_two_places_is_read_once() {
        let mut q = AiQueue::default();
        let from_import = AiJob {
            job_id: 1,
            dest_rel: "ssot/ebooks/2026-08/Seven Powers".into(),
            name: "Seven Powers".into(),
            harness: None,
        };
        let from_library = AiJob {
            job_id: 7,
            ..from_import.clone()
        };
        assert_eq!(q.enqueue(from_import), Enqueue::Queued);
        assert_eq!(
            q.enqueue(from_library),
            Enqueue::Duplicate(1),
            "the window binds its row to the job already doing this book"
        );
        assert_eq!(q.pending(), 1);
    }

    /// The book being read right now is off the queue but still in progress —
    /// asking for it again must be refused just the same, or the running read
    /// and a second one would race to write the same summary file.
    #[test]
    fn the_book_currently_being_read_is_not_queued_again() {
        let mut q = AiQueue::default();
        q.enqueue(job(1));
        let claim = q.claim_workers().pop().unwrap();
        assert_eq!(
            q.next(&claim.provider, claim.worker_id)
                .map(|j| j.job_id),
            Some(1)
        );
        let retry = AiJob { job_id: 9, ..job(1) };
        assert_eq!(q.enqueue(retry), Enqueue::Duplicate(1));
    }

    /// …but once that read is over, re-reading the same book is exactly what
    /// the library's "重读" is for. A finished book must not be blocked forever.
    #[test]
    fn a_book_can_be_read_again_after_its_read_finishes() {
        let mut q = AiQueue::default();
        q.enqueue(job(1));
        let claim = q.claim_workers().pop().unwrap();
        q.next(&claim.provider, claim.worker_id);
        q.finish(&claim.provider, claim.worker_id);
        let again = AiJob { job_id: 9, ..job(1) };
        assert_eq!(q.enqueue(again), Enqueue::Queued);
    }

    #[test]
    fn claimed_worker_slots_are_not_claimed_twice() {
        let mut q = AiQueue::default();
        q.enqueue(job(1));
        let claim = q.claim_workers().pop().unwrap();
        assert!(q.claim_workers().is_empty());
        assert_eq!(q.next(&claim.provider, claim.worker_id), Some(job(1)));
        q.finish(&claim.provider, claim.worker_id);
        assert_eq!(q.next(&claim.provider, claim.worker_id), None);
        q.release_worker(&claim.provider, claim.worker_id);
        q.enqueue(job(2));
        assert_eq!(q.claim_workers().len(), 1);
    }

    /// worker panic 后必须能重新拉起,否则「AI 先读」永久失灵。
    #[test]
    fn release_worker_lets_a_new_worker_take_over() {
        let mut q = AiQueue::default();
        q.enqueue(job(1));
        q.enqueue(job(2));
        let claim = q.claim_workers().pop().unwrap();
        assert_eq!(q.next(&claim.provider, claim.worker_id), Some(job(1)));
        q.release_worker(&claim.provider, claim.worker_id);
        assert_eq!(q.pending(), 1);
        let replacement = q.claim_workers().pop().unwrap();
        assert_eq!(
            q.next(&replacement.provider, replacement.worker_id),
            Some(job(2))
        );
    }

    /// A worker that died mid-book leaves nobody reading it. If `active` stayed
    /// set, that book could never be queued again — the one case where the
    /// duplicate guard would lock a user out of retrying.
    #[test]
    fn release_worker_frees_the_book_that_died_with_it() {
        let mut q = AiQueue::default();
        q.enqueue(job(1));
        let claim = q.claim_workers().pop().unwrap();
        q.next(&claim.provider, claim.worker_id);
        q.release_worker(&claim.provider, claim.worker_id);
        let retry = AiJob { job_id: 9, ..job(1) };
        assert_eq!(q.enqueue(retry), Enqueue::Queued);
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
