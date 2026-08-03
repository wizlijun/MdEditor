//! Re-key the id-less local blocks an *earlier* import wrote, so the merge can
//! recognise them.
//!
//! Two import paths write the same `.note.md`, and they disagree about `id::`.
//! The TypeScript full-graph importer
//! (`src/lib/roam-import/convert.ts`) persists an id only for blocks that are
//! `((ref))` targets; this crate's daily sync persists one on **every** block,
//! because `merge` aligns Roam to the file by that id and nothing else. So a
//! page first built by the JSON importer and then synced by the CLI aligns on
//! nothing: every Roam block reads as `created`, the id-less copies read as the
//! user's own writing and are kept (which is `merge`'s whole contract — it
//! never deletes what the user wrote), and the page doubles. Observed in the
//! user's vault: the same block twice, byte-identical text and `created::`, one
//! copy with an `id::` and one without, `created=93 kept_local=93` for the day.
//!
//! This runs *before* `merge` and changes nothing about it: it hands the
//! id-less copy the uid it should always have had, after which `merge`'s
//! existing alignment simply succeeds. `merge` is deliberately untouched — its
//! "the user's writing is never lost" semantics are the riskiest thing in the
//! plugin, and a rule that guesses which blocks are the same block does not
//! belong inside them.
//!
//! **The key is `content` + `created_at`, both exact.** Both import paths
//! derive the text from the same inline-syntax conversion and the timestamp
//! from the same Roam `:create/time` field, in the same ISO-8601 UTC
//! millisecond format — which is why the duplicated pairs in the vault are
//! byte-identical on both. Content alone is not a key (`- [ ] follow up`
//! appears on twenty days); a normalised or truncated timestamp is not a key
//! either (it is the only thing making the text unique); and a local block with
//! no `created::` has no key at all and is never adopted.
use crate::outline::Tree;
use std::collections::{HashMap, HashSet, VecDeque};

