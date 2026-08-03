//! Merge one day of Roam blocks into the day's existing `.note.md` outline.
//!
//! The product rule, and the reason this module is the riskiest code in the
//! plugin: **Roam is authoritative for Roam's own blocks, and the user's own
//! writing is never lost.** A daily sync runs unattended and repeatedly, so a
//! merge bug here does not annoy — it silently eats the margin notes that are
//! the whole point of note.md. Every branch below is written to fail towards
//! "keep the local node".
//!
//! Identity is the node id. Task 5 gives every Roam block `persist_id = true`,
//! so its uid survives the write/read round-trip and is still there on the
//! next sync. Anything in the file whose id is *not* in the incoming Roam tree
//! is either the user's own block or a block Roam has since deleted; both are
//! preserved, and the stats tell the two apart (a `local-N` placeholder id was
//! never written to disk, so an id that *was* persisted can only have come
//! from an earlier Roam sync).
use crate::outline::{Node, Tree};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Default)]
pub struct MergeStats {
    /// Roam blocks with no counterpart in the file.
    pub created: usize,
    /// Roam blocks whose counterpart exists but whose text changed.
    pub updated: usize,
    /// Blocks the user wrote in note.md (no `id::`), kept in place.
    pub kept_local: usize,
    /// Blocks that carry an `id::` Roam no longer knows — synced here once,
    /// deleted in Roam since. Kept: deleting the user's copy is not ours to do.
    pub roam_gone_kept: usize,
}

/// Everything the level walk needs, so its signature stays readable.
struct Ctx<'a> {
    local: &'a Tree,
    roam: &'a Tree,
    roam_uids: HashSet<&'a str>,
    local_by_id: HashMap<&'a str, &'a Node>,
}

/// One slot of a level's output list, tagged with the side that owns its
/// children: a Roam-owned node recurses back into `merge_level` (its local-only
/// children are found by id, wherever in the file they sat), a local-only node
/// brings its local subtree along verbatim.
struct Placed {
    node: Node,
    from_roam: bool,
}

/// Merge the Roam tree for a day into the `.note.md` tree already on disk.
/// Front-matter is carried through untouched — refreshing it is the caller's
/// job (Task 7), which knows the date and the clock.
pub fn merge(local: &Tree, roam: &Tree) -> (Tree, MergeStats) {
    // Rule 1. Only *persisted* ids count as Roam identity. Task 5 marks every
    // Roam block persisted, so in practice this is "all of them" — but a
    // `local-N` placeholder id is per-parse bookkeeping, and two independently
    // parsed trees hand those out from the same counter. Letting a placeholder
    // match would overwrite one of the user's own blocks with unrelated Roam
    // text, so identity is restricted to ids that actually exist in a file.
    let roam_uids: HashSet<&str> =
        roam.nodes.iter().filter(|n| n.persist_id).map(|n| n.id.as_str()).collect();
    // First occurrence wins if a hand-edited file repeats an `id::`. Children
    // are never looked up through this map (`children_of` matches on the parent
    // id string, so it already gathers the children of every duplicate), which
    // keeps a duplicate from swallowing a subtree.
    let mut local_by_id: HashMap<&str, &Node> = HashMap::new();
    for n in &local.nodes {
        local_by_id.entry(n.id.as_str()).or_insert(n);
    }

    let ctx = Ctx { local, roam, roam_uids, local_by_id };
    // Rule 8: front-matter passes through as-is.
    let mut out = Tree { frontmatter: local.frontmatter.clone(), nodes: Vec::new() };
    let mut stats = MergeStats::default();
    merge_level(&ctx, &mut out, &mut stats, None);
    (out, stats)
}

