//! `contributes.tray` → menu-bar tray items (and the global hotkeys that stand
//! in for clicking them).
//!
//! Everything here is pure: it turns the installed manifests into a resolved,
//! ordered list of launch items. The `AppHandle`-backed side — building the
//! actual `MenuItem`s, registering the hotkeys, opening the window — lives in
//! `lib.rs` (`build_tray_menu`) because that is where the tray is assembled.

use plugin_protocol::{ManifestV2, TRAY_SECTION_CAPTURE};

/// One resolved tray launch item: a plugin window, a label to show for it, and
/// optionally a hotkey that opens it without going through the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayEntry {
    pub plugin_id: String,
    pub window: String,
    /// Already localized (see [`plugin_display_name`]).
    pub label: String,
    /// Verbatim from the manifest; parsing/registration is the host's job.
    pub accelerator: Option<String>,
}

impl TrayEntry {
    /// Tray menu-item id. `plugin_id` may contain dots but neither part
    /// contains a colon, so `rsplit_once(':')` decodes it unambiguously — the
    /// tray's click handler relies on that.
    pub fn menu_id(&self) -> String {
        format!("tray-plugin:{}:{}", self.plugin_id, self.window)
    }
}

/// The label for a plugin's tray item when it doesn't set one: the localized
/// plugin name from the manifest's `i18n.<locale>.name`, else the manifest
/// `name`.
pub fn plugin_display_name(m: &ManifestV2, locale: &str) -> String {
    m.i18n
        .as_ref()
        .and_then(|v| v.get(locale))
        .and_then(|l| l.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| m.name.clone())
}

/// Every tray contribution across `plugins`, split into
/// `(capture group, default group)` and each sorted by its stable base label.
///
/// The capture group is the tray's top block — the one holding Quick Note and
/// Daily Notes — which a plugin opts into with `"section": "capture"`. Any
/// other value, including one this build has never heard of, falls into the
/// default group: `section` is an open vocabulary (see `TrayContribution`), so
/// an unknown group name has to degrade to a usable position rather than drop
/// the item.
///
/// The sort key is the explicit manifest label, or the base English product
/// name when the label is omitted. The displayed label remains localized. This
/// keeps related actions in the same order across locales (for example Idea
/// Spark before Next) instead of reshuffling the menu when the language changes.
pub fn collect_entries<'a, I>(plugins: I, locale: &str) -> (Vec<TrayEntry>, Vec<TrayEntry>)
where
    I: IntoIterator<Item = (&'a str, &'a ManifestV2)>,
{
    let (mut capture, mut rest) = (Vec::new(), Vec::new());
    for (id, manifest) in plugins {
        for tc in &manifest.contributes.tray {
            let sort_key = tc.label.as_deref().unwrap_or(&manifest.name).to_lowercase();
            let entry = TrayEntry {
                plugin_id: id.to_string(),
                window: tc.window.clone(),
                label: tc
                    .label
                    .clone()
                    .unwrap_or_else(|| plugin_display_name(manifest, locale)),
                accelerator: tc.accelerator.clone(),
            };
            if tc.section.as_deref() == Some(TRAY_SECTION_CAPTURE) {
                capture.push((sort_key, entry));
            } else {
                rest.push((sort_key, entry));
            }
        }
    }
    let sort_entries = |entries: &mut Vec<(String, TrayEntry)>| {
        entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.label.cmp(&b.1.label)));
    };
    sort_entries(&mut capture);
    sort_entries(&mut rest);
    (
        capture.into_iter().map(|(_, entry)| entry).collect(),
        rest.into_iter().map(|(_, entry)| entry).collect(),
    )
}

/// [`collect_entries`] over the live runtime state. A poisoned lock yields no
/// items rather than panicking the tray build.
pub fn entries_from_state(locale: &str) -> (Vec<TrayEntry>, Vec<TrayEntry>) {
    match super::STATE.read() {
        Ok(st) => collect_entries(
            st.plugins.iter().map(|(id, (m, _dir))| (id.as_str(), m)),
            locale,
        ),
        Err(_) => (Vec::new(), Vec::new()),
    }
}

/// The `payload.type` the host pushes when a tray item (or its hotkey) is used
/// on an already-open window. Part of the public plugin contract — see
/// `docs/plugin-v2-development.md` §3 and idea-spark's `App.svelte`, which
/// matches this exact string.
pub const TRAY_ACTIVATE: &str = "tray-activate";

/// The push body for [`TRAY_ACTIVATE`]. A function rather than an inline
/// `json!` at the call site so the wire shape has one authoring point and a
/// test can hold it still.
pub fn activate_payload() -> serde_json::Value {
    serde_json::json!({ "type": TRAY_ACTIVATE })
}

