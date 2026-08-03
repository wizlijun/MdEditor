//! `RoamPage` → `.note.md` outline `Tree`. Ported from
//! `plugins-src/roam-import/src/lib/roam-import/convert.ts`, with one
//! deliberate difference: every Roam block gets `persist_id = true` here,
//! whereas the TS integral-import path only persists ids for `((ref))`
//! targets. The daily-sync merge (Task 6) aligns blocks by uid on every
//! subsequent sync, so without an `id::` on each block the merge would have
//! no way to tell "same block, edited" from "new block" and would degrade to
//! whole-file overwrite.
use crate::outline::{touch_frontmatter, Node, Tree};
use crate::roam_page::{RoamBlock, RoamPage};
use crate::syntax::{convert_inline, escape_reserved_props, normalize_date_links};
use chrono::{SecondsFormat, TimeZone, Utc};

/// Roam epoch-millisecond timestamp → `new Date(ms).toISOString()`-compatible
/// string (UTC, millisecond precision, `Z` suffix). `to_rfc3339()` alone
/// would emit `+00:00` instead of `Z`; only the `Millis`-truncated
/// `SecondsFormat` variant matches byte-for-byte.
pub fn iso_ms(ms: Option<i64>) -> Option<String> {
    ms.map(|m| {
        Utc.timestamp_millis_opt(m)
            .single()
            .expect("Roam timestamps are plain epoch millis, not ambiguous/gap-adjacent")
            .to_rfc3339_opts(SecondsFormat::Millis, true)
    })
}

/// One block's content: inline syntax conversion, date-link normalization,
/// then escaping any continuation line that would otherwise be misread as a
/// `key:: value` outline property — in that order, matching `blockContent`
/// in convert.ts (minus `rewriteLinks`, which only applies to the full-graph
/// import path's uid-rename map and has no equivalent here).
fn block_content(b: &RoamBlock) -> String {
    let s = escape_reserved_props(&normalize_date_links(&convert_inline(&b.string)));
    match b.heading {
        Some(h) if (1..=3).contains(&h) => format!("{} {}", "#".repeat(h as usize), s),
        _ => s,
    }
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

/// `RoamPage` → a fresh `.note.md` outline `Tree` for `date` (`yyyy-MM-dd`).
/// Front-matter `title` is always `date`, never Roam's English daily title
/// ("August 2nd, 2026") — daily notes must match note.md's own
/// `yyyy-MM-dd` convention. An empty page produces empty `nodes`; unlike the
/// TS full-graph importer, no placeholder empty node is inserted here —
/// Task 6's merge owns deciding what an empty day looks like on disk.
pub fn convert_page(page: &RoamPage, date: &str) -> Tree {
    let created = iso_ms(page.create_time);
    let now = iso_ms(page.edit_time).unwrap_or_else(|| Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
    let created_for_touch = created.unwrap_or_else(|| now.clone());

    let mut tree = Tree { frontmatter: None, nodes: Vec::new() };
    walk(&mut tree, &page.children, None, &[]);
    tree.frontmatter = Some(touch_frontmatter(None, date, &created_for_touch, &now));
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let t = convert_page(&page(vec![block("hCIv7Y63h", "hi")]), "2026-08-02");
        assert_eq!(t.nodes[0].id, "hCIv7Y63h");
        assert!(t.nodes[0].persist_id, "every Roam block must carry id:: or the next merge cannot align");
    }

    #[test]
    fn inline_syntax_is_converted() {
        let t = convert_page(&page(vec![block("a", "{{[[TODO]]}} __x__")]), "2026-08-02");
        assert_eq!(t.nodes[0].content, "[ ] *x*");
    }

    #[test]
    fn heading_level_becomes_hashes() {
        let mut b = block("a", "Title");
        b.heading = Some(2);
        let t = convert_page(&page(vec![b]), "2026-08-02");
        assert_eq!(t.nodes[0].content, "## Title");
    }

    #[test]
    fn timestamps_match_the_ts_iso_format() {
        let mut b = block("a", "x");
        b.create_time = Some(1785600005019);
        let t = convert_page(&page(vec![b]), "2026-08-02");
        // 1785600005019ms verified independently against both Python
        // (datetime.utcfromtimestamp) and Node (`new Date(ms).toISOString()`):
        // both agree on 2026-08-01T16:00:05.019Z. The brief's placeholder
        // literal (2026-08-02T04:00:05.019Z) was wrong; per the brief's own
        // instruction this assertion — not the implementation — is corrected.
        assert_eq!(t.nodes[0].created_at.as_deref(), Some("2026-08-01T16:00:05.019Z"));
    }

    #[test]
    fn frontmatter_title_is_the_iso_date_not_the_roam_title() {
        let t = convert_page(&page(vec![]), "2026-08-02");
        assert!(t.frontmatter.as_ref().unwrap().contains("title: 2026-08-02"));
        assert!(!t.frontmatter.as_ref().unwrap().contains("August"));
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
        );
        let b = convert_page(
            &page(vec![block("o", "other"), with_child(block("p", "parent"), nameless("mine"))]),
            "2026-08-02",
        );
        assert_eq!(id_of(&a, "mine"), id_of(&b, "mine"));
        assert_eq!(id_of(&a, "mine"), "roam-1-0", "child index path from the page root");
        assert_eq!(id_of(&a, "other"), "roam-0");
    }

    #[test]
    fn children_become_nested_nodes() {
        let mut parent = block("p", "parent");
        parent.children = vec![block("c", "child")];
        let t = convert_page(&page(vec![parent]), "2026-08-02");
        let child = t.nodes.iter().find(|n| n.id == "c").unwrap();
        assert_eq!(child.parent.as_deref(), Some("p"));
    }
}
