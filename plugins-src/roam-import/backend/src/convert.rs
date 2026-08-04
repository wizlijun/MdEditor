//! `RoamPage` → `.note.md` outline `Tree`. Ported from
//! `plugins-src/roam-import/src/lib/roam-import/convert.ts`, with one
//! deliberate difference: every Roam block gets `persist_id = true` here,
//! whereas the TS integral-import path only persists ids for `((ref))`
//! targets. The daily-sync merge (Task 6) aligns blocks by uid on every
//! subsequent sync, so without an `id::` on each block the merge would have
//! no way to tell "same block, edited" from "new block" and would degrade to
//! whole-file overwrite.
use crate::outline::{fence_close_len, fence_open_len, touch_frontmatter, Node, Tree};
use crate::roam_page::{RoamBlock, RoamPage};
use crate::syntax::{convert_inline, escape_structural_lines, normalize_date_links};
use chrono::{SecondsFormat, TimeZone, Utc};

/// Roam epoch-millisecond timestamp → `new Date(ms).toISOString()`-compatible
/// string (UTC, millisecond precision, `Z` suffix). `to_rfc3339()` alone
/// would emit `+00:00` instead of `Z`; only the `Millis`-truncated
/// `SecondsFormat` variant matches byte-for-byte.
///
/// A value chrono cannot represent reads as `None`, not a panic: these are
/// plain `i64`s out of a subprocess's JSON, so a garbage/overflowing
/// timestamp is untrusted input, and a panic here would take the whole plugin
/// process down mid-session. A block without a usable `created::`/`updated::`
/// is a cosmetic loss; a dead process loses the sync.
pub fn iso_ms(ms: Option<i64>) -> Option<String> {
    Utc.timestamp_millis_opt(ms?)
        .single()
        .map(|t| t.to_rfc3339_opts(SecondsFormat::Millis, true))
}

/// One block's content, in the exact form `parse_outline` will read it back.
/// Inline syntax conversion and date-link normalization first (matching
/// `blockContent` in convert.ts, minus `rewriteLinks` — that only applies to
/// the full-graph import path's uid-rename map and has no equivalent here),
/// then the escaping this comment is really about.
///
/// **`parse_outline` treats THREE shapes as structure, and the escaping has to
/// cover all three.** A Roam block whose own text contains one is otherwise
/// re-read as something other than itself on the next sync — it loses the
/// `id::` line that is its identity, so `merge` sees a brand-new Roam block
/// and re-creates it, duplicating the user's note on every single run:
///
/// 1. `key:: value` on a continuation line → a node property.
/// 2. `  - text` on a continuation line → a *child bullet*. Roam's
///    shift-enter lists are exactly this.
/// 3. a fence opener on the block's FIRST line → raw mode, which runs until a
///    matching closer and swallows every following block when the text never
///    closes it. Neutralized by [`close_dangling_fence`].
///
/// 1 and 2 are fixed with one leading space (renders the same, no longer
/// matches) by [`escape_structural_lines`], which skips the region 3 makes
/// raw — the parser cannot misread code it takes verbatim, and escaping
/// inside a fence would edit the sample the user pasted. 3 cannot be fixed
/// with a space — the first line is the bullet's own text — so the missing
/// closer is appended instead. All three are idempotent because the content is
/// re-derived from Roam on every sync rather than re-read from the file.
///
/// Anyone teaching `parse_outline` a fourth structural shape has to teach it
/// to this function in the same commit.
fn block_content(b: &RoamBlock) -> String {
    // `\r` first, before anything looks at lines: `parse_outline` strips it at
    // its own entry (line-ending noise, never content), so a `\r` written here
    // is by definition not what is read back — and this function's whole
    // contract is "the exact form `parse_outline` will read it back". Doing it
    // first also means the fence tracking below and the parser agree on where
    // the lines are.
    let s = b.string.replace('\r', "");
    let s = normalize_date_links(&convert_inline(&s));
    // The heading prefix comes before both fence-aware steps on purpose: with
    // it, the first line no longer *starts* with backticks, so it opens no
    // fence at all — and `parse_outline` reads it back the same way. Prefixing
    // first is what keeps the escape's raw-region tracking and
    // `close_dangling_fence` agreeing with the parser (and with each other).
    let s = match b.heading {
        Some(h) if (1..=3).contains(&h) => format!("{} {}", "#".repeat(h as usize), s),
        _ => s,
    };
    close_dangling_fence(escape_structural_lines(&s))
}