/// Every plugin-declared global accelerator, in a stable order. The label —
/// hence the locale — is irrelevant to hotkey registration, so callers get the
/// combo plus the target it opens.
pub fn accelerators_from_state() -> Vec<(String, TrayEntry)> {
    let (capture, rest) = entries_from_state("en");
    capture
        .into_iter()
        .chain(rest)
        .filter_map(|e| e.accelerator.clone().map(|a| (a, e)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A minimal ui-only manifest with the given tray block.
    fn manifest(name: &str, tray: serde_json::Value, i18n: Option<serde_json::Value>) -> ManifestV2 {
        let mut v = json!({
            "manifest_version": 2,
            "id": "notemd.sample",
            "name": name,
            "version": "1.0.0",
            "kind": "native",
            "engines": { "notemd": ">=6.0.0" },
            "ui": "ui/",
            "activation": { "events": ["onCommand:open"] },
            "contributes": {
                "windows": [{ "id": "main", "entry": "index.html", "width": 700.0, "height": 600.0 }],
                "tray": tray,
            },
            "capabilities": [],
        });
        if let Some(i) = i18n {
            v["i18n"] = i;
        }
        serde_json::from_value(v).expect("sample manifest should deserialize")
    }

    #[test]
    fn capture_section_is_split_out_from_the_default_group() {
        let spark = manifest("Idea Spark", json!([{ "window": "main", "section": "capture" }]), None);
        let chat = manifest("OpenClaw Chat", json!([{ "window": "main" }]), None);
        let (capture, rest) =
            collect_entries([("notemd.idea-spark", &spark), ("notemd.openclaw-chat", &chat)], "en");
        assert_eq!(capture.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), ["Idea Spark"]);
        assert_eq!(rest.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), ["OpenClaw Chat"]);
    }

    /// An open vocabulary has to degrade, not drop: a `section` from a newer
    /// spec must still produce a clickable item, just in the default group.
    #[test]
    fn an_unknown_section_falls_back_to_the_default_group() {
        let m = manifest("Future", json!([{ "window": "main", "section": "sync" }]), None);
        let (capture, rest) = collect_entries([("notemd.future", &m)], "en");
        assert!(capture.is_empty());
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn each_group_is_sorted_by_base_label_independently() {
        let z = manifest("Zebra", json!([{ "window": "main", "section": "capture" }]), None);
        let a = manifest("Apple", json!([{ "window": "main", "section": "capture" }]), None);
        let y = manifest("Yak", json!([{ "window": "main" }]), None);
        let b = manifest("Bee", json!([{ "window": "main" }]), None);
        // Fed in deliberately unsorted order.
        let (capture, rest) = collect_entries(
            [("p.z", &z), ("p.y", &y), ("p.a", &a), ("p.b", &b)],
            "en",
        );
        assert_eq!(capture.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), ["Apple", "Zebra"]);
        assert_eq!(rest.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), ["Bee", "Yak"]);
    }

    #[test]
    fn label_falls_back_to_the_localized_plugin_name() {
        let i18n = json!({ "zh": { "name": "奇思妙想" } });
        let m = manifest("Idea Spark", json!([{ "window": "main" }]), Some(i18n));
        let (_, rest) = collect_entries([("notemd.idea-spark", &m)], "zh");
        assert_eq!(rest[0].label, "奇思妙想");
        // An explicit label wins over the localized name.
        let m2 = manifest(
            "Idea Spark",
            json!([{ "window": "main", "label": "Spark" }]),
            Some(json!({ "zh": { "name": "奇思妙想" } })),
        );
        let (_, rest2) = collect_entries([("notemd.idea-spark", &m2)], "zh");
        assert_eq!(rest2[0].label, "Spark");
    }

    #[test]
    fn localized_labels_keep_the_base_product_order() {
        let spark = manifest(
            "Idea Spark",
            json!([{ "window": "main", "section": "capture" }]),
            Some(json!({ "zh": { "name": "奇思妙想" } })),
        );
        let next = manifest(
            "Next",
            json!([{ "window": "main", "section": "capture" }]),
            Some(json!({ "zh": { "name": "下一步" } })),
        );

        let (capture, _) = collect_entries(
            [("notemd.next", &next), ("notemd.idea-spark", &spark)],
            "zh",
        );
        assert_eq!(
            capture.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
            ["奇思妙想", "下一步"],
        );
    }

    #[test]
    fn accelerator_is_carried_through_verbatim() {
        let m = manifest(
            "Idea Spark",
            json!([{ "window": "main", "section": "capture", "accelerator": "Cmd+Ctrl+I" }]),
            None,
        );
        let (capture, _) = collect_entries([("notemd.idea-spark", &m)], "en");
        assert_eq!(capture[0].accelerator.as_deref(), Some("Cmd+Ctrl+I"));
        // No accelerator declared → None, not an empty string.
        let plain = manifest("Plain", json!([{ "window": "main" }]), None);
        let (_, rest) = collect_entries([("p.plain", &plain)], "en");
        assert_eq!(rest[0].accelerator, None);
    }

    #[test]
    fn menu_id_round_trips_through_the_click_handlers_decoder() {
        let e = TrayEntry {
            plugin_id: "notemd.idea-spark".into(),
            window: "main".into(),
            label: "Idea Spark".into(),
            accelerator: None,
        };
        let id = e.menu_id();
        assert_eq!(id, "tray-plugin:notemd.idea-spark:main");
        // Exactly what lib.rs's tray on_menu_event does with it.
        let (plugin_id, window) = id["tray-plugin:".len()..].rsplit_once(':').unwrap();
        assert_eq!(plugin_id, "notemd.idea-spark");
        assert_eq!(window, "main");
    }

    /// The plugin side matches this literal by hand (isolated webview, no
    /// shared types), so changing it here silently breaks every plugin that
    /// implements "activate = start a new one". Pinned on purpose.
    #[test]
    fn the_activate_push_keeps_its_wire_shape() {
        assert_eq!(activate_payload(), json!({ "type": "tray-activate" }));
    }

    #[test]
    fn a_plugin_may_contribute_several_tray_items() {
        let m = manifest(
            "Multi",
            json!([
                { "window": "main", "label": "B", "section": "capture" },
                { "window": "main", "label": "A" },
            ]),
            None,
        );
        let (capture, rest) = collect_entries([("p.multi", &m)], "en");
        assert_eq!(capture.len(), 1);
        assert_eq!(rest.len(), 1);
        assert_eq!(capture[0].label, "B");
        assert_eq!(rest[0].label, "A");
    }
}
