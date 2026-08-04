use std::path::Path;

/// Book metadata as extracted from the source file (EPUB/PDF/etc). All
/// fields are optional because extraction is best-effort and downstream
/// consumers (config.txt) must degrade gracefully to "field omitted".
#[derive(Debug, Default, Clone)]
pub struct BookMeta {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
}

const FORBIDDEN_CHARS: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
const MAX_DIRNAME_CHARS: usize = 200;

/// Ports the directory-name sanitizer from the original `bookread.sh` shell
/// pipeline verbatim, rule order included, so directory names produced by
/// this Rust port are byte-identical to the shell version for the same
/// input (needed so existing ebook libraries organized by the old pipeline
/// keep matching directory names).
pub fn sanitize_dirname(input: &str) -> String {
    let replaced: String = input
        .chars()
        .filter(|c| !matches!(*c, '\u{0}'..='\u{1f}'))
        .map(|c| if FORBIDDEN_CHARS.contains(&c) { '_' } else { c })
        .collect();

    let collapsed = {
        let mut out = String::with_capacity(replaced.len());
        let mut prev_space = false;
        for c in replaced.chars() {
            if c.is_whitespace() {
                if !prev_space {
                    out.push(' ');
                }
                prev_space = true;
            } else {
                out.push(c);
                prev_space = false;
            }
        }
        out
    };

    let trimmed = collapsed.trim_matches(|c: char| c == ' ' || c == '.');

    trimmed.chars().take(MAX_DIRNAME_CHARS).collect()
}

/// OKF v0.2 概念文档头(docs/okf-v0.2-format-constraints.md):`type` 是唯一
/// 必填字段(§4.1),来源书文件按 §5.1 记进 `sources[].resource`。元数据缺失时
/// 只降级为 type + sources —— 缺可选字段绝不影响合规(§11)。
pub fn book_frontmatter(input_file: &str, meta: &BookMeta) -> String {
    let mut out = String::from("---\ntype: Book\n");
    if let Some(v) = &meta.title {
        out.push_str(&format!("title: {}\n", yaml_quote(v)));
    }
    if let Some(v) = &meta.publisher {
        out.push_str(&format!("publisher: {}\n", yaml_quote(v)));
    }
    if let Some(v) = &meta.language {
        out.push_str(&format!("language: {}\n", yaml_quote(v)));
    }
    out.push_str("sources:\n");
    out.push_str(&format!("  - resource: {}\n", yaml_quote(input_file)));
    if let Some(v) = &meta.creator {
        out.push_str(&format!("    author: {}\n", yaml_quote(v)));
    }
    out.push_str("---\n");
    out
}

/// YAML 双引号标量:书名里的冒号/引号/反斜杠都必须转义,否则整份 frontmatter
/// 不可解析(违反 §11 条件 1)。
fn yaml_quote(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Writes config.txt in the exact key=value layout the downstream
/// translation pipeline (ported from the original python script) parses.
/// The `# Book Metadata` block and each key within it are emitted only
/// when there is data, so a book with no discoverable metadata produces a
/// config.txt with just the transfer/conversion header.
pub fn write_config_txt(
    path: &Path,
    input_file: &str,
    method: &str,
    meta: &BookMeta,
) -> std::io::Result<()> {
    let mut out = String::new();
    out.push_str("# Translation Configuration\n");
    out.push_str(&format!("input_file={input_file}\n"));
    out.push_str("input_lang=auto\n");
    out.push_str("output_lang=zh\n");
    out.push_str(&format!("conversion_method={method}\n"));

    if meta.title.is_some()
        || meta.creator.is_some()
        || meta.publisher.is_some()
        || meta.language.is_some()
    {
        out.push('\n');
        out.push_str("# Book Metadata\n");
        if let Some(v) = &meta.title {
            out.push_str(&format!("original_title={v}\n"));
        }
        if let Some(v) = &meta.creator {
            out.push_str(&format!("creator={v}\n"));
        }
        if let Some(v) = &meta.publisher {
            out.push_str(&format!("publisher={v}\n"));
        }
        if let Some(v) = &meta.language {
            out.push_str(&format!("source_language={v}\n"));
        }
    }

    std::fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_ports_shell_rules() {
        assert_eq!(sanitize_dirname("a/b\\c:d*e?f\"g<h>i|j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize_dirname("  many   spaces  "), "many spaces");
        assert_eq!(sanitize_dirname("..dots.."), "dots");
        assert_eq!(sanitize_dirname("x\u{0007}y"), "xy");
        assert_eq!(sanitize_dirname(&"字".repeat(300)).chars().count(), 200);
        assert_eq!(sanitize_dirname("   "), "");
    }

    /// 与宿主校验器共用的 golden:同一份 `book.md` 头,这里断言字节,
    /// 宿主 `src/lib/okf/book-head.test.ts` 断言它过 OKF 硬约束。
    /// 两侧都盯着同一个文件,任何一侧漂了都会红。
    #[test]
    fn book_head_matches_the_shared_golden() {
        let golden = include_str!("../tests/fixtures/book-head.md");
        let meta = BookMeta {
            title: Some("7 Powers".into()),
            creator: Some("Hamilton Helmer".into()),
            publisher: Some("Stripe Press".into()),
            language: Some("en".into()),
        };
        let head = book_frontmatter("/in/7 \"powers\".epub", &meta);
        assert!(
            golden.starts_with(&head),
            "golden drifted from book_frontmatter\n--- got ---\n{head}\n--- golden ---\n{golden}",
        );
    }

    #[test]
    fn book_frontmatter_carries_type_title_and_source() {
        let meta = BookMeta {
            title: Some("7 Powers".into()),
            creator: Some("Hamilton".into()),
            publisher: None,
            language: Some("en".into()),
        };
        let fm = book_frontmatter("/in/7powers.epub", &meta);
        assert_eq!(
            fm,
            concat!(
                "---\n",
                "type: Book\n",
                "title: \"7 Powers\"\n",
                "language: \"en\"\n",
                "sources:\n",
                "  - resource: \"/in/7powers.epub\"\n",
                "    author: \"Hamilton\"\n",
                "---\n",
            )
        );
    }

    #[test]
    fn book_frontmatter_degrades_to_type_and_source_only() {
        let fm = book_frontmatter("/in/unknown.pdf", &BookMeta::default());
        assert_eq!(
            fm,
            "---\ntype: Book\nsources:\n  - resource: \"/in/unknown.pdf\"\n---\n"
        );
    }

    #[test]
    fn book_frontmatter_escapes_quotes_and_backslashes() {
        let meta = BookMeta {
            title: Some("a \"quoted\" \\ title".into()),
            ..Default::default()
        };
        let fm = book_frontmatter("/in/x.epub", &meta);
        assert!(fm.contains("title: \"a \\\"quoted\\\" \\\\ title\"\n"));
    }

    #[test]
    fn config_txt_matches_bookread_format() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.txt");
        let meta = BookMeta {
            title: Some("7 Powers".into()),
            creator: Some("Hamilton".into()),
            publisher: None,
            language: Some("en".into()),
        };
        write_config_txt(&p, "/in/7powers.epub", "calibre_htmlz", &meta).unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("input_file=/in/7powers.epub"));
        assert!(s.contains("input_lang=auto"));
        assert!(s.contains("output_lang=zh"));
        assert!(s.contains("conversion_method=calibre_htmlz"));
        assert!(s.contains("original_title=7 Powers"));
        assert!(s.contains("creator=Hamilton"));
        assert!(!s.contains("publisher="));
        assert!(s.contains("source_language=en"));
    }
}