/// Shape 3 of [`block_content`]'s list. `parse_outline` enters raw mode when a
/// bullet's first line opens a fence and leaves it only on a closing line at
/// least as long — a closer this block never writes is then found somewhere in
/// the *following* blocks, which the fence has already eaten by then. Hand the
/// block back its own closer.
fn close_dangling_fence(s: String) -> String {
    let mut lines = s.split('\n');
    let Some(open) = lines.next().and_then(fence_open_len) else { return s };
    // Only the lines AFTER the opener can close it — the opener is not its own
    // closer (`parse_outline` checks continuation lines only).
    if lines.any(|l| fence_close_len(l).is_some_and(|close| close >= open)) {
        return s;
    }
    format!("{s}\n{}", "`".repeat(open))
}

/// Fallback id for a block Roam returned without a `uid`: its child-index
/// path from the page root, e.g. `roam-0-2-1`. Two different blocks can never
/// share a path, and — unlike a running counter — a block's path does not
/// move when something elsewhere on the page changes. That matters because
/// this id IS written to the file (`persist_id` below) and the next sync's
/// merge treats it as identity: an id that drifted to another block would
/// overwrite that block's text and reparent the user's notes under it.
fn fallback_id(path: &[usize]) -> String {
    let mut id = String::from("roam");
    for i in path {
        id.push('-');
        id.push_str(&i.to_string());
    }
    id
}

/// Depth-first walk of Roam's already order-sorted block tree, flattening it
/// into `tree.nodes`. `path` is the child-index path of `blocks`' parent,
/// used only to name blocks that arrive without a `uid`.
fn walk(tree: &mut Tree, blocks: &[RoamBlock], parent: Option<&str>, path: &[usize]) {
    for (idx, b) in blocks.iter().enumerate() {
        let mut here = path.to_vec();
        here.push(idx);
        let id = match &b.uid {
            Some(uid) => uid.clone(),
            None => fallback_id(&here),
        };
        let node = Node {
            id: id.clone(),
            parent: parent.map(|p| p.to_string()),
            order: idx as i64 * 100,
            content: block_content(b),
            collapsed: false,
            source: "manual".to_string(),
            anchor_line: None,
            status: None,
            answered_at: None,
            answered_by: None,
            created_at: iso_ms(b.create_time),
            updated_at: iso_ms(b.edit_time),
            persist_id: true,
        };
        tree.nodes.push(node);
        walk(tree, &b.children, Some(id.as_str()), &here);
    }
}

