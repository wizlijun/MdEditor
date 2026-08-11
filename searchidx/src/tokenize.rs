//! The tokenizer. Index side and query side call the SAME function — that
//! symmetry is the whole contract (design spec §3.2).
//!
//! Why pre-tokenize at all: FTS5's built-in `unicode61` tokenizer treats a run
//! of Han characters as ONE term. Measured: storing "增量索引" makes
//! `MATCH "增量"` miss it entirely. So we do the segmentation ourselves, store
//! the result space-joined, and let unicode61 do nothing but split on spaces.
//! Writing a custom FTS5 tokenizer would mean the C API for no extra benefit.
//!
//! Why `cut_for_search` rather than plain `cut`: it emits the long word AND its
//! sub-words, so "增量索引" indexes as {增量, 索引, 增量索引} and a query for
//! "增量" hits. Recall over precision, deliberately.

use std::sync::OnceLock;

use jieba_rs::Jieba;

/// Identity of the tokenization *algorithm*, stored in the index's `meta` table.
///
/// A mismatch means the stored tokens were produced by different rules than the
/// query would produce, so the index is not a valid pure function of the files
/// any more and gets rebuilt from scratch. Bump this whenever the output of
/// `tokenize` changes for any input — including a jieba upgrade that moves the
/// dictionary. The frozen-fingerprint test in this module exists to make sure
/// nobody forgets.
pub const TOKENIZER_ID: &str = "v1+jieba-rs-0.10+cut_for_search+hmm";

/// The dictionary is deflate-compressed into the binary by jieba-rs's
/// `default-dict` feature and decompressed on first touch (~78 ms measured on a
/// release build). Lazy on purpose: the CLI has a startup budget and a pure
/// ASCII query must not pay the dictionary tax.
static JIEBA: OnceLock<Jieba> = OnceLock::new();

fn jieba() -> &'static Jieba {
    JIEBA.get_or_init(Jieba::new)
}

/// CJK Unified Ideographs (+ extensions and compatibility). Deliberately NOT
/// kana or Hangul: jieba is a Chinese segmenter and would produce noise there.
/// Those scripts fall through to the generic word-run path, which keeps a run
/// as one token — findable by exact term or by the LIKE fallback, which is an
/// honest limitation rather than a fake segmentation.
fn is_han(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// A character that can be part of a non-Han word token.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Segment `text` into index/query terms.
pub fn tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut han = String::new();

    let flush_word = |word: &mut String, out: &mut Vec<String>| {
        if !word.is_empty() {
            out.push(word.to_lowercase());
            word.clear();
        }
    };
    let flush_han = |han: &mut String, out: &mut Vec<String>| {
        if !han.is_empty() {
            for tok in jieba().cut_for_search(han, true) {
                out.push(tok.word.to_string());
            }
            han.clear();
        }
    };

    for c in text.chars() {
        if is_han(c) {
            flush_word(&mut word, &mut out);
            han.push(c);
        } else if is_word(c) {
            flush_han(&mut han, &mut out);
            word.push(c);
        } else {
            flush_word(&mut word, &mut out);
            flush_han(&mut han, &mut out);
        }
    }
    flush_word(&mut word, &mut out);
    flush_han(&mut han, &mut out);
    out
}

/// Space-joined tokens, ready to be stored in an FTS5 column.
pub fn tokenize(text: &str) -> String {
    tokens(text).join(" ")
}

/// Whether the text contains Han ideographs, i.e. whether the jieba path (and
/// therefore the dictionary-blind-spot fallback) is relevant.
pub fn has_han(text: &str) -> bool {
    text.chars().any(is_han)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_runs_are_lowercased_word_tokens() {
        assert_eq!(tokens("Hello World"), vec!["hello", "world"]);
        // 标点是分隔符,不是词元
        assert_eq!(tokens("foo_bar-baz.qux"), vec!["foo_bar", "baz", "qux"]);
        assert_eq!(tokens("v6.808.3"), vec!["v6", "808", "3"]);
    }

    /// spec §3.2 的核心主张:cut_for_search 的重叠输出让「查'增量'命中'增量索引'」
    /// 成立。FTS5 的 unicode61 把整段汉字当一个词元,所以不预分词就必然漏检。
    #[test]
    fn han_runs_go_through_cut_for_search_with_overlap() {
        let t = tokens("增量索引");
        assert!(t.contains(&"增量".to_string()), "{t:?}");
        assert!(t.contains(&"索引".to_string()), "{t:?}");
    }

    #[test]
    fn mixed_cjk_and_ascii_are_both_tokenized() {
        let t = tokens("用 FTS5 做全文检索");
        assert!(t.contains(&"fts5".to_string()), "{t:?}");
        assert!(t.contains(&"全文".to_string()), "{t:?}");
        assert!(t.contains(&"检索".to_string()), "{t:?}");
    }

    #[test]
    fn single_han_char_is_its_own_token() {
        assert_eq!(tokens("我"), vec!["我"]);
    }

    #[test]
    fn tokenize_joins_with_single_spaces_for_fts_storage() {
        assert_eq!(tokenize("Hello 世界"), "hello 世界");
        assert_eq!(tokenize("   "), "");
    }

    #[test]
    fn has_han_detects_only_ideographs() {
        assert!(has_han("检索"));
        assert!(!has_han("search"));
        assert!(!has_han("かな"));      // 假名走通用词元路径,不进 jieba
    }

    /// 分词器指纹:jieba 升级或我们改了切分规则时必须失败,提醒开发者 bump
    /// TOKENIZER_ID —— 那才是让所有用户的索引自动重建的开关。指纹是「金句」而不是
    /// 版本号,因为真正会伤到索引的是**输出漂移**,不是版本字符串。
    #[test]
    fn tokenizer_fingerprint_is_frozen() {
        const PROBE: &str = "增量索引与全文检索 v2 Hello 我";
        assert_eq!(
            tokenize(PROBE),
            "增量 索引 与 全文 检索 全文检索 v2 hello 我",
            "tokenizer output drifted — bump TOKENIZER_ID so existing indexes rebuild"
        );
        assert_eq!(TOKENIZER_ID, "v1+jieba-rs-0.10+cut_for_search+hmm");
    }
}
