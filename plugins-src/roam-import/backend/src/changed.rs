//! Merge the two edit-time dimensions (`roam_cli::{changed_blocks_query,
//! changed_pages_query}`) into one change list. Neither query alone is a
//! superset of the other: the block dimension finds content edits but misses
//! a page that was renamed or created without its blocks changing; the page
//! entity dimension finds renames and creation but, for a daily note, its
//! `:edit/time` is the page-creation instant — so it misses almost every
//! content edit. The union, taking the later timestamp per uid, is what
//! "changed since the watermark" actually means.
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub uid: String,
    pub edited: i64,
}

/// Fold one query's rows into `acc`, keeping the later `edited` per uid. A
/// row missing `uid` or `edited` (or with `edited` not representable as an
/// integer — e.g. a float or a string) is skipped rather than failing the
/// whole run; the other rows still matter.
fn fold_rows(payload: &serde_json::Value, acc: &mut HashMap<String, i64>) -> Result<(), String> {
    let rows = payload
        .as_array()
        .ok_or_else(|| "changed query did not return an array".to_string())?;
    for row in rows {
        let Some(uid) = row.get("uid").and_then(|v| v.as_str()) else { continue };
        let Some(edited) = row.get("edited").and_then(|v| v.as_i64()) else { continue };
        acc.entry(uid.to_string())
            .and_modify(|e| *e = (*e).max(edited))
            .or_insert(edited);
    }
    Ok(())
}

/// Union the block and page dimensions, taking the later timestamp per uid,
/// ascending by `edited` — ascending because that is what makes the watermark
/// resumable: it advances from the front of this list as pages succeed. It
/// advances a whole *timestamp* at a time rather than a page at a time,
/// because `edited` is not unique; the rule and the reason live in
/// [`crate::incremental`] (design doc §5), which re-sorts by `(edited, uid)`
/// so a batch's order is reproducible down to ties.
pub fn merge_changed(
    blocks: &serde_json::Value,
    pages: &serde_json::Value,
) -> Result<Vec<Changed>, String> {
    let mut acc: HashMap<String, i64> = HashMap::new();
    fold_rows(blocks, &mut acc)?;
    fold_rows(pages, &mut acc)?;
    let mut out: Vec<Changed> =
        acc.into_iter().map(|(uid, edited)| Changed { uid, edited }).collect();
    out.sort_by_key(|c| c.edited);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merges_both_dimensions_taking_the_later_timestamp() {
        let blocks = json!([{ "uid": "d", "edited": 300 }, { "uid": "both", "edited": 100 }]);
        let pages  = json!([{ "uid": "w", "edited": 200 }, { "uid": "both", "edited": 900 }]);
        let got = merge_changed(&blocks, &pages).unwrap();
        assert_eq!(got, vec![
            Changed { uid: "w".into(), edited: 200 },
            Changed { uid: "d".into(), edited: 300 },
            Changed { uid: "both".into(), edited: 900 },
        ], "ascending by edited, and `both` takes 900 not 100");
    }

    #[test]
    fn empty_on_both_sides_is_an_empty_list_not_an_error() {
        assert!(merge_changed(&json!([]), &json!([])).unwrap().is_empty());
    }

    #[test]
    fn a_uid_in_only_one_dimension_still_appears() {
        let got = merge_changed(&json!([{ "uid": "a", "edited": 1 }]), &json!([])).unwrap();
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn a_row_missing_uid_or_edited_is_skipped_rather_than_failing_the_run() {
        let blocks = json!([{ "edited": 1 }, { "uid": "ok", "edited": 2 }, { "uid": "no-time" }]);
        let got = merge_changed(&blocks, &json!([])).unwrap();
        assert_eq!(got, vec![Changed { uid: "ok".into(), edited: 2 }]);
    }

    #[test]
    fn a_non_array_payload_is_an_error() {
        assert!(merge_changed(&json!({"error": "x"}), &json!([])).is_err());
    }
}