/// `RoamPage` → a fresh `.note.md` outline `Tree` for `title` under
/// `concept_type` (OKF §4.1). For a daily page `title` is always the
/// `yyyy-MM-dd` date, never Roam's English daily title ("August 2nd, 2026")
/// — daily notes must match note.md's own `yyyy-MM-dd` convention; for any
/// other page it is Roam's own title. The OKF type is the caller's call, not
/// this function's — it is `sync_page`'s caller that knows whether the page
/// being written is a day or a wikipage. An empty page produces empty
/// `nodes`; unlike the TS full-graph importer, no placeholder empty node is
/// inserted here — the merge owns deciding what an empty page looks like on
/// disk.
pub fn convert_page(page: &RoamPage, title: &str, concept_type: &str) -> Tree {
    let created = iso_ms(page.create_time);
    let now = iso_ms(page.edit_time).unwrap_or_else(|| Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
    let created_for_touch = created.unwrap_or_else(|| now.clone());

    let mut tree = Tree { frontmatter: None, nodes: Vec::new() };
    walk(&mut tree, &page.children, None, &[]);
    tree.frontmatter =
        Some(touch_frontmatter(None, concept_type, title, &created_for_touch, &now));
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::CONCEPT_TYPE_DAILY_NOTE;
    use crate::roam_page::{RoamBlock, RoamPage};

    fn block(uid: &str, s: &str) -> RoamBlock {
        RoamBlock { uid: Some(uid.into()), string: s.into(), order: 0, heading: None,
                    create_time: None, edit_time: None, children: vec![] }
    }

    fn page(children: Vec<RoamBlock>) -> RoamPage {
        RoamPage { title: "August 2nd, 2026".into(), uid: Some("08-02-2026".into()),
                   create_time: Some(1785600005019), edit_time: None, children }
    }

    #[test]
    fn node_id_is_the_roam_uid_and_is_always_persisted() {
        let t = convert_page(&page(vec![block("hCIv7Y63h", "hi")]), "2026-08-02", CONCEPT_TYPE_DAILY_NOTE);
        assert_eq!(t.nodes[0].id, "hCIv7Y63h");
        assert!(t.nodes[0].persist_id, "every Roam block must carry id:: or the next merge cannot align");
    }

    #[test]
    fn inline_syntax_is_converted() {
        let t = convert_page(&page(vec![block("a", "{{[[TODO]]}} __x__")]), "2026-08-02", CONCEPT_TYPE_DAILY_NOTE);
        assert_eq!(t.nodes[0].content, "[ ] *x*");
    }

    #[test]
    fn heading_level_becomes_hashes() {
        let mut b = block("a", "Title");
        b.heading = Some(2);
        let t = convert_page(&page(vec![b]), "2026-08-02", CONCEPT_TYPE_DAILY_NOTE);
        assert_eq!(t.nodes[0].content, "## Title");
    }

    #[test]
    fn timestamps_match_the_ts_iso_format() {
        let mut b = block("a", "x");
        b.create_time = Some(1785600005019);
        let t = convert_page(&page(vec![b]), "2026-08-02", CONCEPT_TYPE_DAILY_NOTE);
        // 1785600005019ms verified independently against both Python
        // (datetime.utcfromtimestamp) and Node (`new Date(ms).toISOString()`):
        // both agree on 2026-08-01T16:00:05.019Z. The brief's placeholder
        // literal (2026-08-02T04:00:05.019Z) was wrong; per the brief's own
        // instruction this assertion — not the implementation — is corrected.
        assert_eq!(t.nodes[0].created_at.as_deref(), Some("2026-08-01T16:00:05.019Z"));
    }

    #[test]
    fn frontmatter_title_is_the_iso_date_not_the_roam_title() {
        let t = convert_page(&page(vec![]), "2026-08-02", CONCEPT_TYPE_DAILY_NOTE);
        assert!(t.frontmatter.as_ref().unwrap().contains("title: \"2026-08-02\""));
        assert!(!t.frontmatter.as_ref().unwrap().contains("August"));
    }

    /// OKF v0.2 §4.1: the `type` is REQUIRED, and a Roam daily page is a
    /// `Daily Note` — the type the host's `outlineConceptType` derives from the
    /// daily folder this note is written into.
    #[test]
    fn frontmatter_carries_the_okf_daily_note_type() {
        let t = convert_page(&page(vec![]), "2026-08-02", CONCEPT_TYPE_DAILY_NOTE);
        assert!(t.frontmatter.as_ref().unwrap().starts_with("type: Daily Note\n"));
    }

    /// A uid-less block's fallback id is written to the file (`persist_id`),
    /// and the next sync's merge treats an id it finds there as identity. A
    /// running counter makes that id depend on how many *other* uid-less
    /// blocks the walk happened to pass first, so an unrelated block
    /// elsewhere on the page changing can hand this block's id to a different
    /// block — merge would then overwrite that block's text and reparent the
    /// user's children under it. The id must come from the block's own
    /// position in the tree. (Roam always assigns uids and the pull asks for
    /// `:block/uid`, so this is a landmine, not a live bug.)
    #[test]
    fn a_uid_less_block_is_identified_by_its_position_not_by_a_counter() {
        let nameless = |s: &str| RoamBlock {
            uid: None, string: s.into(), order: 0, heading: None,
            create_time: None, edit_time: None, children: vec![],
        };
        let with_child = |head: RoamBlock, child: RoamBlock| {
            let mut h = head;
            h.children = vec![child];
            h
        };
        let id_of = |t: &Tree, s: &str| {
            t.nodes.iter().find(|n| n.content == s).unwrap().id.clone()
        };

        // Same shape, same position for "mine" — the only difference is that
        // the block above it has a uid in one tree and not in the other.
        let a = convert_page(
            &page(vec![nameless("other"), with_child(block("p", "parent"), nameless("mine"))]),
            "2026-08-02",
            CONCEPT_TYPE_DAILY_NOTE,
        );
        let b = convert_page(
            &page(vec![block("o", "other"), with_child(block("p", "parent"), nameless("mine"))]),
            "2026-08-02",
            CONCEPT_TYPE_DAILY_NOTE,
        );
        assert_eq!(id_of(&a, "mine"), id_of(&b, "mine"));
        assert_eq!(id_of(&a, "mine"), "roam-1-0", "child index path from the page root");
        assert_eq!(id_of(&a, "other"), "roam-0");
    }

    /// The three shapes `block_content`'s doc comment names, each asserted at
    /// the level that matters: the emitted content must survive a
    /// `serialize → parse` round-trip as ONE node with the same text. Anything
    /// less and the next sync sees a different tree than the one it wrote.
    fn survives_a_round_trip(roam_text: &str) -> String {
        use crate::outline::{parse_outline, serialize_outline};
        let t = convert_page(&page(vec![block("u1", roam_text)]), "2026-08-02", CONCEPT_TYPE_DAILY_NOTE);
        let text = serialize_outline(&t);
        let back = parse_outline(&text);
        assert_eq!(back.nodes.len(), 1, "must read back as exactly one node:\n{text}");
        assert_eq!(back.nodes[0].id, "u1", "the block must keep its identity:\n{text}");
        assert_eq!(back.nodes[0].content, t.nodes[0].content, "content drifted:\n{text}");
        t.nodes[0].content.clone()
    }

    #[test]
    fn a_property_shaped_continuation_line_stays_content() {
        assert_eq!(survives_a_round_trip("meeting notes\nid:: not-a-property"),
                   "meeting notes\n id:: not-a-property");
    }

    /// C2: an ordinary Roam shift-enter list. Before the escape, `- milk` was
    /// re-read as a child bullet, which pushed the block's own `id:: u1` out of
    /// its continuation indent and cost the block its identity — merge then
    /// re-created it, once per sync, forever.
    #[test]
    fn a_bullet_shaped_continuation_line_stays_content() {
        assert_eq!(survives_a_round_trip("shopping\n- milk\n- eggs"),
                   "shopping\n - milk\n - eggs");
        assert_eq!(survives_a_round_trip("outline\n  - nested\n    - deeper"),
                   "outline\n   - nested\n     - deeper");
    }

    /// C2b, the fourth structural shape. `parse_outline` now also reads a line
    /// of nothing but indentation and `-` as a (empty) bullet — an empty block
    /// is written `- `, and the trailing space does not survive contact with
    /// editors/formatters/git hooks, so the parser accepts both spellings. That
    /// makes a Roam block containing an empty shift-enter line the same hazard
    /// `- milk` was: read back as a child, the block loses the `id::` that is
    /// its identity and `merge` re-creates it on every sync, forever.
    /// `block_content`'s doc comment says a new structural shape must be taught
    /// here in the same commit; this is that.
    #[test]
    fn an_empty_bullet_shaped_continuation_line_stays_content() {
        assert_eq!(survives_a_round_trip("shopping\n-\nmilk"), "shopping\n -\nmilk");
        assert_eq!(survives_a_round_trip("outline\n  -\n    -"), "outline\n   -\n     -");
    }

    /// A Roam block whose text carries a `\r` (Windows-authored, or a Roam
    /// soft break): `parse_outline` strips `\r` at its entry now, so a block
    /// written with one is NOT what is read back — the round-trip contract in
    /// this function's doc comment ("the exact form `parse_outline` will read
    /// it back") fails unless the conversion normalises it too.
    #[test]
    fn a_block_carrying_a_carriage_return_is_normalised_before_it_is_written() {
        assert_eq!(survives_a_round_trip("a\r\nb"), "a\nb");
        assert_eq!(survives_a_round_trip("lone\rcr"), "lonecr");
    }

    /// C3: a fence the block opens and never closes put `parse_outline` into a
    /// raw mode that swallowed the following blocks whole.
    #[test]
    fn an_unterminated_fence_is_closed() {
        assert_eq!(survives_a_round_trip("```js\nconst x = 1"), "```js\nconst x = 1\n```");
        // A bare opener is not its own closer.
        assert_eq!(survives_a_round_trip("```"), "```\n```");
        // Longer fences get a closer of the same length.
        assert_eq!(survives_a_round_trip("````\n```\nstill inside"),
                   "````\n```\nstill inside\n````");
    }

    #[test]
    fn a_closed_fence_is_left_exactly_as_roam_wrote_it() {
        assert_eq!(survives_a_round_trip("```js\nconst x = 1\n```"), "```js\nconst x = 1\n```");
        // Trailing prose after the closer is outside the fence and unaffected.
        assert_eq!(survives_a_round_trip("```\nx\n```\nafter"), "```\nx\n```\nafter");
    }

    /// R2: the escapes are for lines `parse_outline` would read as structure,
    /// and inside the block's own fence it reads nothing as structure — it
    /// takes the lines verbatim. A Roam block that is a fenced YAML sample must
    /// therefore come out byte-identical, not with a space quietly inserted
    /// into every `- foo` of the user's code.
    #[test]
    fn a_fenced_list_is_written_exactly_as_the_user_pasted_it() {
        assert_eq!(
            survives_a_round_trip("```yaml\n- foo\n- bar\n```"),
            "```yaml\n- foo\n- bar\n```"
        );
        // Same for the other escaped shape: `key:: value` is code in there.
        assert_eq!(
            survives_a_round_trip("```\nid:: not-a-property\n```"),
            "```\nid:: not-a-property\n```"
        );
        // An unterminated fenced list: the closer is still appended (C3), and
        // the body is still untouched.
        assert_eq!(survives_a_round_trip("```yaml\n- foo"), "```yaml\n- foo\n```");
        // Once the fence closes the parser is reading structure again, so the
        // tail is escaped as usual.
        assert_eq!(
            survives_a_round_trip("```\n- inside\n```\n- after"),
            "```\n- inside\n```\n - after"
        );
    }

    /// The other direction, and the reason the fence-awareness is deliberately
    /// narrow: a fence opened on a *later* line never puts `parse_outline` into
    /// raw mode, so those lines really are read as structure and really do have
    /// to be escaped — round-trip fidelity over fence cosmetics.
    #[test]
    fn a_fence_opened_after_the_first_line_is_still_escaped() {
        assert_eq!(
            survives_a_round_trip("prose\n```yaml\n- foo\n```"),
            "prose\n```yaml\n - foo\n```"
        );
    }

    /// A heading turns the first line into `## …`, which opens no fence at all
    /// — so nothing must be appended, or the block would grow a stray ``` line
    /// on every sync.
    #[test]
    fn a_heading_block_starting_with_backticks_gets_no_closer() {
        let mut b = block("u1", "```js");
        b.heading = Some(2);
        let t = convert_page(&page(vec![b]), "2026-08-02", CONCEPT_TYPE_DAILY_NOTE);
        assert_eq!(t.nodes[0].content, "## ```js");
    }

    /// A timestamp chrono cannot represent arrives here as a plain i64 from a
    /// subprocess's JSON. It must degrade to "no timestamp", never panic — a
    /// panic takes the plugin process down and the sync with it.
    #[test]
    fn an_out_of_range_timestamp_reads_as_none_instead_of_panicking() {
        assert_eq!(iso_ms(Some(i64::MAX)), None);
        assert_eq!(iso_ms(Some(i64::MIN)), None);
        assert_eq!(iso_ms(None), None);
        assert_eq!(iso_ms(Some(0)).as_deref(), Some("1970-01-01T00:00:00.000Z"));
    }

    #[test]
    fn children_become_nested_nodes() {
        let mut parent = block("p", "parent");
        parent.children = vec![block("c", "child")];
        let t = convert_page(&page(vec![parent]), "2026-08-02", CONCEPT_TYPE_DAILY_NOTE);
        let child = t.nodes.iter().find(|n| n.id == "c").unwrap();
        assert_eq!(child.parent.as_deref(), Some("p"));
    }
}
