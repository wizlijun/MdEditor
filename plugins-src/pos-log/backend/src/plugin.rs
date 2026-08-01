//! SDK 粘合：activate 起 30 分钟循环 task；每轮经 `host.location.get`（宿主
//! 主线程 CoreLocation，插件裸二进制拿不到定位授权）取位 → logbook 决策 →
//! host.vault.exists/read/write。deactivate 撤销循环。
use crate::logbook;
use crate::strings::{self, Locale};
use notemd_plugin_sdk::{self as sdk, plugin_protocol as proto};
use serde_json::{json, Value};

const ROUND_SECS: u64 = 30 * 60;

pub struct PosLogPlugin {
    stop_tx: Option<tokio::sync::watch::Sender<bool>>,
    /// Set from `$initialize.locale` (host protocol guarantees `$initialize`
    /// precedes `$activate`/`command.execute`, so this is populated before
    /// either spawns a round). `Locale` is `Copy`, so it's captured by value
    /// into each spawned task rather than shared behind a lock.
    locale: Locale,
}

impl PosLogPlugin {
    pub fn new() -> Self {
        Self { stop_tx: None, locale: Locale::En }
    }
}

impl Default for PosLogPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl sdk::NotemdPlugin for PosLogPlugin {
    fn initialize(&mut self, _host: &sdk::Host, params: &proto::InitializeParams) {
        self.locale = Locale::from_code(&params.locale);
    }

    fn activate(&mut self, host: &sdk::Host, _params: &proto::ActivateParams) -> Result<(), String> {
        if self.stop_tx.is_some() {
            return Ok(()); // 幂等：重复 $activate 不叠循环
        }
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        self.stop_tx = Some(stop_tx);
        let host = host.clone();
        let locale = self.locale;
        tokio::spawn(async move {
            let mut warned_once = false;
            loop {
                run_round(&host, locale, &mut warned_once, false).await;
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(ROUND_SECS)) => {}
                    _ = stop_rx.changed() => break,
                }
            }
        });
        Ok(())
    }

    fn deactivate(&mut self, _host: &sdk::Host) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(true);
        }
    }

    fn execute_command(
        &mut self,
        host: &sdk::Host,
        params: &proto::ExecuteCommandParams,
    ) -> Result<Value, String> {
        match params.command.as_str() {
            // Plugins ▸ "Save Location Now" — fire a single fetch+write cycle
            // off the tokio runtime and return immediately (the toast reports
            // the outcome). Reuses the same round the 30-min loop runs.
            "save-now" => {
                let host = host.clone();
                let locale = self.locale;
                tokio::spawn(async move {
                    let mut warned = false;
                    run_round(&host, locale, &mut warned, /*announce=*/ true).await;
                });
                Ok(json!({ "ok": true }))
            }
            other => Err(format!("pos-log: unknown command '{other}'")),
        }
    }
}

