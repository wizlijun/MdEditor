//! Locale-aware text for pos-log's user-visible toasts.
//!
//! Unlike a v2 plugin with a window (which owns a `src/lib/strings.ts` in its
//! isolated webview), pos-log is backend-only — it has no `ui` in its
//! manifest, so its *only* user-visible surface is `host.toast` calls from
//! `plugin.rs`. Those were hardcoded in Chinese regardless of the user's
//! `notemd.locale`, so an English- or Japanese-locale user would see Chinese
//! toasts. This module mirrors the shape of a frontend `strings.ts` (a key
//! enum + one catalog per locale + a `t()`) so the backend can localize its
//! own text instead.
//!
//! `$initialize`'s `locale` field seeds this (see `PosLogPlugin::initialize`);
//! it is one of `en | zh | ja | de` today (a region-tagged code falls back to
//! its base language, then to English for anything else).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Locale {
    #[default]
    En,
    Zh,
    Ja,
    De,
}

impl Locale {
    /// Parses the `$initialize.locale` string; unknown/absent → English.
    pub fn from_code(code: &str) -> Self {
        match code.split(['-', '_']).next().unwrap_or("") {
            "zh" => Locale::Zh,
            "ja" => Locale::Ja,
            "de" => Locale::De,
            _ => Locale::En,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Key {
    /// `host.location.get` failed.
    LocationUnavailable,
    /// The reverse-geocoded address came back empty.
    EmptyGeocode,
    /// `host.vault.exists`/`.read` failed because no vault is configured.
    VaultRequired,
    /// `host.vault.read`/`.write` failed for some other reason.
    SaveFailed,
}

/// The fixed (non-parameterized) toast message for `key`.
pub fn t(locale: Locale, key: Key) -> &'static str {
    use Key::*;
    use Locale::*;
    match (locale, key) {
        (En, LocationUnavailable) => "Location Log couldn't get your location",
        (Zh, LocationUnavailable) => "位置记录无法获取位置",
        (Ja, LocationUnavailable) => "位置記録は位置情報を取得できませんでした",
        (De, LocationUnavailable) => "Standortprotokoll konnte deinen Standort nicht ermitteln",

        (En, EmptyGeocode) => "Location Log got no usable address for this location",
        (Zh, EmptyGeocode) => "位置记录未能解析出有效地址",
        (Ja, EmptyGeocode) => "位置記録は有効な住所を取得できませんでした",
        (De, EmptyGeocode) => "Standortprotokoll konnte keine verwertbare Adresse ermitteln",

        (En, VaultRequired) => "Location Log needs a vault configured",
        (Zh, VaultRequired) => "位置记录需要已配置的 vault",
        (Ja, VaultRequired) => "位置記録には vault の設定が必要です",
        (De, VaultRequired) => "Standortprotokoll benötigt einen konfigurierten Tresor",

        (En, SaveFailed) => "Location Log couldn't save your location",
        (Zh, SaveFailed) => "位置记录保存位置失败",
        (Ja, SaveFailed) => "位置記録は位置情報を保存できませんでした",
        (De, SaveFailed) => "Standortprotokoll konnte deinen Standort nicht speichern",
    }
}

/// Main toast text for a failed `host.location.get` call. Permission failures
/// must be actionable without expanding the technical detail; other failures
/// retain the existing generic message and keep diagnostics in toast detail.
pub fn location_unavailable(locale: Locale, error: &str) -> &'static str {
    let denied = error.contains("location: denied")
        || error.contains("didFailWithError code=1 ")
        || error.contains("kCLErrorDomain error 1.)");
    if !denied {
        return t(locale, Key::LocationUnavailable);
    }

    match locale {
        Locale::En => "Location Log needs location access. Enable note.md in System Settings → Privacy & Security → Location Services.",
        Locale::Zh => "位置记录需要定位权限，请在「系统设置」→「隐私与安全性」→「定位服务」中开启 note.md",
        Locale::Ja => "位置記録には位置情報へのアクセスが必要です。「システム設定」→「プライバシーとセキュリティ」→「位置情報サービス」で note.md を有効にしてください",
        Locale::De => "Das Standortprotokoll benötigt Standortzugriff. Aktiviere note.md unter Systemeinstellungen → Datenschutz & Sicherheit → Ortungsdienste.",
    }
}

/// "Recorded {addr}" — a new line was appended (announce-only feedback).
pub fn recorded(locale: Locale, addr: &str) -> String {
    match locale {
        Locale::En => format!("Recorded {addr}"),
        Locale::Zh => format!("已记录 {addr}"),
        Locale::Ja => format!("{addr} を記録しました"),
        Locale::De => format!("Erfasst: {addr}"),
    }
}

/// "Location unchanged: {addr}" — the address didn't change this round.
pub fn unchanged(locale: Locale, addr: &str) -> String {
    match locale {
        Locale::En => format!("Location unchanged: {addr}"),
        Locale::Zh => format!("位置未变化：{addr}"),
        Locale::Ja => format!("位置は変わっていません：{addr}"),
        Locale::De => format!("Standort unverändert: {addr}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCALES: [Locale; 4] = [Locale::En, Locale::Zh, Locale::Ja, Locale::De];
    const KEYS: [Key; 4] = [
        Key::LocationUnavailable,
        Key::EmptyGeocode,
        Key::VaultRequired,
        Key::SaveFailed,
    ];

    #[test]
    fn from_code_recognizes_base_languages() {
        assert_eq!(Locale::from_code("en"), Locale::En);
        assert_eq!(Locale::from_code("zh"), Locale::Zh);
        assert_eq!(Locale::from_code("ja"), Locale::Ja);
        assert_eq!(Locale::from_code("de"), Locale::De);
    }

    #[test]
    fn from_code_strips_region_suffix() {
        assert_eq!(Locale::from_code("zh-CN"), Locale::Zh);
        assert_eq!(Locale::from_code("de_DE"), Locale::De);
    }

    #[test]
    fn from_code_falls_back_to_english() {
        assert_eq!(Locale::from_code("fr"), Locale::En);
        assert_eq!(Locale::from_code(""), Locale::En);
    }

    #[test]
    fn every_locale_has_every_key_and_none_are_empty() {
        for &locale in &LOCALES {
            for &key in &KEYS {
                assert!(!t(locale, key).is_empty(), "{locale:?}/{key:?} is empty");
            }
        }
    }

    #[test]
    fn non_english_catalogs_differ_from_english() {
        for &key in &KEYS {
            let en = t(Locale::En, key);
            for &locale in &[Locale::Zh, Locale::Ja, Locale::De] {
                assert_ne!(t(locale, key), en, "{locale:?}/{key:?} left in English");
            }
        }
    }

    #[test]
    fn catalogs_use_the_current_product_name() {
        assert!(t(Locale::En, Key::SaveFailed).starts_with("Location Log"));
        assert!(t(Locale::Zh, Key::SaveFailed).starts_with("位置记录"));
        assert!(t(Locale::Ja, Key::SaveFailed).starts_with("位置記録"));
        assert!(t(Locale::De, Key::SaveFailed).starts_with("Standortprotokoll"));
    }

    #[test]
    fn permission_denied_is_actionable_in_every_locale() {
        let stable = "-32000: location: denied — enable note.md in System Settings";
        let legacy = "-32000: location: didFailWithError code=1 kCLErrorDomain error 1";

        for &locale in &LOCALES {
            let stable_message = location_unavailable(locale, stable);
            let legacy_message = location_unavailable(locale, legacy);
            assert_eq!(stable_message, legacy_message, "{locale:?}");
            assert!(
                stable_message.contains("note.md"),
                "{locale:?}: {stable_message}"
            );
            assert_ne!(stable_message, t(locale, Key::LocationUnavailable));
        }
    }

    #[test]
    fn non_permission_location_errors_keep_the_generic_message() {
        for &locale in &LOCALES {
            assert_eq!(
                location_unavailable(locale, "-32000: location: didFailWithError code=2 network"),
                t(locale, Key::LocationUnavailable)
            );
        }
    }

    #[test]
    fn recorded_and_unchanged_interpolate_and_translate() {
        for &locale in &LOCALES {
            assert!(recorded(locale, "武汉").contains('武'), "{locale:?} dropped addr");
            assert!(unchanged(locale, "武汉").contains('武'), "{locale:?} dropped addr");
        }
        assert_ne!(recorded(Locale::Zh, "X"), recorded(Locale::En, "X"));
        assert_ne!(unchanged(Locale::Zh, "X"), unchanged(Locale::En, "X"));
    }
}
