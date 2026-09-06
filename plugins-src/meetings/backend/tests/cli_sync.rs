use notemd_meetings::{MigrationMode, MigrationReport};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, SystemTime};

const FIRST: &str = "20260403_173300";
const SECOND: &str = "20260404_173300";
const TRANSCRIPT: &str = "00:00:00  Alice: ready\n";

struct Fixture {
    dir: tempfile::TempDir,
    home: PathBuf,
    source: PathBuf,
    vault: PathBuf,
    data: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let source = home.join(".hemory/vault/test-vault");
        let vault = dir.path().join("vault");
        let data = dir.path().join("data");
        let config = dir.path().join("shared.json");
        fs::create_dir_all(&vault).unwrap();
        // Default discovery must skip a lexically earlier, invalid candidate.
        fs::create_dir_all(home.join(".hemory/vault/000-invalid")).unwrap();
        fs::write(&config, json!({"sotvault": vault}).to_string()).unwrap();
        let fixture = Self {
            dir,
            home,
            source,
            vault,
            data,
            config,
        };
        fixture.meeting("alice", FIRST, "2026-04-03T17:33:00+08:00");
        fixture
    }

    fn meeting(&self, user: &str, id: &str, created_at: &str) -> PathBuf {
        let path = self.source.join(user).join("conversation/202604").join(id);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("meta.json"),
            json!({"created_at": created_at, "title": "Weekly", "source": "mac"}).to_string(),
        )
        .unwrap();
        fs::write(path.join("content.md"), TRANSCRIPT).unwrap();
        fs::write(path.join("summary.md"), "# Summary\r\n").unwrap();
        path
    }

    fn context(&self, flags: Value) -> Value {
        json!({"cli": {"args": {"source": self.source}, "flags": flags}})
    }

    fn execute(&self, command: &str, context: Value) -> Value {
        let mut rpc = Backend::start(self);
        assert_eq!(
            rpc.request(
                "$initialize",
                json!({
                    "protocol_version": 2, "host_version": "6.903.2",
                    "locale": "en", "theme": "light",
                    "plugin_root": self.dir.path(), "data_dir": self.data,
                }),
            )["result"]["ok"],
            true
        );
        let subcommand = match command {
            "import-hemory" | "meetings-import-hemory" => "meetings-import-hemory",
            _ => "meetings-sync",
        };
        assert_eq!(
            rpc.request("$activate", json!({"event": format!("onCli:{subcommand}")}))["result"]
                ["ok"],
            true
        );
        rpc.request(
            "command.execute",
            json!({"command": command, "context": context}),
        )
    }

    fn run(&self, command: &str, context: Value, exit_code: i32) -> MigrationReport {
        let response = self.execute(command, context);
        assert!(response.get("error").is_none(), "{response}");
        let outcome = &response["result"]["__notemd_cli_result"];
        assert_eq!(outcome["exit_code"], exit_code);
        assert!(outcome["message"].as_str().is_some_and(|s| !s.is_empty()));
        serde_json::from_value(outcome["data"].clone()).unwrap()
    }
}

struct Backend {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    vault: PathBuf,
    next_id: u64,
}

impl Backend {
    fn start(fixture: &Fixture) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_notemd-meetings"))
            .current_dir(fixture.dir.path())
            .env("HOME", &fixture.home)
            .env("NOTEMD_SHARED_CONFIG", &fixture.config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send(line.expect("read plugin protocol")).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
            vault: fixture.vault.clone(),
            next_id: 100,
        }
    }

    fn send(&mut self, value: Value) {
        writeln!(self.stdin, "{value}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(10))
                .expect("plugin did not respond within 10 seconds");
            let message: Value = serde_json::from_str(&line).expect("JSON-RPC stdout line");
            if message["method"] == "host.vault.info" {
                self.send(json!({
                    "jsonrpc": "2.0", "id": message["id"],
                    "result": {"root": self.vault},
                }));
            } else if message.get("method").is_none() && message["id"] == id {
                return message;
            } else {
                assert!(
                    message["method"]
                        .as_str()
                        .is_some_and(|m| m.starts_with("host.log.")),
                    "unexpected protocol message: {message}"
                );
            }
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // Also reap the isolated backend if an assertion or protocol timeout fails.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, (SystemTime, Option<Vec<u8>>)> {
    fn visit(
        root: &Path,
        path: &Path,
        state: &mut BTreeMap<PathBuf, (SystemTime, Option<Vec<u8>>)>,
    ) {
        let meta = fs::metadata(path).unwrap();
        state.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            (
                meta.modified().unwrap(),
                meta.is_file().then(|| fs::read(path).unwrap()),
            ),
        );
        if meta.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), state);
            }
        }
    }
    let mut state = BTreeMap::new();
    visit(root, root, &mut state);
    state
}