/// 一轮：取位（host.location.get）→ 组行 → 决策 → 写盘。所有失败仅告警跳过
/// （spec §8 错误表）。`announce`=true（手动「Save Location Now」）总是弹 toast
/// 反馈结果；=false（30 分钟循环）沿用 `warned_once` 抑制重复告警。`locale`
/// selects the toast wording (captured from `$initialize` — see `PosLogPlugin`).
async fn run_round(host: &sdk::Host, locale: Locale, warned_once: &mut bool, announce: bool) {
    let (place, coords) = match host.request("host.location.get", json!({})).await {
        Ok(v) => {
            let place = logbook::Place {
                country: v["country"].as_str().unwrap_or_default().to_string(),
                province: v["province"].as_str().unwrap_or_default().to_string(),
                city: v["city"].as_str().unwrap_or_default().to_string(),
                poi: v["poi"].as_str().unwrap_or_default().to_string(),
            };
            // 坐标随行末记录；两者都在才拼（非 macOS / 老宿主不返回坐标时省略）。
            let coords = match (v["latitude"].as_f64(), v["longitude"].as_f64()) {
                (Some(lat), Some(lon)) => Some(logbook::format_coords(lat, lon)),
                _ => None,
            };
            (place, coords)
        }
        Err(e) => {
            if announce || !*warned_once {
                host.toast("warning", strings::t(locale, strings::Key::LocationUnavailable), Some(&e));
                *warned_once = true;
            }
            host.log_warn(&format!("pos-log: host.location.get failed: {e}"));
            return;
        }
    };
    let addr = logbook::format_address(&place);
    if addr.is_empty() {
        host.log_warn("pos-log: empty geocode result, skipping round");
        if announce {
            host.toast(
                "warning",
                strings::t(locale, strings::Key::EmptyGeocode),
                Some("empty geocode result"),
            );
        }
        return;
    }
    let now = chrono::Local::now();
    let path = logbook::file_rel_path(&now);
    let line = logbook::format_line(&now, &addr, coords.as_deref());

    let existing: Option<String> = match host
        .request("host.vault.exists", json!({"path": path}))
        .await
    {
        Ok(v) if v["exists"] == true => {
            match host.request("host.vault.read", json!({"path": path})).await {
                Ok(v) => v["content"].as_str().map(str::to_string),
                Err(e) => {
                    host.log_warn(&format!("pos-log: vault.read failed: {e}"));
                    if announce {
                        host.toast("warning", strings::t(locale, strings::Key::SaveFailed), Some(&e));
                    }
                    return;
                }
            }
        }
        Ok(_) => None,
        Err(e) => {
            // vault 未配置等；循环侧首次 toast，手动侧总是 toast
            if announce || !*warned_once {
                host.toast("warning", strings::t(locale, strings::Key::VaultRequired), Some(&e));
                *warned_once = true;
            }
            host.log_warn(&format!("pos-log: vault.exists failed: {e}"));
            return;
        }
    };
    match logbook::decide(existing.as_deref(), &line, &addr) {
        Some(new_content) => {
            if let Err(e) = host
                .request("host.vault.write", json!({"path": path, "content": new_content}))
                .await
            {
                host.log_error(&format!("pos-log: vault.write failed: {e}"));
                if announce {
                    host.toast("warning", strings::t(locale, strings::Key::SaveFailed), Some(&e));
                }
                return;
            }
            if announce {
                host.toast("info", &strings::recorded(locale, &addr), None);
            }
        }
        None => {
            // 地址未变化：循环侧静默，手动侧仍反馈当前位置
            if announce {
                host.toast("info", &strings::unchanged(locale, &addr), None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notemd_plugin_sdk as sdk;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn first_round_writes_baseline_line() {
        // 假宿主管道
        let (host_side, plugin_side) = tokio::io::duplex(64 * 1024);
        let (plug_r, plug_w) = tokio::io::split(plugin_side);
        tokio::spawn(sdk::serve_io(PosLogPlugin::new(), plug_r, plug_w));

        let (host_r, mut host_w) = tokio::io::split(host_side);
        let mut lines = BufReader::new(host_r).lines();

        // $initialize + $activate（参数形状照 plugin_protocol，deny_unknown_fields）
        host_w.write_all(br#"{"jsonrpc":"2.0","id":1,"method":"$initialize","params":{"protocol_version":2,"host_version":"6.720.4","locale":"en","theme":"light","plugin_root":"/tmp/plugin","data_dir":"/tmp/data"}}"#).await.unwrap();
        host_w.write_all(b"\n").await.unwrap();
        host_w
            .write_all(br#"{"jsonrpc":"2.0","id":2,"method":"$activate","params":{"event":"onStartupFinished"}}"#)
            .await
            .unwrap();
        host_w.write_all(b"\n").await.unwrap();

        // 依次应答 host.location.get → host.vault.exists(false) → 期待 host.vault.write
        let mut wrote: Option<serde_json::Value> = None;
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        while wrote.is_none() {
            let line = tokio::time::timeout_at(deadline, lines.next_line())
                .await
                .expect("timed out waiting for vault.write")
                .unwrap()
                .expect("pipe closed");
            let v: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            match v["method"].as_str() {
                Some("host.location.get") => {
                    let id = v["id"].as_u64().unwrap();
                    let resp = format!(
                        r#"{{"jsonrpc":"2.0","id":{id},"result":{{"country":"中国","province":"湖北","city":"武汉","poi":"光谷软件园","latitude":30.50762,"longitude":114.41956}}}}"#
                    );
                    host_w.write_all(resp.as_bytes()).await.unwrap();
                    host_w.write_all(b"\n").await.unwrap();
                }
                Some("host.vault.exists") => {
                    let id = v["id"].as_u64().unwrap();
                    let resp = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"exists":false}}}}"#);
                    host_w.write_all(resp.as_bytes()).await.unwrap();
                    host_w.write_all(b"\n").await.unwrap();
                }
                Some("host.vault.write") => {
                    wrote = Some(v);
                }
                _ => {} // $initialize/$activate 应答、日志通知——忽略
            }
        }
        let w = wrote.unwrap();
        let path = w["params"]["path"].as_str().unwrap();
        let content = w["params"]["content"].as_str().unwrap();
        assert!(path.starts_with("pos/") && path.ends_with("-pos.md"), "path: {path}");
        assert!(content.starts_with("- "), "content: {content}");
        assert!(
            content.trim_end().ends_with("中国-湖北-武汉 光谷软件园 (30.50762, 114.41956)"),
            "content: {content}"
        );
        assert!(content.ends_with('\n'));
        // 回 ok，避免插件侧悬挂
        let id = w["id"].as_u64().unwrap();
        let resp = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"ok":true}}}}"#);
        host_w.write_all(resp.as_bytes()).await.unwrap();
        host_w.write_all(b"\n").await.unwrap();
    }
}
