//! `.note.md` outline model + parser + serializer + front-matter touch-up.
//! Ported from `src/lib/outline/markdown.ts` (`parseOutline`/`serializeOutline`/
//! `splitFrontmatterBlock`) — the host's companion-file format. Task 6 reads a
//! vault `.note.md`, merges Roam blocks into the tree, and writes it back, so
//! this must parse/serialize byte-identically to the TS side; the golden
//! fixture in Task 7 is what catches drift, keep the two in step.
use regex::Regex;
use std::sync::OnceLock;

/// One outline bullet. `parent`/`order`/`content` mirror the TS `OutlineNode`
/// shape; `source` stays a plain string (not an enum) because the property
/// whitelist below is itself the source of truth for valid values.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub parent: Option<String>,
    pub order: i64,
    pub content: String,
    pub collapsed: bool,
    pub source: String,
    pub anchor_line: Option<i64>,
    pub status: Option<String>,
    pub answered_at: Option<String>,
    pub answered_by: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    /// `id::` was explicitly present in the file (or the node was renamed to
    /// one). Only these ids are ever written back — placeholder `local-N`
    /// ids exist purely for this-process bookkeeping (Task 6 aligns Roam
    /// blocks by id and positions local blocks by neighbours).
    pub persist_id: bool,
}

pub struct Tree {
    pub frontmatter: Option<String>,
    pub nodes: Vec<Node>,
}

impl Tree {
    /// Children of `parent` (root when `None`), ascending by `order` —
    /// mirrors `childrenOf` in `model.ts`.
    pub fn children_of(&self, parent: Option<&str>) -> Vec<&Node> {
        let mut out: Vec<&Node> = self
            .nodes
            .iter()
            .filter(|n| n.parent.as_deref() == parent)
            .collect();
        out.sort_by_key(|n| n.order);
        out
    }
}

/// File-head YAML front-matter block. Must start at byte 0, `---` alone on
/// its own line. Mirrors `FM_RE` in markdown.ts exactly (including that `^`
/// there is *not* multiline — it only ever matches at the string start).
fn frontmatter_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)").unwrap())
}

pub fn split_frontmatter_block(text: &str) -> (Option<String>, String) {
    match frontmatter_pattern().captures(text) {
        Some(caps) => {
            let whole = caps.get(0).unwrap();
            let fm = caps.get(1).unwrap().as_str().to_string();
            (Some(fm), text[whole.end()..].to_string())
        }
        None => (None, text.to_string()),
    }
}

fn bullet_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^((?:  )*)- (.*)$").unwrap())
}

fn prop_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(type|line|id|collapsed|created|updated|status|answered|by):: (.*)$")
            .unwrap()
    })
}

/// Leading run of backticks, with no requirement on what follows — used to
/// detect a bullet's first line *opening* a raw fence. `pub(crate)` so
/// `convert::close_dangling_fence` asks the parser itself what counts as a
/// fence rather than keeping a second, driftable copy of the rule.
pub(crate) fn fence_open_len(s: &str) -> Option<usize> {
    let n = s.chars().take_while(|&c| c == '`').count();
    if n >= 3 { Some(n) } else { None }
}

/// A line that is *only* backticks (plus trailing whitespace) — closes a raw
/// fence when its run is at least as long as the one that opened it. See
/// `fence_open_len` for why this is `pub(crate)`.
pub(crate) fn fence_close_len(s: &str) -> Option<usize> {
    let n = s.chars().take_while(|&c| c == '`').count();
    if n < 3 {
        return None;
    }
    if s[n..].chars().all(|c| c.is_whitespace()) { Some(n) } else { None }
}

/// Strip up to `max` leading spaces (not necessarily that many — tolerates a
/// hand-edited file with less indentation than expected). Mirrors the JS
/// fallback `raw.replace(new RegExp(`^ {0,${max}}`), '')`.
fn strip_leading_spaces(raw: &str, max: usize) -> &str {
    let mut n = 0;
    for c in raw.chars() {
        if c == ' ' && n < max {
            n += 1;
        } else {
            break;
        }
    }
    &raw[n..]
}

