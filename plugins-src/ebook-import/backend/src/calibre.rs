//! Detects a local Calibre install (specifically its `ebook-convert` CLI)
//! and shells out to it to produce HTMLZ, the format the rest of the
//! pipeline parses. Calibre itself is never vendored — this module only
//! locates whatever the user already has installed.

#[cfg(test)]
mod tests;

use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One located `ebook-convert` binary: its path plus the (trimmed) first
/// line of `--version` output. The version string is shown to the user /
/// logged for support, never parsed further — Calibre's version format
/// isn't a contract we depend on.
#[derive(Debug, Clone, PartialEq)]
pub struct Detected {
    pub path: String,
    pub version: String,
}

/// Probe timeout for `--version` calls. Generous relative to how fast a
/// real `ebook-convert --version` returns, but short enough that a wedged
/// candidate (e.g. a broken shim) doesn't stall the whole probe chain for
/// long.
const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Ceiling for an actual conversion. Calibre can be slow on large / image
/// heavy books, so this is far longer than the version probe.
const CONVERT_TIMEOUT: Duration = Duration::from_secs(600);

/// Detects an installed `ebook-convert`, in priority order: an explicit
/// per-device override, then a hint left by other laobu tools in the
/// shared config, then well-known install locations, then `PATH`. Thin
/// shell over [`detect_with_candidates`] so the actual probing logic is
/// testable without touching the real filesystem/PATH.
pub fn detect(device_override: Option<&str>) -> Option<Detected> {
    detect_with_candidates(&candidates(device_override), DEFAULT_PROBE_TIMEOUT)
}

/// Converts `input` to HTMLZ by invoking `<calibre> <input> <out_htmlz>`.
/// Calibre's own conversion progress/warnings go to stderr; on failure we
/// fold a tail excerpt of that (or stdout, if stderr was empty) into the
/// returned error so callers can show something actionable instead of a
/// bare "conversion failed".
pub fn convert_to_htmlz(calibre: &str, input: &Path, out_htmlz: &Path) -> Result<(), String> {
    let mut cmd = Command::new(calibre);
    cmd.arg(input).arg(out_htmlz);
    match run_with_timeout(cmd, CONVERT_TIMEOUT) {
        Ok(RunOutcome::Exited { success: true, .. }) => Ok(()),
        Ok(RunOutcome::Exited { success: false, code, stdout, stderr }) => {
            let excerpt = tail_excerpt(if stderr.trim().is_empty() { &stdout } else { &stderr }, 2000);
            let code_desc = code.map(|c| c.to_string()).unwrap_or_else(|| "signal".to_string());
            Err(format!("ebook-convert exited with status {code_desc}: {excerpt}"))
        }
        Ok(RunOutcome::TimedOut) => {
            Err(format!("ebook-convert timed out after {}s", CONVERT_TIMEOUT.as_secs()))
        }
        Err(e) => Err(format!("failed to launch ebook-convert: {e}")),
    }
}

/// Probes each candidate with `--version` in order, returning the first
/// one that runs successfully within `timeout`. Nonexistent candidates are
/// skipped without spawning a process — most well-known/PATH candidates
/// won't exist on any given machine, and spawning-then-failing for each
/// would be needlessly slow (and noisy under a debugger/strace).
fn detect_with_candidates(candidates: &[PathBuf], timeout: Duration) -> Option<Detected> {
    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }
        let mut cmd = Command::new(candidate);
        cmd.arg("--version");
        if let Ok(RunOutcome::Exited { success: true, stdout, .. }) = run_with_timeout(cmd, timeout)
        {
            let version = stdout.lines().next().unwrap_or("").trim().to_string();
            return Some(Detected {
                path: candidate.to_string_lossy().into_owned(),
                version,
            });
        }
    }
    None
}

/// Assembles the ordered candidate list `detect` probes. Order matters:
/// an explicit override is authoritative, then the shared cross-plugin
/// config (other laobu tools may have already located Calibre), then
/// Calibre.app's fixed install path, then common Homebrew/system prefixes,
/// then whatever `PATH` turns up.
fn candidates(device_override: Option<&str>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = device_override {
        out.push(PathBuf::from(p));
    }
    if let Some(p) = shared_config_candidate() {
        out.push(p);
    }
    out.push(PathBuf::from(
        "/Applications/calibre.app/Contents/MacOS/ebook-convert",
    ));
    for base in ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin"] {
        out.push(PathBuf::from(base).join("ebook-convert"));
    }
    out.extend(path_env_candidates());
    out
}

/// Path to the shared config other laobu tools (and the user, by hand)
/// may have already pointed at a Calibre install. `None` if `HOME` isn't
/// set — in that case we simply have no hint from this source.
fn shared_config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library/Application Support/com.laobu.mdeditor-shared/config.json"),
    )
}