/// Build one output level and recurse. `parent` is an id in the *shared* space:
/// the root (`None`) or a Roam uid — the only levels where the two sides meet.
/// Levels below a local-only node have no Roam side at all and are handled by
/// `copy_local_subtree`.
fn merge_level(ctx: &Ctx, out: &mut Tree, stats: &mut MergeStats, parent: Option<&str>) {
    let mut level: Vec<Placed> = Vec::new();

    // Rule 2a: the Roam children of this level first, in Roam's order — Roam
    // owns the shape of its own content.
    for rn in ctx.roam.children_of(parent) {
        let counterpart = ctx.local_by_id.get(rn.id.as_str()).copied();
        // Rule 7.
        match counterpart {
            None => stats.created += 1,
            Some(l) if l.content != rn.content => stats.updated += 1,
            Some(_) => {}
        }
        level.push(Placed { node: merged_node(rn, counterpart), from_roam: true });
    }

    // Rule 2b: then this level's local blocks, in their original local order —
    // which is what makes a run of consecutive local blocks land in order, each
    // anchoring on the one inserted just before it.
    let local_siblings = ctx.local.children_of(parent);
    for (i, ln) in local_siblings.iter().enumerate() {
        if ctx.roam_uids.contains(ln.id.as_str()) {
            continue; // a Roam block: already emitted above, or moved elsewhere.
        }
        let at = match local_siblings[..i].iter().rev().find_map(|p| slot_of(&level, &p.id)) {
            // Nearest local sibling that is already in the output — a surviving
            // Roam block, or a local block inserted a moment ago. Sit after it.
            Some(pos) => pos + 1,
            None => {
                if local_siblings[i + 1..].iter().any(|s| slot_of(&level, &s.id).is_some()) {
                    // Nothing placed before it, but a placed sibling follows:
                    // it sat above every Roam block it knew, so it stays above
                    // them — the head of the level, not merely above that one.
                    0
                } else {
                    // No placed sibling in either direction (e.g. every Roam
                    // block at this level is brand new, as in a note the user
                    // hung under a Roam block Roam has only now given children
                    // of its own). Nothing anchors it, so append rather than
                    // invent a claim that it belongs above the new material.
                    level.len()
                }
            }
        };
        // Rule 7: an id that was persisted can only have come from an earlier
        // sync, so a local block carrying one is a Roam deletion, not the
        // user's own writing.
        if ln.persist_id {
            stats.roam_gone_kept += 1;
        } else {
            stats.kept_local += 1;
        }
        level.insert(at, Placed { node: (*ln).clone(), from_roam: false });
    }

    for (i, mut placed) in level.into_iter().enumerate() {
        placed.node.parent = parent.map(str::to_string);
        placed.node.order = i as i64 * 100; // rule 6
        let id = placed.node.id.clone();
        let from_roam = placed.from_roam;
        out.nodes.push(placed.node);
        if from_roam {
            // Rule 4: recurse on the id, not on the local position — a block
            // Roam moved under a different parent must find its local-only
            // children wherever in the file they still sit.
            merge_level(ctx, out, stats, Some(&id));
        } else {
            copy_local_subtree(ctx, out, stats, &id);
        }
    }
}

/// Rule 3. Roam owns the text and the timestamps; everything else is local
/// state the Roam side does not model at all (`convert_page` leaves it at its
/// defaults), so taking it from Roam would silently reset it on every sync:
/// `collapsed` is how the user folded their outline, and `type`/`status`/
/// `answered` carry the annotation Q&A state they may have put on the block.
fn merged_node(rn: &Node, local: Option<&Node>) -> Node {
    let view = local.unwrap_or(rn);
    Node {
        id: rn.id.clone(),
        parent: None, // set by the caller, which knows the output level
        order: 0,     // ditto (rule 6)
        content: rn.content.clone(),
        created_at: rn.created_at.clone(),
        updated_at: rn.updated_at.clone(),
        collapsed: view.collapsed,
        source: view.source.clone(),
        anchor_line: view.anchor_line,
        status: view.status.clone(),
        answered_at: view.answered_at.clone(),
        answered_by: view.answered_by.clone(),
        // Rule 3 asks for `true`; carrying Roam's own flag says the same thing
        // for every tree Task 5 builds, without writing a placeholder id into
        // the file should a Roam block ever reach here without a uid.
        persist_id: rn.persist_id,
    }
}

/// Rule 5. Copy a preserved local node's children verbatim, minus any
/// descendant Roam still knows: that one is emitted inside the Roam structure
/// instead (with its own local-only children, picked up when the recursion
/// reaches it there), and emitting it here too would duplicate it.
fn copy_local_subtree(ctx: &Ctx, out: &mut Tree, stats: &mut MergeStats, parent_id: &str) {
    let mut kept = 0i64;
    for ln in ctx.local.children_of(Some(parent_id)) {
        if ctx.roam_uids.contains(ln.id.as_str()) {
            continue; // dropped together with its whole subtree
        }
        if ln.persist_id {
            stats.roam_gone_kept += 1;
        } else {
            stats.kept_local += 1;
        }
        let mut node = ln.clone();
        node.parent = Some(parent_id.to_string());
        node.order = kept * 100; // rule 6, over the kept children only
        kept += 1;
        let id = node.id.clone();
        out.nodes.push(node);
        copy_local_subtree(ctx, out, stats, &id);
    }
}