// orderCounters.length = depth + 1 (markdown.ts:76) is the same sparse-array
// truncate-or-pad as the parent stack below, but every padded slot here is
// read back only through this same function (nothing else ever indexes
// order_counters), and a freshly padded slot is always treated as "start a
// new counter at 0" regardless of whether it's a real JS `undefined` hole or
// our eager `-100` sentinel. So padding every intermediate slot with -100
// up front (instead of leaving true holes and defaulting lazily on read) is
// observationally identical — unlike the parent stack, there is no
// "hole vs. real value" distinction that later code can observe.
fn next_order(order_counters: &mut Vec<i64>, depth: usize) -> i64 {
    if order_counters.len() > depth + 1 {
        order_counters.truncate(depth + 1);
    }
    while order_counters.len() <= depth {
        order_counters.push(-100);
    }
    order_counters[depth] += 100;
    order_counters[depth]
}

#[allow(clippy::too_many_arguments)]
fn push_node(
    tree: &mut Tree,
    stack: &mut Vec<Option<usize>>,
    order_counters: &mut Vec<i64>,
    local_counter: &mut u64,
    depth: usize,
    content: String,
) -> usize {
    // stack[depth - 1] ?? null (markdown.ts:82) — a positional read, not a
    // "most recently pushed" read. A skipped depth (e.g. 0 then 2) leaves
    // stack[1] as a hole, so a node pushed at depth 2 parents at the root,
    // not under the depth-0 node. get(...).flatten() reproduces both the
    // "index doesn't exist yet" and "index exists but is a hole" cases as None.
    let parent = if depth > 0 {
        stack.get(depth - 1).copied().flatten().map(|idx| tree.nodes[idx].id.clone())
    } else {
        None
    };
    *local_counter += 1;
    let node = Node {
        id: format!("local-{local_counter}"),
        parent,
        order: next_order(order_counters, depth),
        content,
        collapsed: false,
        source: "manual".to_string(),
        anchor_line: None,
        status: None,
        answered_at: None,
        answered_by: None,
        created_at: None,
        updated_at: None,
        persist_id: false,
    };
    tree.nodes.push(node);
    let idx = tree.nodes.len() - 1;
    // stack.length = depth; stack[depth] = node (markdown.ts:92-93) — resize
    // to exactly `depth` elements (truncating OR padding with holes), then
    // set index `depth`. Padding-with-holes is the part a dense
    // truncate+push loses: it must leave real gaps, not compact them away.
    stack.resize(depth, None);
    stack.push(Some(idx));
    idx
}

/// Apply one whitelisted `key:: value` property line to `tree.nodes[idx]`.
/// `value` has already had trailing whitespace stripped (tolerate hard-wrap
/// trailing spaces from editors/formatters without dropping the property).
fn apply_prop(tree: &mut Tree, idx: usize, key: &str, value: &str) {
    let value = value.trim_end().to_string();
    // A vault file is hand-editable, so an `id::` that is not a usable
    // identity — a repeated one (a copied bullet, a botched three-way merge),
    // or one shaped like our own placeholder — is expected input. Taking it
    // would let two nodes share an id string, and since parentage is by id
    // string, the tree stops being a tree: a nested twin becomes its own
    // parent and every walk (`serialize_outline` here, `merge`'s two)
    // recurses until the stack dies, while a flat twin makes the walk emit
    // some later subtree under both of them, duplicating the user's blocks
    // into their own vault.
    //
    // Two directions, and both are needed. The scan catches an id that some
    // *earlier* node already holds. The `local-N` shape catches the reverse:
    // `id:: local-2` collides with nothing at the time it is applied, and
    // then `push_node` hands the placeholder `local-2` to a node parsed
    // later. (Not a hazard the TS host shares — its placeholders are UUIDs.)
    // Scanning is O(nodes) per `id::` line, which is nothing at daily-note
    // sizes and is only paid by files that carry ids at all.
    let id_already_taken = key == "id"
        && (value.strip_prefix("local-").is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
            || tree.nodes.iter().enumerate().any(|(i, n)| i != idx && n.id == value));
    let node = &mut tree.nodes[idx];
    match key {
        "type" => {
            if matches!(
                value.as_str(),
                "toc" | "highlight" | "wikilink" | "annotation" | "note" | "question" | "answer"
            ) {
                node.source = value;
            }
        }
        "line" => {
            if let Ok(n) = value.parse::<i64>() {
                node.anchor_line = Some(n);
            }
        }
        "collapsed" => node.collapsed = value == "true",
        "created" => node.created_at = Some(value),
        "updated" => node.updated_at = Some(value),
        "status" => {
            if matches!(value.as_str(), "open" | "answered" | "closed" | "adopted") {
                node.status = Some(value);
                // status:: is question-only; a valid status self-heals a
                // manual/note node into a question even if type:: is
                // missing/damaged — keeps type/status written back as a pair.
                if node.source == "manual" || node.source == "note" {
                    node.source = "question".to_string();
                }
            }
        }
        "answered" => node.answered_at = Some(value),
        "by" => node.answered_by = Some(value),
        // A duplicate id is no identity at all, so the node keeps its
        // placeholder and `persist_id` stays false: the `id::` line does not
        // survive the round-trip, but the block and its children do, and the
        // file stays walkable. (Dropping the *node* instead would lose the
        // user's content, which is never the right trade.)
        "id" if !id_already_taken => {
            // Invariant: id:: precedes any children of this node, so no
            // already-pushed node can reference the old id as its parent —
            // renaming in place (no id->index map needed) is safe.
            node.id = value;
            node.persist_id = true;
        }
        _ => {}
    }
}