/// Reads `calibre_path` out of the shared config, tolerantly: a missing
/// file, unreadable file, malformed JSON, or missing/non-string key all
/// just mean "no hint here" rather than an error — this is a best-effort
/// nicety, not a required config.
fn shared_config_candidate() -> Option<PathBuf> {
    let path = shared_config_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    let raw = json.get("calibre_path")?.as_str()?;
    Some(resolve_calibre_path_value(raw))
}

/// Calibre.app's own preferences (and thus what other tools may copy into
/// the shared config) sometimes store the install *directory*
/// (`.../calibre.app/Contents/MacOS`) rather than the `ebook-convert`
/// binary itself. If `raw` names a directory, append the binary name;
/// otherwise use it as-is.
fn resolve_calibre_path_value(raw: &str) -> PathBuf {
    let p = PathBuf::from(raw);
    if p.is_dir() {
        p.join("ebook-convert")
    } else {
        p
    }
}

/// Every directory on `PATH`, with `ebook-convert` appended. We resolve
/// these ourselves (rather than letting the OS search `PATH` when we spawn
/// a bare `"ebook-convert"`) so [`detect_with_candidates`]'s "skip
/// nonexistent candidates without spawning" fast path applies here too.
fn path_env_candidates() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join("ebook-convert"))
                .collect()
        })
        .unwrap_or_default()
}

/// Outcome of running a child process under [`run_with_timeout`].
enum RunOutcome {
    Exited {
        success: bool,
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    TimedOut,
}

/// Runs `cmd` to completion or until `timeout` elapses, whichever comes
/// first. There's no external crate for process timeouts in this
/// workspace, so this is hand-rolled: spawn, then poll `try_wait` every
/// 100ms (fine-grained enough for a 10s probe / 600s conversion budget
/// without busy-looping) until the child exits or the deadline passes, at
/// which point we kill + `wait` to avoid leaving a zombie.
///
/// stdout/stderr are drained *concurrently* on their own threads while the
/// main loop polls `try_wait`, not read after the child exits. A pipe's OS
/// buffer is small (~64KB on macOS/Linux) — a child that writes more than
/// that before exiting (a noisy Calibre conversion dumping progress/
/// warnings to stderr, say) would block on the full pipe forever while this
/// function sat in `try_wait`/`sleep` waiting for an exit that can now never
/// come, deadlocking until `timeout` (up to 600s for a real conversion). The
/// drain threads are joined after the child exits or is killed, so their
/// buffers are complete and this function never returns before both have
/// finished.
///
/// The child is spawned into its own process group (`process_group(0)`) so
/// a timeout kill can `kill(-pgid, SIGKILL)` the whole group rather than
/// just the immediate child: a wedged `cmd` that forked its own children
/// (e.g. a shell script whose command isn't the last/only one, so the
/// shell doesn't exec-replace itself) would otherwise leave those
/// descendants running after `child.kill()`, still holding the stdout/
/// stderr pipes open and blocking the drain threads above until *they*
/// exit on their own instead of promptly here.
fn run_with_timeout(mut cmd: Command, timeout: Duration) -> std::io::Result<RunOutcome> {
    use std::os::unix::process::CommandExt;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = cmd.spawn()?;
    let pid = child.id() as i32;
    let start = Instant::now();

    let stdout_thread = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = out.read_to_string(&mut buf);
            buf
        })
    });
    let stderr_thread = child.stderr.take().map(|mut err| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = err.read_to_string(&mut buf);
            buf
        })
    });
    let join_drain = |t: Option<std::thread::JoinHandle<String>>| {
        t.and_then(|h| h.join().ok()).unwrap_or_default()
    };

    loop {
        if let Some(status) = child.try_wait()? {
            let stdout = join_drain(stdout_thread);
            let stderr = join_drain(stderr_thread);
            return Ok(RunOutcome::Exited {
                success: status.success(),
                code: status.code(),
                stdout,
                stderr,
            });
        }
        if start.elapsed() >= timeout {
            // SIGKILL the whole process group (negative pid), not just the
            // immediate child -- see the doc comment above.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            let _ = child.wait();
            // The group kill closes every descendant's copy of the pipes,
            // so the drain threads' blocking reads finish promptly too —
            // join them so no thread outlives this function, even though a
            // timed-out run discards the output.
            join_drain(stdout_thread);
            join_drain(stderr_thread);
            return Ok(RunOutcome::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Last `max_chars` characters of `text`, trimmed. Splits on `char`
/// boundaries (not bytes) so this can't panic on multi-byte UTF-8 in
/// Calibre's (potentially non-ASCII) output.
fn tail_excerpt(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start = chars.len().saturating_sub(max_chars);
    let excerpt: String = chars[start..].iter().collect();
    let trimmed = excerpt.trim();
    if trimmed.is_empty() {
        "(no output)".to_string()
    } else {
        trimmed.to_string()
    }
}
