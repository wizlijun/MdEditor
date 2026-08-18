//! Harness-neutral run machinery shared by every notemd agent plugin.
//!
//! A "run" means the same thing whichever agent performs it: one task template,
//! one lock, one record on disk, whatever markdown it left behind. That shape is
//! defined here exactly once, so `notemd.claude-agent` and
//! `notemd.deepseek-agent` cannot drift apart on it — the host's agent slot and
//! the plugin windows read one on-disk format regardless of who wrote it.
//!
//! Three things deliberately stay OUT of this crate, because they are precisely
//! what differs between harnesses:
//!
//! 1. **Executable discovery specifics** — the candidate list and binary name
//!    (the mechanism is here, the answers are not).
//! 2. **The transport** — `claude -p --output-format stream-json` versus ACP
//!    NDJSON JSON-RPC over stdio.
//! 3. **Event mapping** — turning that transport's output into [`event::Event`].
//!
//! Everything else — locks, records, progress, artifacts, OKF stamping, task
//! templates, prechecks, mirror resolution, the detach handoff — is shared.

pub mod artifacts;
pub mod detach;
pub mod discover;
pub mod event;
pub mod harness;
pub mod lock;
pub mod mirror;
pub mod okf;
pub mod precheck;
pub mod prompt;
pub mod record;
pub mod scaffold;
pub mod scope;
pub mod task;

pub use event::{Event, RunResult, Step};
pub use harness::HarnessStatus;
pub use record::{RunRecord, Status};
pub use scope::Scope;
pub use task::TaskDef;