pub fn parse_outline(text: &str) -> Tree {
    let (frontmatter, body) = split_frontmatter_block(text);
    let mut tree = Tree { frontmatter, nodes: Vec::new() };

    let mut stack: Vec<Option<usize>> = Vec::new();
    // (node index, depth) of the most recently pushed node.
    let mut current: Option<(usize, usize)> = None;
    let mut order_counters: Vec<i64> = Vec::new();
    // >0 means inside an answer fence (raw mode): every line is taken
    // verbatim, bullets/properties are not recognized.
    let mut fence_len: usize = 0;
    let mut local_counter: u64 = 0;

    let mut lines: Vec<&str> = body.split('\n').collect();
    // A trailing \n produces one extra empty split element that is a
    // structural artifact, not a semantic blank line; drop it. The regular
    // path already skips blank lines and is unaffected — only raw fence mode
    // (especially an unterminated fence) would otherwise absorb it into the
    // answer body.
    if lines.last() == Some(&"") {
        lines.pop();
    }

    for raw in lines {
        if fence_len > 0 {
            if let Some((idx, depth)) = current {
                let cont_indent_len = depth * 2 + 2;
                let cont_indent = " ".repeat(cont_indent_len);
                let line = if raw.starts_with(&cont_indent) {
                    &raw[cont_indent_len..]
                } else {
                    strip_leading_spaces(raw, cont_indent_len)
                };
                {
                    let node = &mut tree.nodes[idx];
                    node.content.push('\n');
                    node.content.push_str(line);
                }
                if let Some(close_len) = fence_close_len(line) {
                    if close_len >= fence_len {
                        fence_len = 0;
                    }
                }
                continue;
            }
        }
        if raw.trim().is_empty() {
            continue;
        }
        if let Some(caps) = bullet_pattern().captures(raw) {
            let depth = caps[1].len() / 2;
            let rest = caps[2].to_string();
            if let Some(open_len) = fence_open_len(&rest) {
                fence_len = open_len;
            }
            let idx = push_node(&mut tree, &mut stack, &mut order_counters, &mut local_counter, depth, rest);
            current = Some((idx, depth));
            continue;
        }
        if let Some((idx, depth)) = current {
            let cont_indent_len = depth * 2 + 2;
            let cont_indent = " ".repeat(cont_indent_len);
            if raw.starts_with(&cont_indent) {
                let body_line = &raw[cont_indent_len..];
                if let Some(caps) = prop_pattern().captures(body_line) {
                    let key = caps[1].to_string();
                    let value = caps[2].to_string();
                    apply_prop(&mut tree, idx, &key, &value);
                } else {
                    let node = &mut tree.nodes[idx];
                    node.content.push('\n');
                    node.content.push_str(body_line);
                }
                continue;
            }
        }
        // Unclassifiable line: demote to a root-level manual node (spec: never drop content).
        let idx = push_node(&mut tree, &mut stack, &mut order_counters, &mut local_counter, 0, raw.trim().to_string());
        current = Some((idx, 0));
    }

    tree
}

