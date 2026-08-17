//! Which plugin is "the agent"?
//!
//! It used to be a constant: `notemd.claude-agent`, hard-coded in two places.
//! With a second harness (`notemd.deepseek-agent`) that stops being a fact and
//! becomes a choice, so this module answers three questions — who CAN be the
//! agent, who IS by default, and who should serve THIS call.
//!
//! ## Why the marker is a convention rather than a manifest field
//!
//! The obvious design is `contributes.agent_provider: true`. It cannot be used:
//! `Contributes` is `#[serde(deny_unknown_fields)]`, so a manifest carrying that
//! key fails to parse on any host built before the field existed — and a plugin
//! whose manifest fails to parse does not load AT ALL. Shipping it would brick
//! both agent plugins for everyone who has not updated the host yet.
//!
//! So a provider is recognized by something already in every manifest: it
//! declares the three standard agent commands. That is not a proxy for the
//! capability — it IS the capability. `host.agent.run` maps to `run-task` and
//! `host.agent.status` to `run-status`; a plugin that activates on those and on
//! `run-note` can serve every call the slot makes. One that cannot would fail at
//! the first request whatever a boolean claimed.
use plugin_protocol::ManifestV2;

/// The commands the agent slot dispatches. A provider must activate on all three.
pub const REQUIRED_COMMANDS: [&str; 3] = ["run-task", "run-note", "run-status"];

/// The provider used when the setting is unset. claude-agent, because it is what
/// every existing vault already has — an upgrade must not silently move a user's
/// runs onto a different model.
pub const DEFAULT_PROVIDER: &str = "notemd.claude-agent";

/// The vault setting that overrides it.
pub const SETTING_KEY: &str = "agentDefaultProvider";

/// Can this plugin serve the agent slot?
pub fn is_provider(m: &ManifestV2) -> bool {
    REQUIRED_COMMANDS.iter().all(|c| {
        let want = format!("onCommand:{c}");
        m.activation.events.iter().any(|e| e == &want)
    })
}

/// Every installed provider, sorted, with the default first.
///
/// Sorted so the list the user sees does not reshuffle between launches, and
/// default-first so the one that will actually run is the one they read first.
pub fn providers(manifests: &[ManifestV2]) -> Vec<String> {
    let mut ids: Vec<String> = manifests
        .iter()
        .filter(|m| is_provider(m))
        .map(|m| m.id.clone())
        .collect();
    ids.sort();
    if let Some(i) = ids.iter().position(|id| id == DEFAULT_PROVIDER) {
        ids.swap(0, i);
        ids[1..].sort();
    }
    ids
}

