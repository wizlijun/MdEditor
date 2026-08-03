//! The shape `roam datalog-query` returns for a recursive page pull. It is
//! deliberately the same shape as Roam's JSON export (`RoamPage`/`RoamBlock` in
//! the TS importer) with ONE difference: datalog does not guarantee child
//! order, so `order` must be read and sorted on.
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RoamBlock {
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub string: String,
    #[serde(default)]
    pub order: i64,
    #[serde(default)]
    pub heading: Option<u8>,
    #[serde(default, rename = "create-time")]
    pub create_time: Option<i64>,
    #[serde(default, rename = "edit-time")]
    pub edit_time: Option<i64>,
    #[serde(default)]
    pub children: Vec<RoamBlock>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RoamPage {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default, rename = "create-time")]
    pub create_time: Option<i64>,
    #[serde(default, rename = "edit-time")]
    pub edit_time: Option<i64>,
    #[serde(default)]
    pub children: Vec<RoamBlock>,
}

fn sort_tree(blocks: &mut Vec<RoamBlock>) {
    blocks.sort_by_key(|b| b.order);
    for b in blocks.iter_mut() {
        sort_tree(&mut b.children);
    }
}

/// `[[page]]` → the page, with every level order-sorted. An empty relation
/// means Roam has no daily page for that day (NOT an error).
pub fn parse_day_result(v: &serde_json::Value) -> Result<Option<RoamPage>, String> {
    let Some(first) = v.as_array().and_then(|rows| rows.first()) else { return Ok(None) };
    let obj = match first {
        serde_json::Value::Array(cols) => match cols.first() {
            Some(o) => o,
            None => return Ok(None),
        },
        other => other,
    };
    if obj.is_null() { return Ok(None); }
    let mut page: RoamPage =
        serde_json::from_value(obj.clone()).map_err(|e| format!("unreadable page: {e}"))?;
    sort_tree(&mut page.children);
    Ok(Some(page))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_result_means_no_page_that_day() {
        assert_eq!(parse_day_result(&json!([])).unwrap(), None);
    }

    #[test]
    fn reads_title_uid_and_times() {
        let v = json!([[{
            "title": "August 2nd, 2026", "uid": "08-02-2026",
            "create-time": 1785600005019i64, "edit-time": 1785704684051i64
        }]]);
        let p = parse_day_result(&v).unwrap().unwrap();
        assert_eq!(p.title, "August 2nd, 2026");
        assert_eq!(p.uid.as_deref(), Some("08-02-2026"));
        assert_eq!(p.create_time, Some(1785600005019));
        assert!(p.children.is_empty());
    }

    #[test]
    fn sorts_children_by_order_at_every_level() {
        let v = json!([[{
            "title": "August 2nd, 2026", "uid": "08-02-2026",
            "children": [
                { "uid": "b", "string": "second", "order": 1,
                  "children": [ { "uid": "b2", "string": "y", "order": 1 },
                                { "uid": "b1", "string": "x", "order": 0 } ] },
                { "uid": "a", "string": "first", "order": 0 }
            ]
        }]]);
        let p = parse_day_result(&v).unwrap().unwrap();
        assert_eq!(p.children.iter().map(|c| c.string.as_str()).collect::<Vec<_>>(), vec!["first", "second"]);
        assert_eq!(p.children[1].children.iter().map(|c| c.string.as_str()).collect::<Vec<_>>(), vec!["x", "y"]);
    }

    #[test]
    fn missing_string_and_order_default_instead_of_failing() {
        let v = json!([[{ "title": "t", "uid": "u", "children": [ { "uid": "a" } ] }]]);
        let p = parse_day_result(&v).unwrap().unwrap();
        assert_eq!(p.children[0].string, "");
    }

    #[test]
    fn keeps_heading_level() {
        let v = json!([[{ "title": "t", "uid": "u",
                          "children": [ { "uid": "a", "string": "H", "heading": 2 } ] }]]);
        assert_eq!(parse_day_result(&v).unwrap().unwrap().children[0].heading, Some(2));
    }
}