pub fn serialize_outline(tree: &Tree) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(fm) = &tree.frontmatter {
        lines.push("---".to_string());
        lines.push(fm.clone());
        lines.push("---".to_string());
    }

    fn walk(tree: &Tree, lines: &mut Vec<String>, parent: Option<&str>, depth: usize) {
        for n in tree.children_of(parent) {
            let indent = "  ".repeat(depth);
            let mut content_lines = n.content.split('\n');
            lines.push(format!("{indent}- {}", content_lines.next().unwrap_or("")));
            // Blank continuation lines are written as truly empty strings, not
            // indented whitespace: blank lines inside answer prose are semantic.
            for cont in content_lines {
                lines.push(if cont.is_empty() { String::new() } else { format!("{indent}  {cont}") });
            }
            if n.source != "manual" {
                lines.push(format!("{indent}  type:: {}", n.source));
                if let Some(al) = n.anchor_line {
                    lines.push(format!("{indent}  line:: {al}"));
                }
                if n.source == "question" {
                    lines.push(format!("{indent}  status:: {}", n.status.as_deref().unwrap_or("open")));
                }
            }
            if let Some(c) = &n.created_at {
                lines.push(format!("{indent}  created:: {c}"));
            }
            if let Some(u) = &n.updated_at {
                lines.push(format!("{indent}  updated:: {u}"));
            }
            if let Some(a) = &n.answered_at {
                lines.push(format!("{indent}  answered:: {a}"));
            }
            if let Some(b) = &n.answered_by {
                lines.push(format!("{indent}  by:: {b}"));
            }
            if n.persist_id {
                lines.push(format!("{indent}  id:: {}", n.id));
            }
            if n.collapsed {
                lines.push(format!("{indent}  collapsed:: true"));
            }
            walk(tree, lines, Some(n.id.as_str()), depth + 1);
        }
    }
    walk(tree, &mut lines, None, 0);

    if lines.is_empty() { String::new() } else { lines.join("\n") + "\n" }
}

/// Read one **top-level** `key: value` out of a front-matter block. Line-based
/// and deliberately using the same key test as `touch_frontmatter`, so the two
/// always agree on whether a key is there: `frontmatter_value(.., "updated")`
/// answering "yes" for an `updated:` nested under some other key, where
/// `touch_frontmatter` would then append a *top-level* one, is the sync's
/// no-op fast path reading a value that is not the one it is about to replace.
pub fn frontmatter_value(raw: Option<&str>, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    raw?.lines()
        .find(|l| l.starts_with(&prefix) && is_top_level_key(l))
        .map(|l| l[prefix.len()..].trim().to_string())
}

/// What the host's `yaml`-backed reader would make of a front-matter block.
/// Decided line by line, like everything else in this file — no YAML crate —
/// but the three outcomes are exactly the three branches of
/// `touchFrontmatter` in `src/lib/outline/frontmatter.ts`.
enum FmShape {
    /// Nothing but blank lines (`doc.contents == null`): a fresh mapping.
    Blank,
    /// Nothing but comments (`doc.contents == null` too): a fresh mapping,
    /// written *after* the comments.
    CommentsOnly,
    /// At least one top-level `key:` — a mapping, safe to edit.
    Mapping,
    /// Anything else: a scalar (`just a sentence`), a sequence, a flow
    /// collection. `isMap` is false there and the host returns the block
    /// untouched.
    Other,
}

/// Is this line a top-level `key:`/`key: value` — the shape that makes a
/// block-mapping? Deliberately conservative, and conservative in the same
/// direction the host is: an indented line, a comment, a sequence item, a flow
/// collection, or `key:value` with no space (a plain *scalar* to YAML, not a
/// mapping) all read as "not a key", which leaves the block alone rather than
/// appending to something that cannot take keys.
fn is_top_level_key(line: &str) -> bool {
    let Some(first) = line.chars().next() else { return false };
    if first.is_whitespace() || first == '#' || first == '{' || first == '[' {
        return false;
    }
    if line == "-" || line.starts_with("- ") {
        return false;
    }
    match line.find(':') {
        None | Some(0) => false,
        Some(i) => line[i + 1..].chars().next().is_none_or(char::is_whitespace),
    }
}

fn fm_shape(raw: &str) -> FmShape {
    let mut any = false;
    let mut only_comments = true;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        any = true;
        if is_top_level_key(line) {
            return FmShape::Mapping;
        }
        if !trimmed.starts_with('#') {
            only_comments = false;
        }
    }
    match (any, only_comments) {
        (false, _) => FmShape::Blank,
        (true, true) => FmShape::CommentsOnly,
        (true, false) => FmShape::Other,
    }
}