/// Position of `id` in the level built so far, if it has been placed at all.
fn slot_of(level: &[Placed], id: &str) -> Option<usize> {
    level.iter().position(|p| p.node.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::{parse_outline, serialize_outline};

    /// Roam-side trees always carry id::, so they can be written as text.
    fn roam(text: &str) -> crate::outline::Tree { parse_outline(text) }

    #[test]
    fn empty_local_takes_the_roam_tree_whole() {
        let r = roam("- a\n  id:: u1\n- b\n  id:: u2\n");
        let (out, st) = merge(&parse_outline(""), &r);
        assert_eq!(serialize_outline(&out), "- a\n  id:: u1\n- b\n  id:: u2\n");
        assert_eq!(st.created, 2);
    }

    #[test]
    fn same_uid_takes_the_roam_content() {
        let local = parse_outline("- old text\n  id:: u1\n");
        let (out, st) = merge(&local, &roam("- new text\n  id:: u1\n"));
        assert_eq!(serialize_outline(&out), "- new text\n  id:: u1\n");
        assert_eq!(st.updated, 1);
        assert_eq!(st.created, 0);
    }

    #[test]
    fn a_local_block_keeps_its_place_between_roam_blocks() {
        let local = parse_outline("- a\n  id:: u1\n- mine\n- b\n  id:: u2\n");
        let (out, st) = merge(&local, &roam("- a\n  id:: u1\n- b\n  id:: u2\n"));
        assert_eq!(serialize_outline(&out), "- a\n  id:: u1\n- mine\n- b\n  id:: u2\n");
        assert_eq!(st.kept_local, 1);
    }

    #[test]
    fn a_local_block_before_every_roam_block_stays_at_the_top() {
        let local = parse_outline("- mine\n- a\n  id:: u1\n");
        let (out, _) = merge(&local, &roam("- a\n  id:: u1\n"));
        assert_eq!(serialize_outline(&out), "- mine\n- a\n  id:: u1\n");
    }

    #[test]
    fn a_block_deleted_in_roam_is_kept_locally() {
        let local = parse_outline("- a\n  id:: u1\n- gone\n  id:: u9\n");
        let (out, st) = merge(&local, &roam("- a\n  id:: u1\n"));
        assert_eq!(serialize_outline(&out), "- a\n  id:: u1\n- gone\n  id:: u9\n");
        assert_eq!(st.roam_gone_kept, 1);
        assert_eq!(st.kept_local, 0);
    }

    #[test]
    fn local_children_of_a_roam_block_survive() {
        let local = parse_outline("- a\n  id:: u1\n  - my note\n");
        let (out, _) = merge(&local, &roam("- a\n  id:: u1\n  - from roam\n    id:: u2\n"));
        assert_eq!(serialize_outline(&out), "- a\n  id:: u1\n  - from roam\n    id:: u2\n  - my note\n");
    }

    #[test]
    fn a_block_moved_to_another_parent_in_roam_takes_its_local_children_along() {
        let local = parse_outline("- p1\n  id:: u1\n  - moved\n    id:: u9\n    - my note\n- p2\n  id:: u2\n");
        let r = roam("- p1\n  id:: u1\n- p2\n  id:: u2\n  - moved\n    id:: u9\n");
        let (out, _) = merge(&local, &r);
        let text = serialize_outline(&out);
        assert_eq!(text, "- p1\n  id:: u1\n- p2\n  id:: u2\n  - moved\n    id:: u9\n    - my note\n");
        assert_eq!(text.matches("id:: u9").count(), 1, "the moved block must not be duplicated");
    }

    #[test]
    fn collapsed_is_a_local_view_state_and_survives_a_sync() {
        let local = parse_outline("- a\n  id:: u1\n  collapsed:: true\n");
        let (out, _) = merge(&local, &roam("- a\n  id:: u1\n"));
        assert!(serialize_outline(&out).contains("collapsed:: true"));
    }

    #[test]
    fn merging_twice_changes_nothing() {
        let local = parse_outline("- a\n  id:: u1\n- mine\n- gone\n  id:: u9\n");
        let r = roam("- a\n  id:: u1\n- b\n  id:: u2\n");
        let (once, _) = merge(&local, &r);
        let (twice, st) = merge(&once, &r);
        assert_eq!(serialize_outline(&once), serialize_outline(&twice));
        assert_eq!(st.created, 0);
    }

    // The cases below are not in the brief; they pin down the corners a
    // careless refactor would break, and they are the ones a merge bug would
    // cost the user the most.

    /// A margin note hanging off a block Roam has since deleted: the whole
    /// local branch is preserved, note and all.
    #[test]
    fn a_local_child_of_a_roam_deleted_block_survives_with_it() {
        let local = parse_outline("- gone\n  id:: u9\n  - my note\n- a\n  id:: u1\n");
        let (out, st) = merge(&local, &roam("- a\n  id:: u1\n"));
        assert_eq!(serialize_outline(&out), "- gone\n  id:: u9\n  - my note\n- a\n  id:: u1\n");
        assert_eq!(st.roam_gone_kept, 1);
        assert_eq!(st.kept_local, 1, "a nested local block counts as kept too");
    }

    /// Consecutive local blocks above every Roam block keep their own order —
    /// the second anchors on the first, which was just placed.
    #[test]
    fn a_run_of_local_blocks_at_the_head_stays_in_order() {
        let local = parse_outline("- m1\n- m2\n- a\n  id:: u1\n");
        let (out, st) = merge(&local, &roam("- a\n  id:: u1\n- b\n  id:: u2\n"));
        assert_eq!(serialize_outline(&out), "- m1\n- m2\n- a\n  id:: u1\n- b\n  id:: u2\n");
        assert_eq!(st.kept_local, 2);
    }

    /// A Roam block whose local counterpart has both Roam children and a local
    /// one: Roam's edits land, Roam's new child lands, the local child holds
    /// its position after the sibling it followed.
    #[test]
    fn roam_and_local_children_of_the_same_block_interleave() {
        let local = parse_outline("- p\n  id:: u1\n  - r1\n    id:: u2\n  - mine\n");
        let r = roam("- p\n  id:: u1\n  - r1 edited\n    id:: u2\n  - r2\n    id:: u3\n");
        let (out, st) = merge(&local, &r);
        assert_eq!(
            serialize_outline(&out),
            "- p\n  id:: u1\n  - r1 edited\n    id:: u2\n  - mine\n  - r2\n    id:: u3\n"
        );
        assert_eq!((st.created, st.updated, st.kept_local), (1, 1, 1));
    }

    /// Idempotence where it is hard: re-merging after a block moved parents
    /// must not move it back, duplicate it, or lose the note under it.
    #[test]
    fn merging_twice_changes_nothing_after_a_move() {
        let local = parse_outline("- p1\n  id:: u1\n  - moved\n    id:: u9\n    - my note\n- p2\n  id:: u2\n");
        let r = roam("- p1\n  id:: u1\n- p2\n  id:: u2\n  - moved\n    id:: u9\n");
        let (once, _) = merge(&local, &r);
        let (twice, st) = merge(&once, &r);
        assert_eq!(serialize_outline(&twice), serialize_outline(&once));
        assert_eq!((st.created, st.updated), (0, 0));
        assert_eq!(serialize_outline(&twice).matches("id:: u9").count(), 1);
        assert!(serialize_outline(&twice).contains("- my note"));
    }

    /// Roam owns text and timestamps; the annotation state the user put on the
    /// block is theirs and is not Roam's to reset.
    #[test]
    fn local_annotation_state_survives_a_roam_edit() {
        let local = parse_outline("- ask me\n  type:: question\n  status:: answered\n  id:: u1\n");
        let (out, st) = merge(&local, &roam("- ask me, edited\n  id:: u1\n"));
        assert_eq!(
            serialize_outline(&out),
            "- ask me, edited\n  type:: question\n  status:: answered\n  id:: u1\n"
        );
        assert_eq!(st.updated, 1);
    }
}
