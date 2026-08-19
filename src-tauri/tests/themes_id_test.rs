use notemd_lib::themes::id::{is_valid_theme_id, ThemeIdError};

#[test]
fn accepts_simple_lowercase() {
    assert!(is_valid_theme_id("default").is_ok());
    assert!(is_valid_theme_id("effie").is_ok());
}

#[test]
fn accepts_hyphens_underscores_dots_digits() {
    assert!(is_valid_theme_id("claude-like").is_ok());
    assert!(is_valid_theme_id("theme_v2").is_ok());
    assert!(is_valid_theme_id("dracula.dark").is_ok());
    assert!(is_valid_theme_id("a1b2c3").is_ok());
}

#[test]
fn rejects_empty() {
    assert_eq!(is_valid_theme_id(""), Err(ThemeIdError::Empty));
}

#[test]
fn rejects_uppercase() {
    assert_eq!(is_valid_theme_id("Default"), Err(ThemeIdError::BadLeadingChar('D')));
    assert_eq!(is_valid_theme_id("clauDe"), Err(ThemeIdError::InvalidChar('D')));
}

#[test]
fn rejects_leading_punctuation() {
    assert_eq!(is_valid_theme_id("-foo"), Err(ThemeIdError::BadLeadingChar('-')));
    assert_eq!(is_valid_theme_id(".hidden"), Err(ThemeIdError::BadLeadingChar('.')));
    assert_eq!(is_valid_theme_id("_x"), Err(ThemeIdError::BadLeadingChar('_')));
}

#[test]
fn rejects_spaces_and_slashes() {
    assert_eq!(is_valid_theme_id("my theme"), Err(ThemeIdError::InvalidChar(' ')));
    assert_eq!(is_valid_theme_id("a/b"), Err(ThemeIdError::InvalidChar('/')));
    assert_eq!(is_valid_theme_id("..").is_err(), true);
}
