//! Locale-aware text for md2pdf's user-visible toasts and errors.
//!
//! md2pdf has no window, so its only user-visible surfaces are the `host.toast`
//! call and the error string returned from `execute_command` — both of which
//! were hardcoded (the export toast in English, the v1 renderer's own toasts in
//! Chinese) regardless of the user's `notemd.locale`. This mirrors the shape of
//! a frontend `strings.ts`: a key enum, one catalog per locale, and a `t()`.
//!
//! Seeded from `$initialize.locale` (see `Md2PdfV2::initialize`); a
//! region-tagged code falls back to its base language, anything unknown to
//! English.

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
    /// Export succeeded; `{path}` is where the PDF landed.
    Exported,
    /// The renderer subprocess could not be started or died before responding.
    RendererUnavailable,
    /// The renderer ran but reported failure.
    RenderFailed,
    /// A command other than `export` arrived.
    UnknownCommand,
}

/// Localized text for `key`. `{path}` is the only placeholder; callers
/// substitute it via [`with_path`].
pub fn t(locale: Locale, key: Key) -> &'static str {
    use Key::*;
    use Locale::*;
    match (locale, key) {
        (En, Exported) => "✅ Exported to {path}",
        (Zh, Exported) => "✅ 已导出到 {path}",
        (Ja, Exported) => "✅ {path} に書き出しました",
        (De, Exported) => "✅ Exportiert nach {path}",

        (En, RendererUnavailable) => "❌ Export to PDF: the renderer didn't start",
        (Zh, RendererUnavailable) => "❌ 导出 PDF:渲染进程未能启动",
        (Ja, RendererUnavailable) => "❌ PDF 書き出し:レンダラーを起動できませんでした",
        (De, RendererUnavailable) => "❌ PDF-Export: Der Renderer wurde nicht gestartet",

        (En, RenderFailed) => "❌ Export to PDF: rendering failed",
        (Zh, RenderFailed) => "❌ 导出 PDF:渲染失败",
        (Ja, RenderFailed) => "❌ PDF 書き出し:レンダリングに失敗しました",
        (De, RenderFailed) => "❌ PDF-Export: Rendern fehlgeschlagen",

        (En, UnknownCommand) => "❌ Export to PDF: unknown command",
        (Zh, UnknownCommand) => "❌ 导出 PDF:未知命令",
        (Ja, UnknownCommand) => "❌ PDF 書き出し:不明なコマンドです",
        (De, UnknownCommand) => "❌ PDF-Export: Unbekannter Befehl",
    }
}

/// [`t`] with `{path}` filled in.
pub fn with_path(locale: Locale, key: Key, path: &str) -> String {
    t(locale, key).replace("{path}", path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_base_language_from_a_region_tagged_code() {
        assert_eq!(Locale::from_code("zh-CN"), Locale::Zh);
        assert_eq!(Locale::from_code("ja_JP"), Locale::Ja);
        assert_eq!(Locale::from_code("de"), Locale::De);
    }

    #[test]
    fn falls_back_to_english_for_anything_unknown() {
        assert_eq!(Locale::from_code("fr"), Locale::En);
        assert_eq!(Locale::from_code(""), Locale::En);
    }

    /// The catalog is exhaustive by construction (a `match` over both enums),
    /// so what's worth pinning is that no locale was filled in with the English
    /// text by copy-paste, and that the placeholder survived translation.
    #[test]
    fn every_locale_differs_from_english_and_keeps_the_placeholder() {
        for key in [Key::Exported, Key::RendererUnavailable, Key::RenderFailed, Key::UnknownCommand] {
            let en = t(Locale::En, key);
            for loc in [Locale::Zh, Locale::Ja, Locale::De] {
                let s = t(loc, key);
                assert_ne!(s, en, "{loc:?} left untranslated for {key:?}");
                assert_eq!(
                    s.contains("{path}"),
                    en.contains("{path}"),
                    "{loc:?} changed the placeholders of {key:?}"
                );
            }
        }
    }

    #[test]
    fn with_path_substitutes_and_leaves_other_text_alone() {
        let s = with_path(Locale::Zh, Key::Exported, "/tmp/a b.pdf");
        assert!(s.contains("/tmp/a b.pdf"));
        assert!(!s.contains("{path}"));
    }
}