/// Give local blocks that came from the id-less import path the Roam uid of the
/// block they *are*, so `merge` aligns on them instead of creating a second
/// copy. Returns how many local nodes were adopted.
///
/// Matching is one-to-one in both directions: a Roam block adopts at most one
/// local node, and a local node is adopted at most once. Ambiguity is left
/// alone rather than resolved — the vault holds pre-existing pairs of id-less
/// twins (both copies from before any of this), and picking "the right one" out
/// of two blocks with identical text and identical timestamps is not a decision
/// this pass can make correctly. The first in document order is adopted; the
/// rest stay ordinary local blocks, exactly as they are today.
pub fn adopt_ids(local: &mut Tree, roam: &Tree) -> usize {
    // Candidates, keyed exactly as they will be looked up, in document order:
    // a node with no persisted id (an `id::` already in the file is an identity
    // an earlier sync established, and re-keying it would hand this block
    // another block's text on the next sync) and a `created::` to key on.
    let mut candidates: HashMap<(&str, &str), VecDeque<usize>> = HashMap::new();
    for (i, n) in local.nodes.iter().enumerate() {
        if n.persist_id {
            continue;
        }
        if let Some(created) = &n.created_at {
            candidates.entry((n.content.as_str(), created.as_str())).or_default().push_back(i);
        }
    }
    // Every id the file already holds — placeholders included, since a
    // placeholder is what an unadopted node keeps. Adoption must never mint a
    // second node carrying an id some node already has: parentage is by id
    // *string*, so a shared id makes `children_of` gather both nodes' children
    // under each of them, duplicating a subtree into the user's own vault (the
    // hazard `parse_outline`'s own duplicate-`id::` guard exists for). A page
    // that doubled BEFORE this fix hits exactly that: the id-less copy and the
    // CLI-stamped copy sit side by side, and the stamped one keeps the
    // identity. Healing that page means deleting one of the two, which is not
    // this pass's call — and not `merge`'s either.
    let mut taken: HashSet<&str> = local.nodes.iter().map(|n| n.id.as_str()).collect();

    // Borrowck aside: the keys above borrow `local.nodes`, which the re-key
    // below mutates — so decide everything first, then apply. It reads better
    // that way anyway: what to adopt is a pure function of the two trees.
    let mut plan: Vec<(usize, &str)> = Vec::new();
    for rn in &roam.nodes {
        // Rule 1, as `merge` states it: only a *persisted* id is an identity.
        // A Roam-side node without one carries a `local-N` placeholder from its
        // own parse, and writing that into the file gives the block an `id::`
        // the next `parse_outline` rejects as a placeholder shape.
        if !rn.persist_id || taken.contains(rn.id.as_str()) {
            continue;
        }
        let Some(created) = &rn.created_at else { continue };
        // First in document order. Ambiguity is not resolved, only bounded:
        // one local node per Roam node, one Roam node per local node.
        let Some(idx) = candidates
            .get_mut(&(rn.content.as_str(), created.as_str()))
            .and_then(VecDeque::pop_front)
        else {
            continue;
        };
        taken.insert(rn.id.as_str());
        plan.push((idx, rn.id.as_str()));
    }

    for (idx, new_id) in &plan {
        let old_id = std::mem::replace(&mut local.nodes[*idx].id, (*new_id).to_string());
        local.nodes[*idx].persist_id = true;
        // Children point at the parent's id string, so the re-key has to carry
        // them along — a detached margin note is exactly the loss this plugin
        // exists to prevent. The old id is a `local-N` placeholder unique to
        // this parse (a file's own `id:: local-N` is refused by
        // `parse_outline`), so nothing else in the tree answers to it.
        for n in local.nodes.iter_mut() {
            if n.parent.as_deref() == Some(old_id.as_str()) {
                n.parent = Some((*new_id).to_string());
            }
        }
    }
    plan.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::merge;
    use crate::outline::{parse_outline, serialize_outline};

    /// Both sides are written as `.note.md` text: a Roam tree is exactly what
    /// `convert_page` builds (every block carrying `id::` and `created::`), and
    /// a local tree is what an earlier JSON import left on disk.
    fn tree(text: &str) -> Tree {
        parse_outline(text)
    }

    fn by_content<'a>(t: &'a Tree, content: &str) -> &'a crate::outline::Node {
        t.nodes.iter().find(|n| n.content == content).expect("no such node")
    }

    /// Case 1, the shape actually in the user's vault: the JSON importer wrote
    /// the block with its `created::` and no `id::`; the CLI now syncs the same
    /// block from Roam. Before this pass the merge created a second copy.
    #[test]
    fn an_id_less_local_block_adopts_the_roam_uid_and_merges_as_one() {
        let mut local = tree("- ship the daily sync\n  created:: 2026-08-02T00:13:00.000Z\n");
        let roam = tree("- ship the daily sync\n  created:: 2026-08-02T00:13:00.000Z\n  id:: Km2vQx8pL\n");

        assert_eq!(adopt_ids(&mut local, &roam), 1);
        assert_eq!(local.nodes[0].id, "Km2vQx8pL");
        assert!(local.nodes[0].persist_id, "an adopted id must be written back to the file");

        let (out, st) = merge(&local, &roam);
        let text = serialize_outline(&out);
        assert_eq!(text.matches("- ship the daily sync").count(), 1, "the block doubled:\n{text}");
        assert_eq!(
            (st.created, st.kept_local, st.roam_gone_kept),
            (0, 0, 0),
            "the block is no longer a new Roam block plus an orphaned local one"
        );

        // …and once adopted it behaves like any other synced block: Roam's next
        // edit to it lands as an update, not as another copy.
        let edited = tree("- ship the daily sync, done\n  created:: 2026-08-02T00:13:00.000Z\n  id:: Km2vQx8pL\n");
        let (out2, st2) = merge(&out, &edited);
        assert_eq!((st2.created, st2.updated), (0, 1));
        assert_eq!(serialize_outline(&out2).matches("- ship the daily sync").count(), 1);
    }

    /// Case 2. Parentage is by id *string*, so a re-key that forgets the
    /// children detaches them — and a detached margin note is exactly the loss
    /// this whole plugin is written to avoid.
    #[test]
    fn children_of_an_adopted_node_still_resolve_to_it() {
        let mut local = tree(
            "- morning review\n  created:: 2026-08-02T00:12:00.000Z\n  - my own take on this one\n",
        );
        let roam = tree("- morning review\n  created:: 2026-08-02T00:12:00.000Z\n  id:: hCIv7Y63h\n");

        assert_eq!(adopt_ids(&mut local, &roam), 1);
        assert_eq!(
            by_content(&local, "my own take on this one").parent.as_deref(),
            Some("hCIv7Y63h")
        );
        assert_eq!(local.children_of(Some("hCIv7Y63h")).len(), 1);

        let (out, st) = merge(&local, &roam);
        assert_eq!(
            serialize_outline(&out),
            "- morning review\n  created:: 2026-08-02T00:12:00.000Z\n  id:: hCIv7Y63h\n  - my own take on this one\n"
        );
        assert_eq!(st.kept_local, 1, "the child is still the user's own block");
    }

    /// Case 3. The timestamp is the whole strength of the key: `- [ ] follow
    /// up` is written on twenty different days, and adopting on text alone
    /// would hand one day's block the uid of another's — after which Roam
    /// overwrites the wrong block's text on the next sync.
    #[test]
    fn a_different_created_is_a_different_block() {
        let mut local = tree("- follow up\n  created:: 2026-08-02T09:00:00.000Z\n");
        let roam = tree("- follow up\n  created:: 2026-08-03T09:00:00.000Z\n  id:: u1\n");
        assert_eq!(adopt_ids(&mut local, &roam), 0);
        assert!(!local.nodes[0].persist_id);
        assert_eq!(merge(&local, &roam).1.created, 1, "Roam's block is genuinely new here");
    }

    /// Case 4. No `created::` is no key at all — never "same text, close
    /// enough". A hand-written block quoting a Roam block's text would be
    /// swallowed by such a fallback.
    #[test]
    fn a_local_block_without_created_is_never_adopted() {
        let mut local = tree("- follow up\n");
        let roam = tree("- follow up\n  created:: 2026-08-02T09:00:00.000Z\n  id:: u1\n");
        assert_eq!(adopt_ids(&mut local, &roam), 0);
        assert!(!local.nodes[0].persist_id);

        // The mirror case: a Roam block Roam gave no create-time has no key
        // either, and must not adopt the first same-text local it sees.
        let mut local = tree("- follow up\n  created:: 2026-08-02T09:00:00.000Z\n");
        let roam = tree("- follow up\n  id:: u1\n");
        assert_eq!(adopt_ids(&mut local, &roam), 0);
    }

    /// Case 5. The vault holds pre-existing id-less twins (89 pairs in old
    /// daily notes, from before any of this). One of them adopts; the other
    /// stays an ordinary local block and is kept, not deleted and not re-keyed
    /// into a second node sharing the same id.
    #[test]
    fn two_id_less_twins_yield_exactly_one_adoption() {
        let text = "- twin\n  created:: 2026-08-02T09:00:00.000Z\n- twin\n  created:: 2026-08-02T09:00:00.000Z\n";
        let mut local = tree(text);
        let roam = tree("- twin\n  created:: 2026-08-02T09:00:00.000Z\n  id:: u1\n");

        assert_eq!(adopt_ids(&mut local, &roam), 1);
        assert_eq!(local.nodes[0].id, "u1");
        assert!(local.nodes[0].persist_id);
        assert!(!local.nodes[1].persist_id, "the second twin must stay a local block");
        assert_ne!(local.nodes[1].id, "u1", "two nodes must never share an id");

        let (out, st) = merge(&local, &roam);
        let out_text = serialize_outline(&out);
        assert_eq!(out_text.matches("- twin").count(), 2, "the user's block was deleted:\n{out_text}");
        assert_eq!(out_text.matches("id:: u1").count(), 1);
        assert_eq!((st.created, st.kept_local), (0, 1));
    }

    /// Case 6, the other direction: two Roam blocks with identical text and
    /// identical `created` and one local candidate. The local is adopted once;
    /// the second Roam block is new material and lands as its own node.
    #[test]
    fn one_local_cannot_be_adopted_by_two_roam_blocks() {
        let mut local = tree("- twin\n  created:: 2026-08-02T09:00:00.000Z\n");
        let roam = tree(
            "- twin\n  created:: 2026-08-02T09:00:00.000Z\n  id:: u1\n- twin\n  created:: 2026-08-02T09:00:00.000Z\n  id:: u2\n",
        );

        assert_eq!(adopt_ids(&mut local, &roam), 1);
        assert_eq!(local.nodes.len(), 1);
        assert_eq!(local.nodes[0].id, "u1");

        let (out, st) = merge(&local, &roam);
        let out_text = serialize_outline(&out);
        assert_eq!(out_text.matches("id:: u1").count(), 1);
        assert_eq!(out_text.matches("id:: u2").count(), 1);
        assert_eq!((st.created, st.kept_local), (1, 0));
    }

    /// Case 7. An `id::` already in the file is an identity an earlier sync
    /// established; re-keying it would hand this block another block's text on
    /// the next sync and orphan whatever the user hung under it.
    #[test]
    fn a_block_that_already_carries_an_id_is_never_re_keyed() {
        let mut local = tree("- twin\n  created:: 2026-08-02T09:00:00.000Z\n  id:: mine\n");
        let roam = tree("- twin\n  created:: 2026-08-02T09:00:00.000Z\n  id:: u1\n");
        assert_eq!(adopt_ids(&mut local, &roam), 0);
        assert_eq!(local.nodes[0].id, "mine");
    }

    /// Case 8. The count is the report — and a pass with nothing to do must be
    /// exactly that.
    #[test]
    fn the_count_is_the_number_adopted_and_zero_when_nothing_matches() {
        let mut nothing = tree("- something else\n  created:: 2026-08-02T09:00:00.000Z\n");
        let roam = tree("- a\n  created:: 2026-08-02T09:00:00.000Z\n  id:: u1\n- b\n  created:: 2026-08-02T09:30:00.000Z\n  id:: u2\n");
        assert_eq!(adopt_ids(&mut nothing, &roam), 0);
        assert_eq!(adopt_ids(&mut tree(""), &roam), 0);
        assert_eq!(adopt_ids(&mut tree("- a\n  created:: 2026-08-02T09:00:00.000Z\n"), &tree("")), 0);

        let mut both = tree("- a\n  created:: 2026-08-02T09:00:00.000Z\n- b\n  created:: 2026-08-02T09:30:00.000Z\n");
        assert_eq!(adopt_ids(&mut both, &roam), 2);
        assert_eq!(both.nodes[0].id, "u1");
        assert_eq!(both.nodes[1].id, "u2");
    }

    /// A page that has ALREADY doubled (the vault's current state: the id-less
    /// copy and the CLI-stamped copy side by side). Adoption must not hand the
    /// id-less copy an id a node in the same file already holds: parentage is
    /// by id string, so two nodes sharing one id make `children_of` gather both
    /// subtrees under each — the duplication hazard `parse_outline` itself
    /// guards against. The already-stamped copy keeps the identity; the id-less
    /// one stays a local block. (Healing a page that doubled *before* this fix
    /// means deleting one of the two, which is not this pass's call.)
    #[test]
    fn adoption_never_mints_an_id_the_file_already_holds() {
        let mut local = tree(
            "- twin\n  created:: 2026-08-02T09:00:00.000Z\n- twin\n  created:: 2026-08-02T09:00:00.000Z\n  id:: u1\n",
        );
        let roam = tree("- twin\n  created:: 2026-08-02T09:00:00.000Z\n  id:: u1\n");

        assert_eq!(adopt_ids(&mut local, &roam), 0);
        assert_eq!(local.nodes.iter().filter(|n| n.id == "u1").count(), 1);

        let (out, _) = merge(&local, &roam);
        let out_text = serialize_outline(&out);
        assert_eq!(out_text.matches("- twin").count(), 2, "nothing was deleted:\n{out_text}");
        assert_eq!(out_text.matches("id:: u1").count(), 1);
    }

    /// Only a *persisted* Roam id is an identity worth handing out — the same
    /// Rule 1 `merge` applies. A Roam-side node without one carries a
    /// `local-N` placeholder from its own parse, and adopting that would write
    /// `id:: local-3` into the user's file, where the next parse rejects it as
    /// a placeholder shape and the block loses its id all over again.
    #[test]
    fn a_roam_node_with_no_persisted_id_hands_out_nothing() {
        let mut local = tree("- twin\n  created:: 2026-08-02T09:00:00.000Z\n");
        let roam = tree("- twin\n  created:: 2026-08-02T09:00:00.000Z\n");
        assert_eq!(adopt_ids(&mut local, &roam), 0);
        assert!(!local.nodes[0].persist_id);
    }

    /// Multi-line content is compared whole. A shift-enter list or a fenced
    /// code block is one Roam block, and both import paths escape it the same
    /// way — comparing only the first line would collapse two different blocks
    /// that happen to share an opening line.
    #[test]
    fn the_whole_multi_line_content_is_the_key() {
        let mut local = tree("- shopping\n   - milk\n  created:: 2026-08-02T09:00:00.000Z\n");
        let roam = tree("- shopping\n   - eggs\n  created:: 2026-08-02T09:00:00.000Z\n  id:: u1\n");
        assert_eq!(adopt_ids(&mut local, &roam), 0);

        let roam = tree("- shopping\n   - milk\n  created:: 2026-08-02T09:00:00.000Z\n  id:: u1\n");
        assert_eq!(adopt_ids(&mut local, &roam), 1);
        assert_eq!(local.nodes[0].id, "u1");
    }

    /// Adoption is per node, and a node deeper in the file is reached the same
    /// way as a root one — including when its own parent was adopted in the
    /// same pass (the re-key must not strand the child's lookup).
    #[test]
    fn a_nested_pair_is_adopted_along_with_its_parent() {
        let mut local = tree(
            "- parent\n  created:: 2026-08-02T09:00:00.000Z\n  - child\n    created:: 2026-08-02T09:01:00.000Z\n",
        );
        let roam = tree(
            "- parent\n  created:: 2026-08-02T09:00:00.000Z\n  id:: u1\n  - child\n    created:: 2026-08-02T09:01:00.000Z\n    id:: u2\n",
        );
        assert_eq!(adopt_ids(&mut local, &roam), 2);
        assert_eq!(by_content(&local, "child").parent.as_deref(), Some("u1"));

        let (out, st) = merge(&local, &roam);
        assert_eq!(serialize_outline(&out), serialize_outline(&roam));
        assert_eq!((st.created, st.kept_local), (0, 0));
    }
}
