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
        (En, LocationUnavailable) => "Position Log couldn't get your location",
        (Zh, LocationUnavailable) => "Position Log 无法获取位置",
        (Ja, LocationUnavailable) => "Position Log は位置情報を取得できませんでした",
        (De, LocationUnavailable) => "Position Log konnte deinen Standort nicht ermitteln",

        (En, EmptyGeocode) => "Position Log got no usable address for this location",
        (Zh, EmptyGeocode) => "Position Log 未能解析出有效地址",
        (Ja, EmptyGeocode) => "Position Log は有効な住所を取得できませんでした",
        (De, EmptyGeocode) => "Position Log konnte keine verwertbare Adresse ermitteln",

        (En, VaultRequired) => "Position Log needs a vault configured",
        (Zh, VaultRequired) => "Position Log 需要已配置的 vault",
        (Ja, VaultRequired) => "Position Log には vault の設定が必要です",
        (De, VaultRequired) => "Position Log benötigt einen konfigurierten Tresor",

        (En, SaveFailed) => "Position Log couldn't save your location",
        (Zh, SaveFailed) => "Position Log 保存位置失败",
        (Ja, SaveFailed) => "Position Log は位置情報を保存できませんでした",
        (De, SaveFailed) => "Position Log konnte deinen Standort nicht speichern",
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
    fn recorded_and_unchanged_interpolate_and_translate() {
        for &locale in &LOCALES {
            assert!(recorded(locale, "武汉").contains('武'), "{locale:?} dropped addr");
            assert!(unchanged(locale, "武汉").contains('武'), "{locale:?} dropped addr");
        }
        assert_ne!(recorded(Locale::Zh, "X"), recorded(Locale::En, "X"));
        assert_ne!(unchanged(Locale::Zh, "X"), unchanged(Locale::En, "X"));
    }
}
