use notemd_lib::themes::appearance::{resolve_appearance, title_case_from_stem, Appearance};

#[test]
fn explicit_header_wins() {
    assert_eq!(resolve_appearance(Some("dark"), "anything-light"), Appearance::Dark);
    assert_eq!(resolve_appearance(Some("LIGHT"), "anything-dark"), Appearance::Light);
}

#[test]
fn unknown_header_falls_through_to_stem() {
    assert_eq!(resolve_appearance(Some("amber"), "warm-dark"), Appearance::Dark);
    assert_eq!(resolve_appearance(Some(""), "default"), Appearance::Light);
}

#[test]
fn stem_dark_keyword_at_end() {
    assert_eq!(resolve_appearance(None, "claude-like-dark"), Appearance::Dark);
    assert_eq!(resolve_appearance(None, "claude_dark"), Appearance::Dark);
}

#[test]
fn stem_dark_keyword_at_start() {
    assert_eq!(resolve_appearance(None, "dark-claude"), Appearance::Dark);
    assert_eq!(resolve_appearance(None, "night-mode"), Appearance::Dark);
}

#[test]
fn stem_dark_keyword_in_middle() {
    assert_eq!(resolve_appearance(None, "claude-dark-pro"), Appearance::Dark);
    assert_eq!(resolve_appearance(None, "a_night_b"), Appearance::Dark);
}

#[test]
fn substring_does_not_match() {
    assert_eq!(resolve_appearance(None, "darkroom"), Appearance::Light);
    assert_eq!(resolve_appearance(None, "midnighter"), Appearance::Light);
}

#[test]
fn unrelated_stems_are_light() {
    assert_eq!(resolve_appearance(None, "default"), Appearance::Light);
    assert_eq!(resolve_appearance(None, "claude-like"), Appearance::Light);
    assert_eq!(resolve_appearance(None, "claude-like-grey"), Appearance::Light);
}

#[test]
fn title_case_basic() {
    assert_eq!(title_case_from_stem("default"), "Default");
    assert_eq!(title_case_from_stem("claude-like"), "Claude-Like");
    assert_eq!(title_case_from_stem("claude-like-dark"), "Claude-Like Dark");
}

#[test]
fn title_case_underscores_dots() {
    assert_eq!(title_case_from_stem("theme_v2"), "Theme V2");
    assert_eq!(title_case_from_stem("dracula.dark"), "Dracula Dark");
}

#[test]
fn title_case_handles_non_ascii_before_keyword() {
    // The previous byte-indexed implementation produced "Café- ark" for this input.
    assert_eq!(title_case_from_stem("café-dark"), "Café Dark");
    assert_eq!(title_case_from_stem("á-dark"), "Á Dark");
}

#[test]
fn title_case_handles_non_ascii_in_general() {
    // CJK / accented chars in stems should pass through cleanly.
    assert_eq!(title_case_from_stem("书苑-dark"), "书苑 Dark");
    assert_eq!(title_case_from_stem("naïve"), "Naïve");
}