/// The OKF v0.2 §4.1 `type` this plugin stamps. It only ever writes
/// `<daily_dir>/<yyyy>/<yyyy-MM-dd>.note.md`, which is what the host's
/// `outlineConceptType` (src/lib/outline/frontmatter.ts) maps to
/// `CONCEPT_TYPE.dailyNote` — *not* `Outline Note`, the default a caller that
/// says nothing would get. Registered here as a constant for the same reason
/// `CONCEPT_TYPE` exists in src/lib/okf/concept.ts: one spelling, in one place.
pub const CONCEPT_TYPE_DAILY_NOTE: &str = "Daily Note";

/// The OKF v0.2 §4.1 `type` for a synced Roam wikipage — the non-daily
/// counterpart to [`CONCEPT_TYPE_DAILY_NOTE`] above, and `CONCEPT_TYPE.wikiPage`
/// in `src/lib/okf/concept.ts`. Same reasoning: one spelling, in one place.
pub const CONCEPT_TYPE_WIKI_PAGE: &str = "Wiki Page";

/// Refresh a companion file's front-matter without a YAML crate: unknown
/// keys and their order must survive untouched (round-tripping a
/// hand-edited or third-party-tool-written file is a hard requirement, not
/// a nicety). `concept_type` (OKF §4.1 REQUIRED), `title` and `created` are
/// filled in only if absent — appended at the end, in that order, which is the
/// order `touchConceptFrontmatter` iterates its keys in; `updated` is replaced
/// in place if present, appended otherwise. On a block with no keys at all the
/// same rule reads as "`type` first", since there is nothing to append after.
///
/// A block that is **not a mapping** comes back untouched. This function does
/// not write the notes it edits — it writes into whatever daily note is
/// already on disk, and a `.note.md` is hand-edited and agent-edited
/// (file-over-app). Appending `title:`/`created:`/`updated:` after
/// `---\njust a sentence\n---` produces a block the host's own `yaml`-backed
/// reader then refuses as a map, i.e. this sync corrupting front-matter it did
/// not write. `touchFrontmatter` bails out on exactly this case
/// (`else if (!isMap(doc.contents)) return raw`); so does this.
pub fn touch_frontmatter(
    raw: Option<&str>,
    concept_type: &str,
    title: &str,
    created: &str,
    now: &str,
) -> String {
    let raw = raw.unwrap_or("");
    let mut lines: Vec<String> = match fm_shape(raw) {
        FmShape::Other => return raw.to_string(),
        // Whitespace-only reads as "no front-matter at all", as it does on the
        // host — the blank lines are not content to preserve.
        FmShape::Blank => Vec::new(),
        FmShape::Mapping => raw.lines().map(|l| l.to_string()).collect(),
        FmShape::CommentsOnly => {
            let mut lines: Vec<String> = raw.lines().map(|l| l.to_string()).collect();
            // The host's serializer separates a leading comment from the
            // mapping it creates beneath it with a blank line; match it, or
            // the two sides drift on a comment-only block.
            lines.push(String::new());
            lines
        }
    };

    // Top-level only, like the host's `doc.has(key)`. An indented `type:` or
    // `title:` belongs to some other key's value — OKF's own `sources:` and
    // `generated:` both nest exactly those names (§5.1/§5.2) — and reading it
    // as "the key is already there" is how the required top-level `type:`
    // would silently not get stamped on such a file.
    let has_key = |lines: &[String], key: &str| {
        let prefix = format!("{key}:");
        lines.iter().any(|l| l.starts_with(&prefix) && is_top_level_key(l))
    };

    if !has_key(&lines, "type") {
        lines.push(format!("type: {concept_type}"));
    }
    if !has_key(&lines, "title") {
        lines.push(format!("title: {title}"));
    }
    if !has_key(&lines, "created") {
        lines.push(format!("created: {created}"));
    }

    let updated_line = format!("updated: {now}");
    match lines.iter().position(|l| l.starts_with("updated:") && is_top_level_key(l)) {
        Some(pos) => lines[pos] = updated_line,
        None => lines.push(updated_line),
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_plain_outline() {
        let text = "---\ntitle: 2026-08-02\n---\n- a\n  - b\n- c\n";
        assert_eq!(serialize_outline(&parse_outline(text)), text);
    }

    #[test]
    fn round_trips_properties_in_canonical_order() {
        let text = "- hello\n  created:: 2026-08-02T00:00:00.000Z\n  updated:: 2026-08-02T01:00:00.000Z\n  id:: abc\n  collapsed:: true\n";
        assert_eq!(serialize_outline(&parse_outline(text)), text);
    }

    #[test]
    fn keeps_id_and_marks_it_persisted() {
        let t = parse_outline("- hello\n  id:: abc\n");
        assert_eq!(t.nodes[0].id, "abc");
        assert!(t.nodes[0].persist_id);
    }

    #[test]
    fn a_node_without_id_is_not_persisted() {
        let t = parse_outline("- hello\n");
        assert!(!t.nodes[0].persist_id);
        assert_eq!(serialize_outline(&t), "- hello\n");
    }

    #[test]
    fn multi_line_content_survives() {
        let text = "- first\n  second\n";
        let t = parse_outline(text);
        assert_eq!(t.nodes[0].content, "first\nsecond");
        assert_eq!(serialize_outline(&t), text);
    }

    #[test]
    fn answer_fences_are_taken_raw() {
        let text = "- ```\n  type:: answer\n  still inside the fence\n  ```\n";
        let t = parse_outline(text);
        assert_eq!(t.nodes.len(), 1);
        assert!(t.nodes[0].content.contains("type:: answer"));
        assert_eq!(t.nodes[0].source, "manual");
    }

    #[test]
    fn typed_nodes_round_trip() {
        let text = "- ask me\n  type:: question\n  status:: open\n";
        assert_eq!(serialize_outline(&parse_outline(text)), text);
    }

    #[test]
    fn children_are_ordered() {
        let t = parse_outline("- a\n- b\n- c\n");
        let kids: Vec<&str> = t.children_of(None).iter().map(|n| n.content.as_str()).collect();
        assert_eq!(kids, vec!["a", "b", "c"]);
    }

    /// Regression: `0 -> 2 -> 3 -> 1` depth sequence (e.g. Tab-indented at 4
    /// spaces in an external editor, so a bullet lands at depth 2 with no
    /// depth-1 sibling ever written). The parent stack must be positionally
    /// indexed like `markdown.ts`'s sparse array — a dense
    /// truncate-then-push flattens `c` to the root instead of nesting it
    /// under `b`, silently destroying structure rather than merely
    /// re-indenting it.
    #[test]
    fn skipped_depth_levels_still_nest_by_stack_position() {
        let text = "- a\n    - b\n      - c\n  - d\n";
        let t = parse_outline(text);
        assert_eq!(t.nodes.len(), 4);
        let by_content = |c: &str| t.nodes.iter().find(|n| n.content == c).unwrap();
        let a = by_content("a");
        let b = by_content("b");
        let c = by_content("c");
        let d = by_content("d");

        // depth-1 slot was never written when b (depth 2) was pushed, so b
        // reads as root — not nested under a.
        assert_eq!(b.parent, None);
        // c (depth 3) reads stack[2], which b just set — nests under b.
        assert_eq!(c.parent.as_deref(), Some(b.id.as_str()));
        // returning to depth 1 reads stack[0] (a) again — re-parents under a.
        assert_eq!(d.parent.as_deref(), Some(a.id.as_str()));
        assert_eq!(a.parent, None);

        assert_eq!(serialize_outline(&t), "- a\n  - d\n- b\n  - c\n");
    }

    /// A vault file is allowed to be hand-edited (file-over-app), so a
    /// repeated `id::` is expected, not exotic — a careless copy/paste of a
    /// bullet is enough. Re-keying the second node to an id already in the
    /// tree makes a node whose parent is itself, and `serialize_outline`'s
    /// walk then recurses until the stack dies, taking the user's file with
    /// it. The duplicate keeps its placeholder id instead: it loses the
    /// `id::` line (it was never a valid identity), never its content.
    #[test]
    fn a_duplicate_id_under_its_own_twin_does_not_recurse_forever() {
        let t = parse_outline("- parent\n  id:: dup\n  - child\n    id:: dup\n");
        let child = t.nodes.iter().find(|n| n.content == "child").unwrap();
        assert_ne!(child.id, "dup", "a second node must not claim an id already in the tree");
        assert_eq!(child.parent.as_deref(), Some("dup"));
        assert!(!child.persist_id);
        assert_eq!(serialize_outline(&t), "- parent\n  id:: dup\n  - child\n");
    }

    /// The other half of the same hazard, and the one the "is this id already
    /// in the tree?" check alone cannot see: a file may contain the
    /// placeholder shape itself. `id:: local-2` is applied while node `b`
    /// does not exist yet, so nothing collides — and then `b` is handed the
    /// placeholder `local-2` and becomes its own parent.
    #[test]
    fn an_id_that_looks_like_a_placeholder_cannot_steal_a_later_nodes_id() {
        let t = parse_outline("- a\n  id:: local-2\n  - b\n");
        let b = t.nodes.iter().find(|n| n.content == "b").unwrap();
        assert_ne!(b.parent.as_deref(), Some(b.id.as_str()), "a node must not parent itself");
        assert_eq!(serialize_outline(&t), "- a\n  - b\n");
    }

    /// Same root cause, quieter symptom: `d`'s parent id string matches two
    /// nodes, so every walk emits it under both — the user's block is
    /// duplicated into their own vault (and `merge` copies the duplication
    /// through, since it walks by the same parent-id string).
    #[test]
    fn an_id_that_looks_like_a_placeholder_cannot_duplicate_a_later_subtree() {
        let text = "- a\n  id:: local-3\n- b\n- c\n  - d\n";
        let out = serialize_outline(&parse_outline(text));
        assert_eq!(out.matches("- d").count(), 1, "d must be emitted once:\n{out}");
        assert_eq!(out, "- a\n- b\n- c\n  - d\n");
    }

    #[test]
    fn a_duplicate_id_between_siblings_keeps_both_blocks() {
        let t = parse_outline("- first\n  id:: dup\n- second\n  id:: dup\n");
        assert_eq!(t.nodes.len(), 2);
        assert_eq!(t.nodes[0].id, "dup");
        assert_ne!(t.nodes[1].id, "dup");
        assert_eq!(serialize_outline(&t), "- first\n  id:: dup\n- second\n");
    }

    #[test]
    fn frontmatter_touch_fills_and_refreshes() {
        let fm = touch_frontmatter(
            None,
            CONCEPT_TYPE_DAILY_NOTE,
            "2026-08-02",
            "2026-08-02T00:00:00.000Z",
            "2026-08-03T09:00:00.000Z",
        );
        assert!(fm.contains("title: 2026-08-02"));
        assert!(fm.contains("created: 2026-08-02T00:00:00.000Z"));
        assert!(fm.contains("updated: 2026-08-03T09:00:00.000Z"));
    }

    /// OKF v0.2 §4.1: `type` is REQUIRED on every concept document, so a note
    /// this plugin writes must carry one — as `Daily Note`, the type the host
    /// derives from the daily folder, not the `Outline Note` default.
    #[test]
    fn frontmatter_touch_stamps_the_okf_type_first_on_a_fresh_block() {
        let fm = touch_frontmatter(None, CONCEPT_TYPE_DAILY_NOTE, "T", "C", "N");
        assert_eq!(fm, "type: Daily Note\ntitle: T\ncreated: C\nupdated: N");
    }

    /// …and a type the file already declares is never rewritten (an existing
    /// key's value is not ours to change), while an *existing* block gets the
    /// missing key appended, not prepended — the host's key order survives.
    #[test]
    fn an_existing_type_is_kept_and_a_missing_one_is_appended() {
        let kept = touch_frontmatter(Some("type: Outline Note\ntitle: T"), CONCEPT_TYPE_DAILY_NOTE, "T", "C", "N");
        assert_eq!(kept, "type: Outline Note\ntitle: T\ncreated: C\nupdated: N");
        let stamped = touch_frontmatter(Some("title: T\nupdated: old"), CONCEPT_TYPE_DAILY_NOTE, "T", "C", "N");
        assert_eq!(stamped, "title: T\nupdated: N\ntype: Daily Note\ncreated: C");
    }

    /// A `type:`/`title:` nested under another key is that key's value, not the
    /// top-level one the host's `doc.has(key)` asks about — OKF nests exactly
    /// those names under `generated:`/`sources:` (§5.1/§5.2). Reading one as
    /// "already present" would leave the file with no top-level `type` at all.
    #[test]
    fn a_nested_key_is_not_mistaken_for_the_top_level_one() {
        let raw = "title: 2026-08-02\ngenerated:\n  by: claude-code\n  type: not-the-top-level-one\nupdated: old";
        let fm = touch_frontmatter(Some(raw), CONCEPT_TYPE_DAILY_NOTE, "2026-08-02", "C", "N");
        assert_eq!(
            fm,
            "title: 2026-08-02\ngenerated:\n  by: claude-code\n  type: not-the-top-level-one\nupdated: N\ntype: Daily Note\ncreated: C"
        );
        // The same rule, read back: a nested `updated:` is not the day's.
        assert_eq!(frontmatter_value(Some(raw), "updated").as_deref(), Some("old"));
        assert_eq!(frontmatter_value(Some("generated:\n  updated: x"), "updated"), None);
    }

    #[test]
    fn frontmatter_value_reads_a_key_or_none() {
        let raw = "title: 2026-08-02\nupdated: 2026-08-03T09:00:00.000Z";
        assert_eq!(frontmatter_value(Some(raw), "updated").as_deref(), Some("2026-08-03T09:00:00.000Z"));
        assert_eq!(frontmatter_value(Some(raw), "created"), None);
        assert_eq!(frontmatter_value(None, "updated"), None);
    }

    /// R6. This function writes into whatever note is already on disk, and a
    /// `.note.md` is hand-edited and agent-edited — it does not have to have
    /// *produced* a file to be handed one. Appending keys to a front-matter
    /// block that is not a YAML mapping makes something the host's own reader
    /// then refuses to parse as a map, so the block comes back untouched,
    /// exactly as `touchFrontmatter` (frontmatter.ts) leaves it.
    #[test]
    fn a_frontmatter_block_that_is_not_a_mapping_is_returned_untouched() {
        let touch = |raw: &str| {
            touch_frontmatter(
                Some(raw),
                CONCEPT_TYPE_DAILY_NOTE,
                "2026-08-02",
                "2026-08-01T16:00:05.019Z",
                "2026-08-03T09:00:00.000Z",
            )
        };
        for raw in [
            "just a sentence",
            "just a sentence\nsecond line",
            "- a\n- b",       // a sequence
            "title:value",    // no space: a plain scalar to YAML, not a mapping
            ">-\n  folded",
        ] {
            assert_eq!(touch(raw), raw, "rewrote a non-mapping block: {raw:?}");
        }
    }

    /// The other side of the same check: a real mapping must still be touched,
    /// including the shapes the line-based test could get wrong.
    #[test]
    fn a_mapping_is_still_touched_whatever_shape_it_takes() {
        let touch = |raw: Option<&str>| {
            touch_frontmatter(raw, "TY", "T", "C", "N")
        };
        // A key with an empty value is still a key.
        assert_eq!(touch(Some("title:")), "title:\ntype: TY\ncreated: C\nupdated: N");
        // A nested sequence under a key: the `- a` line is not a top-level
        // key, but `tags:` above it is — the block is still a mapping.
        assert_eq!(touch(Some("title: x\ntags:\n  - a")), "title: x\ntags:\n  - a\ntype: TY\ncreated: C\nupdated: N");
        // A value that contains a colon of its own.
        assert_eq!(touch(Some("home: https://notemd.net")), "home: https://notemd.net\ntype: TY\ntitle: T\ncreated: C\nupdated: N");
        // Empty / whitespace-only reads as "no front-matter at all".
        assert_eq!(touch(Some("")), touch(None));
        assert_eq!(touch(Some("\n   \n")), touch(None));
        // Comments are not content the keys may be appended *into*, but they
        // are kept — with the blank separator the host's serializer writes.
        assert_eq!(touch(Some("# a\n# b")), "# a\n# b\n\ntype: TY\ntitle: T\ncreated: C\nupdated: N");
    }

    #[test]
    fn frontmatter_touch_keeps_unknown_keys_and_only_moves_updated() {
        let raw = "title: 2026-08-02\nroam-uid: 08-02-2026\nupdated: 2026-01-01T00:00:00.000Z";
        let fm =
            touch_frontmatter(Some(raw), CONCEPT_TYPE_DAILY_NOTE, "2026-08-02", "x", "2026-08-03T09:00:00.000Z");
        assert!(fm.contains("roam-uid: 08-02-2026"));
        assert!(fm.contains("updated: 2026-08-03T09:00:00.000Z"));
        assert!(!fm.contains("2026-01-01"));
    }
}