/// Read `agentDefaultProvider` out of the vault's settings file. Absent,
/// unreadable, or blank all mean "unset".
pub fn configured_default(vault: Option<&std::path::Path>) -> Option<String> {
    let p = vault?.join(".notemd/settings.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
    let s = v.get(SETTING_KEY)?.as_str()?.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Which plugin should serve this call.
///
/// * `requested` — an explicit `harness` parameter on `host.agent.*`. A caller
///   that names one gets it, full stop: a plugin asking a SPECIFIC agent to do
///   something must not be silently redirected by a setting it cannot see.
/// * `configured` — the vault setting.
/// * `installed` — provider ids actually present.
///
/// A configured provider that is not installed falls back rather than failing:
/// uninstalling a plugin should not break the agent slot, and the fallback is
/// visible (the run record names the plugin that served it).
pub fn resolve(
    requested: Option<&str>,
    configured: Option<&str>,
    installed: &[String],
) -> String {
    let known = |id: &str| installed.iter().any(|p| p == id);
    if let Some(r) = requested.map(str::trim).filter(|s| !s.is_empty()) {
        return r.to_string();
    }
    if let Some(c) = configured.map(str::trim).filter(|s| !s.is_empty()) {
        if known(c) {
            return c.to_string();
        }
    }
    if known(DEFAULT_PROVIDER) || installed.is_empty() {
        return DEFAULT_PROVIDER.to_string();
    }
    // The default is gone but something else is installed — use it rather than
    // dispatching at a plugin that is not there.
    installed[0].clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_protocol::{Activation, Contributes, Engines, PluginKind};

    fn manifest(id: &str, events: &[&str]) -> ManifestV2 {
        ManifestV2 {
            manifest_version: 2,
            id: id.into(),
            name: id.into(),
            version: "1.0.0".into(),
            kind: PluginKind::Native,
            engines: Engines {
                notemd: ">=6.0.0".into(),
            },
            description: None,
            binary: Default::default(),
            ui: None,
            activation: Activation {
                events: events.iter().map(|e| e.to_string()).collect(),
            },
            contributes: Contributes::default(),
            capabilities: Vec::new(),
            request_timeout_seconds: None,
            idle_shutdown_seconds: None,
            i18n: None,
        }
    }

    fn agent(id: &str) -> ManifestV2 {
        manifest(
            id,
            &[
                "onCommand:open",
                "onCommand:run-task",
                "onCommand:run-note",
                "onCommand:run-status",
            ],
        )
    }

    #[test]
    fn a_plugin_declaring_all_three_commands_is_a_provider() {
        assert!(is_provider(&agent("notemd.claude-agent")));
        assert!(is_provider(&agent("notemd.deepseek-agent")));
    }

    /// Two of three is not enough: the slot would fail at the first call it
    /// cannot dispatch.
    #[test]
    fn a_plugin_missing_any_of_the_three_is_not() {
        assert!(!is_provider(&manifest(
            "notemd.half",
            &["onCommand:run-task", "onCommand:run-status"]
        )));
        assert!(!is_provider(&manifest("notemd.other", &["onCommand:open"])));
        assert!(!is_provider(&manifest("notemd.none", &[])));
    }

    /// The two shipped plugins' real manifests must satisfy the convention, or
    /// the slot silently has no providers.
    #[test]
    fn both_shipped_agent_manifests_are_recognized() {
        for path in [
            "../plugins-src/claude-agent/manifest.v2.json",
            "../plugins-src/deepseek-agent/manifest.v2.json",
        ] {
            let body = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
            let m: ManifestV2 =
                serde_json::from_str(&body).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert!(is_provider(&m), "{path} must be recognized as a provider");
        }
    }

    #[test]
    fn providers_lists_only_agents_with_the_default_first() {
        let ms = vec![
            agent("notemd.deepseek-agent"),
            manifest("notemd.md2pdf", &["onCommand:export"]),
            agent("notemd.claude-agent"),
            agent("notemd.zzz-agent"),
        ];
        assert_eq!(
            providers(&ms),
            vec![
                "notemd.claude-agent",
                "notemd.deepseek-agent",
                "notemd.zzz-agent"
            ]
        );
    }

    #[test]
    fn providers_is_empty_when_no_agent_is_installed() {
        assert!(providers(&[manifest("notemd.md2pdf", &["onCommand:export"])]).is_empty());
    }

    /// Today's behaviour, byte for byte: one claude-agent installed, no setting.
    #[test]
    fn with_only_claude_installed_nothing_changes() {
        let installed = vec!["notemd.claude-agent".to_string()];
        assert_eq!(resolve(None, None, &installed), "notemd.claude-agent");
    }

    #[test]
    fn the_setting_picks_the_provider() {
        let installed = vec![
            "notemd.claude-agent".to_string(),
            "notemd.deepseek-agent".to_string(),
        ];
        assert_eq!(
            resolve(None, Some("notemd.deepseek-agent"), &installed),
            "notemd.deepseek-agent"
        );
    }

    /// A plugin that names a harness must get that harness: it may be asking
    /// for a model's specific strengths, and a setting it cannot see must not
    /// silently redirect it.
    #[test]
    fn an_explicit_request_beats_the_setting() {
        let installed = vec![
            "notemd.claude-agent".to_string(),
            "notemd.deepseek-agent".to_string(),
        ];
        assert_eq!(
            resolve(
                Some("notemd.claude-agent"),
                Some("notemd.deepseek-agent"),
                &installed
            ),
            "notemd.claude-agent"
        );
        // Blank and whitespace are "unset", not a request for a plugin named "".
        assert_eq!(
            resolve(Some("  "), Some("notemd.deepseek-agent"), &installed),
            "notemd.deepseek-agent"
        );
    }

    /// Uninstalling the configured plugin must not wedge the slot.
    #[test]
    fn a_configured_provider_that_is_gone_falls_back() {
        let installed = vec!["notemd.claude-agent".to_string()];
        assert_eq!(
            resolve(None, Some("notemd.deepseek-agent"), &installed),
            "notemd.claude-agent"
        );
    }

    #[test]
    fn with_the_default_uninstalled_another_provider_serves() {
        let installed = vec!["notemd.deepseek-agent".to_string()];
        assert_eq!(resolve(None, None, &installed), "notemd.deepseek-agent");
    }

    /// Nothing installed at all: still name the default, so the error the user
    /// sees is "claude-agent is not installed" rather than an empty id.
    #[test]
    fn with_nothing_installed_the_default_is_still_named() {
        assert_eq!(resolve(None, None, &[]), DEFAULT_PROVIDER);
    }

    #[test]
    fn the_setting_is_read_out_of_the_vault() {
        let v = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(v.path().join(".notemd")).unwrap();
        assert_eq!(configured_default(Some(v.path())), None);

        std::fs::write(
            v.path().join(".notemd/settings.json"),
            r#"{"agentDefaultProvider":"notemd.deepseek-agent"}"#,
        )
        .unwrap();
        assert_eq!(
            configured_default(Some(v.path())).as_deref(),
            Some("notemd.deepseek-agent")
        );
    }

    #[test]
    fn an_unreadable_or_blank_setting_reads_as_unset() {
        let v = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(v.path().join(".notemd")).unwrap();
        for body in [r#"{"agentDefaultProvider":""}"#, r#"{}"#, "{not json"] {
            std::fs::write(v.path().join(".notemd/settings.json"), body).unwrap();
            assert_eq!(configured_default(Some(v.path())), None, "{body}");
        }
        assert_eq!(configured_default(None), None);
    }
}