#[test]
fn default_source_sync_is_incremental_across_backend_restarts() {
    let fixture = Fixture::new();
    let first = fixture.run("sync", json!({"cli": {"args": {}, "flags": {}}}), 0);
    assert_eq!(first.mode, MigrationMode::Incremental);
    assert!(!first.dry_run);
    assert_eq!((first.create, first.committed), (1, 1));
    let target = fixture
        .vault
        .join("ssot/meetings")
        .join(FIRST)
        .join("transcript.md");
    assert_eq!(fs::read_to_string(&target).unwrap(), TRANSCRIPT);

    let before = snapshot(fixture.dir.path());
    std::thread::sleep(Duration::from_millis(20));
    let repeat = fixture.run("sync", json!({}), 0);
    assert_eq!((repeat.skip, repeat.committed), (1, 0));
    assert_eq!(
        snapshot(fixture.dir.path()),
        before,
        "rerun changed bytes or mtimes"
    );

    let source = fixture.source.join("alice/conversation/202604").join(FIRST);
    fs::write(source.join("content.md"), "00:00:00  Alice: updated\n").unwrap();
    fixture.meeting("alice", SECOND, "2026-04-04T17:33:00+08:00");
    let before_preview = snapshot(fixture.dir.path());
    let preview = fixture.run("sync", fixture.context(json!({"dry-run": true})), 0);
    assert_eq!(
        (preview.create, preview.update, preview.committed),
        (1, 1, 0)
    );
    assert_eq!(snapshot(fixture.dir.path()), before_preview);
    let changed = fixture.run("sync", fixture.context(json!({})), 0);
    assert_eq!(
        (changed.create, changed.update, changed.committed),
        (1, 1, 2)
    );
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "00:00:00  Alice: updated\n"
    );

    fs::write(&target, "00:00:00  Alice: local edit\n").unwrap();
    fs::write(
        source.join("content.md"),
        "00:00:00  Alice: upstream edit\n",
    )
    .unwrap();
    let conflicted = fixture.run("sync", fixture.context(json!({})), 4);
    assert_eq!(
        (conflicted.conflict, conflicted.skip, conflicted.committed),
        (1, 1, 0)
    );
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "00:00:00  Alice: local edit\n"
    );

    fs::remove_dir_all(source).unwrap();
    let removed = fixture.run("sync", fixture.context(json!({})), 0);
    assert_eq!(removed.source_missing, 1);
    assert!(target.exists(), "source deletion must preserve the archive");
}

#[test]
fn dry_run_leaves_archive_ledger_binding_and_source_untouched() {
    let fixture = Fixture::new();
    let before = snapshot(fixture.dir.path());
    let report = fixture.run("sync", fixture.context(json!({"dry-run": true})), 0);
    assert!(report.dry_run);
    assert_eq!((report.create, report.committed), (1, 0));
    assert_eq!(snapshot(fixture.dir.path()), before);
    assert!(!fixture.data.exists());
    assert!(!fixture.vault.join(".notemd").exists());
}

#[test]
fn sync_reads_the_vaults_custom_meetings_directory() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.vault.join(".notemd")).unwrap();
    fs::write(
        fixture.vault.join(".notemd/meetings.json"),
        r#"{"meetings_root":"archive/transcripts"}"#,
    )
    .unwrap();
    let report = fixture.run(
        "sync",
        json!({"cli": {"args": {"source": fixture.source.join("alice/conversation")}}}),
        0,
    );
    assert_eq!(
        report.items[0].target_relative_path,
        format!("archive/transcripts/{FIRST}")
    );
    assert!(fixture
        .vault
        .join(format!("archive/transcripts/{FIRST}/transcript.md"))
        .is_file());
    assert!(!fixture.vault.join("ssot/meetings").exists());
}

#[test]
fn legacy_import_commands_preserve_full_mode_and_share_the_checkpoint() {
    let fixture = Fixture::new();
    for (command, committed) in [("import-hemory", 1), ("meetings-import-hemory", 0)] {
        let report = fixture.run(command, fixture.context(json!({"full": true})), 0);
        assert_eq!(report.mode, MigrationMode::Full);
        assert_eq!(report.committed, committed);
    }
    // Even a directly injected legacy flag cannot turn the sync route into full mode.
    let synced = fixture.run("meetings-sync", fixture.context(json!({"full": true})), 0);
    assert_eq!(synced.mode, MigrationMode::Incremental);
    assert_eq!((synced.skip, synced.committed), (1, 0));
}

#[test]
fn sync_requires_user_selection_and_forwards_timezone_for_legacy_timestamps() {
    let fixture = Fixture::new();
    fixture.meeting("alice", FIRST, "2026-04-03 17:33:00");
    fixture.meeting("bob", SECOND, "2026-04-04T17:33:00+08:00");
    let error = fixture.execute("sync", fixture.context(json!({"dry-run": true})));
    assert_eq!(error["error"]["code"], -32000);
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("--user"));

    let before = snapshot(fixture.dir.path());
    let blocked = fixture.run(
        "sync",
        fixture.context(json!({"user": "alice", "dry-run": true})),
        4,
    );
    assert_eq!((blocked.blocked, blocked.committed), (1, 0));
    assert!(blocked.items[0].reason.contains("needs_timezone"));
    assert_eq!(snapshot(fixture.dir.path()), before);

    let report = fixture.run(
        "sync",
        fixture.context(json!({"user": "alice", "timezone": "Asia/Taipei"})),
        0,
    );
    assert_eq!(report.source_user, "alice");
    assert_eq!((report.scanned, report.create, report.committed), (1, 1, 1));
}
