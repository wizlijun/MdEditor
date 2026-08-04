//! `.note.md` outline model + parser + serializer + front-matter touch-up.
//! Ported from `src/lib/outline/markdown.ts` (`parseOutline`/`serializeOutline`/
//! `splitFrontmatterBlock`) — the host's companion-file format. Task 6 reads a
//! vault `.note.md`, merges Roam blocks into the tree, and writes it back, so
//! this must parse/serialize byte-identically to the TS side; the golden
//! fixture in Task 7 is what catches drift, keep the two in step.
use regex::Regex;
use std::borrow::Cow;
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

/// A bullet: two-space indent units, then `-`, then either the end of the line
/// or a single space and the content. The "end of the line" half is what makes
/// an *empty* bullet survive a round trip through the outside world: it is
/// written as `- ` (dash, space, empty content), so the trailing space would
/// otherwise be load-bearing — and editors, formatters and git hooks strip
/// trailing whitespace routinely, while file-over-app treats an externally
/// edited vault file as normal input. Mirrors markdown.ts.
///
/// The optional group must be `(?: (.*))?`, never `- ?`: the latter would read
/// `--` and `---` (front-matter fence, horizontal rule) as bullets. The
/// front-matter fence is in any case already split off `text` before any line
/// reaches here.
fn bullet_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^((?:  )*)-(?: (.*))?$").unwrap())
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

/// `\r` is line-ending noise, never content.
///
/// A `.note.md` comes back from the outside world — a Windows editor, a
/// `core.autocrlf` checkout, a synced file — which is the traffic
/// file-over-app promises to survive. The host's parser is JavaScript, where
/// `.` and `$` both treat `\r` as a line terminator: a bullet line ending
/// `\r` fails its bullet regex outright and collapses into its parent, the
/// same data loss an empty bullet suffered when its trailing space was
/// stripped. Rust's `regex` crate matches `\r` with `.`, so the two ports
/// disagreed about whether a CRLF file even has nodes — unacceptable when one
/// vault is read by several agents.
///
/// Stripping once at the parser entry (rather than sprinkling `\r?` through
/// every pattern) is one place per side, byte-identical between the ports,
/// and needs no reasoning about JS-vs-Rust regex dialects. Stated plainly: a
/// CRLF file that is read and rewritten comes back as LF. The serializer is
/// untouched.
fn strip_carriage_returns(text: &str) -> Cow<'_, str> {
    if text.contains('\r') { Cow::Owned(text.replace('\r', "")) } else { Cow::Borrowed(text) }
}

pub fn parse_outline(text: &str) -> Tree {
    let text = strip_carriage_returns(text);
    let (frontmatter, body) = split_frontmatter_block(&text);
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
            // Group 2 is absent for an empty bullet written without its
            // trailing space (`-` alone) — that is an empty content, not a
            // missing bullet.
            let rest = caps.get(2).map_or(String::new(), |m| m.as_str().to_string());
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

/// The values a YAML 1.2 **core schema** reader resolves to something that is
/// not a string — `null`/`true`/`123`/`0x1f`/`1e3`/`.inf`. The host's `yaml`
/// package quotes exactly these when serializing a string (its
/// `stringifyString` runs every default tag's `test` against the plain form
/// and falls back to a quoted scalar on a hit), so `title: 123` never comes
/// back as the number 123.
/// Scalars a **YAML 1.1** reader (PyYAML and friends) resolves to a date or a
/// bool while YAML 1.2 keeps them strings. A daily note's `title` is exactly
/// such a value (`2026-08-02`), and an agent reading the vault with PyYAML
/// would get a `date` object where the host has a string. The host quotes these
/// too (`YAML11_AMBIGUOUS` in `src/lib/okf/concept.ts`); both sides must agree
/// or the shared front-matter fixture goes red.
fn yaml11_ambiguous_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}|y|Y|yes|Yes|YES|n|N|no|No|NO|on|On|ON|off|Off|OFF)$")
            .unwrap()
    })
}

fn non_string_scalar_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"^(?:~|[Nn]ull|NULL",
            r"|[Tt]rue|TRUE|[Ff]alse|FALSE",
            r"|0o[0-7]+|[-+]?[0-9]+|0x[0-9a-fA-F]+",
            r"|[-+]?(?:\.[0-9]+|[0-9]+(?:\.[0-9]*)?)[eE][-+]?[0-9]+",
            r"|[-+]?(?:\.[0-9]+|[0-9]+\.[0-9]*)",
            r"|[-+]?\.(?:inf|Inf|INF)|\.nan|\.NaN|\.NAN)$",
        ))
        .unwrap()
    })
}

/// May `value` be written as a YAML **plain** (unquoted) scalar? A port of the
/// host `yaml` package's `plainString` guard — the regex in
/// `yaml/dist/stringify/stringifyString.js`:
///
/// ```text
/// /^[\n\t ,[\]{}#&*!|>'"%@`]|^[?-]$|^[?-][ \t]|[\n:][ \t]|[ \t]\n|[\n\t ]#|[\n\t :]$/
/// ```
///
/// plus the "would it round-trip as a string?" check
/// ([`non_string_scalar_pattern`]). Kept as a predicate rather than folded
/// into [`yaml_scalar`] so each clause is separately assertable.
fn is_plain_safe(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    let Some(&first) = chars.first() else { return false }; // the empty string
    if matches!(
        first,
        '\n' | '\t' | ' ' | ',' | '[' | ']' | '{' | '}' | '#' | '&' | '*' | '!' | '|' | '>'
            | '\'' | '"' | '%' | '@' | '`'
    ) {
        return false;
    }
    if value == "?" || value == "-" {
        return false;
    }
    // Not from the regex: `plainString` has an earlier branch that sends ANY
    // multi-line value to `blockString`, so a line break is never plain even
    // when the regex would have allowed it. See `yaml_scalar`'s note on the
    // one place the two implementations part ways.
    if value.contains('\n') {
        return false;
    }
    if matches!(first, '?' | '-') && matches!(chars.get(1), Some(' ' | '\t')) {
        return false;
    }
    for pair in chars.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        // `: ` / `:\t` / a line break followed by indentation — all of which
        // make the reader see a nested key or a continuation.
        if matches!(a, '\n' | ':') && matches!(b, ' ' | '\t') {
            return false;
        }
        if matches!(a, ' ' | '\t') && b == '\n' {
            return false;
        }
        // A `#` preceded by whitespace opens a comment.
        if matches!(a, '\n' | '\t' | ' ') && b == '#' {
            return false;
        }
    }
    if matches!(chars[chars.len() - 1], '\n' | '\t' | ' ' | ':') {
        return false;
    }
    !non_string_scalar_pattern().is_match(value) && !yaml11_ambiguous_pattern().is_match(value)
}

/// A string as a YAML scalar, quoted **exactly when and how the host's `yaml`
/// package would quote it**. Both sides write this file — the plugin through
/// [`touch_frontmatter`], the host through `touchFrontmatter`
/// (`src/lib/outline/frontmatter.ts`, which goes through `yaml`) — so a
/// disagreement here is a file one of them cannot read back.
///
/// Until incremental sync, the only `title` this ever saw was a `yyyy-MM-dd`
/// date, and writing it raw was safe. It is now the writer for **arbitrary
/// Roam page titles**, where raw is not safe at all: `title: Book: Thinking
/// Fast and Slow` is unparsable YAML (`scripts/okf-lint-core.mjs` reports
/// `frontmatter-unparsable`), and the host's `yaml` reader swallows the
/// `type`/`created`/`updated` beneath it into a nested map — after which
/// `fmHas(raw, 'type')` is false and the host appends a *second* `type`, so
/// each write compounds the damage. `PKM #2` is worse still: it parses, so
/// nothing complains, and the title silently becomes `PKM`.
///
/// Quote style follows `yaml`'s `quotedString`: single quotes when the value
/// contains a `"` and no `'` (so the double quotes need no escaping), double
/// quotes otherwise — **except** that a value containing a line break always
/// takes the double-quoted form. A single-quoted YAML scalar cannot carry a
/// raw line break in a front-matter block: `title: 'say "hi"⏎there'` makes the
/// reader report `Missing closing 'quote`, read the title as `say "hi` and
/// lose every key after it. That is the unparsable-front-matter failure this
/// whole function exists to prevent, so the newline check comes first.
///
/// Two divergences from the host's `yaml` package remain, both verified to
/// read back through its own reader as the same string — they are byte
/// differences, not disagreements about what the file says:
///
/// 1. A value **containing a line break**: `yaml` renders a block scalar (`|-`
///    plus an indented body), which is a page of re-implementation (chomping
///    indicator, indentation indicator, its own fallbacks) for a case that
///    cannot occur — a Roam page title is a single-line field, and a newline
///    in one would already have produced a file name with a newline in it long
///    before reaching here. This writes a double-quoted `\n` escape instead.
///    `parseDocument` reads both back as the identical string, and the host
///    normalises the bytes to its own spelling the next time it touches the
///    file. Pinned from both sides by the shared fixture's `host_expected`.
/// 2. **Control characters** (`\r`, BEL, DEL, C1, ...) with nothing else
///    hostile about them: `yaml` forces double quotes, this leaves them plain.
///    The host's reader parses the plain form with no errors and returns the
///    control character intact, so nothing is lost or truncated; matching
///    `yaml` here would mean porting its escape table (`\a`, `\v`, `\0`, ...)
///    as well, for a shape no Roam title can hold.
pub fn yaml_scalar(value: &str) -> String {
    if is_plain_safe(value) {
        return value.to_string();
    }
    if !value.contains('\n') && value.contains('"') && !value.contains('\'') {
        return format!("'{}'", value.replace('\'', "''"));
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Replace the top-level `title:` of a front-matter block, in place, keeping
/// every other key and its order. Returns the block untouched when it is not a
/// mapping or has no top-level `title:` at all (in which case
/// [`touch_frontmatter`] appends one).
///
/// The **only** caller is the rename case in [`crate::incremental`]: Roam
/// renamed the page, the file moved, and without this the front-matter keeps
/// the old title forever — `touch_frontmatter` fills a *missing* title and
/// never overwrites one (the shared fixture pins that rule), and the sync that
/// follows a rename usually has nothing else to write, so no later run repairs
/// it. Scoped to the one case where the sync *knows* the title changed;
/// everywhere else an existing title is still the user's to keep.
pub fn refresh_frontmatter_title(raw: &str, title: &str) -> String {
    if !matches!(fm_shape(raw), FmShape::Mapping) {
        return raw.to_string();
    }
    let mut lines: Vec<String> = raw.lines().map(|l| l.to_string()).collect();
    if let Some(pos) = lines.iter().position(|l| l.starts_with("title:") && is_top_level_key(l)) {
        lines[pos] = format!("title: {}", yaml_scalar(title));
    }
    lines.join("\n")
}

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

    // Every value goes through `yaml_scalar`, not just `title`: the three
    // others are machine-generated (an OKF type constant, two ISO-8601
    // instants) and always come back unquoted today, but "this one happens to
    // be safe" is exactly the reasoning that made `title` unsafe the moment
    // it started carrying Roam page titles.
    if !has_key(&lines, "type") {
        lines.push(format!("type: {}", yaml_scalar(concept_type)));
    }
    if !has_key(&lines, "title") {
        lines.push(format!("title: {}", yaml_scalar(title)));
    }
    if !has_key(&lines, "created") {
        lines.push(format!("created: {}", yaml_scalar(created)));
    }

    let updated_line = format!("updated: {}", yaml_scalar(now));
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

    // Fix F. Same disease as the trailing space: one invisible byte decides
    // whether a node exists. In JavaScript `.` and `$` treat `\r` as a line
    // terminator, so a bullet line ending `\r` fails the host's bullet regex
    // and collapses into its parent — child gone, properties leaked into the
    // parent's text. Rust's regex crate matches it, so the two ports disagreed
    // about whether a CRLF file even has nodes. CRLF arrives from outside (a
    // Windows editor, a `core.autocrlf` checkout, a synced file), which is
    // exactly the traffic file-over-app promises to survive.
    //
    // Both ports now strip every `\r` at the parser entry — line-ending noise,
    // never content. One place per side, so the next person has nowhere to
    // miss, and no reasoning about JS-vs-Rust regex dialects is needed.
    //
    // The inputs and expectations below are the same, byte for byte, as the
    // host's `markdown.test.ts` CRLF suite: "both ports yield the same tree
    // for the same input" is the actual product requirement here.

    /// LF twin of the CRLF fixture, and what a CRLF file is rewritten as.
    const CRLF_LF_TWIN: &str = "---\ntitle: x\ncreated: y\n---\n- parent\n  - child\n    created:: 2026-08-03T14:11:47.891Z\n    id:: c1\n- after\n";

    #[test]
    fn a_crlf_file_parses_to_the_same_tree_as_its_lf_twin() {
        let crlf = CRLF_LF_TWIN.replace('\n', "\r\n");
        assert_eq!(
            serialize_outline(&parse_outline(&crlf)),
            serialize_outline(&parse_outline(CRLF_LF_TWIN))
        );
        // …and that is the LF text itself: the serializer is untouched, so a
        // CRLF file read and rewritten comes back as LF.
        assert_eq!(serialize_outline(&parse_outline(&crlf)), CRLF_LF_TWIN);
        assert_eq!(parse_outline(&crlf).frontmatter.as_deref(), Some("title: x\ncreated: y"));
    }

    /// The data-loss shape, with CRLF instead of a stripped trailing space.
    #[test]
    fn a_nested_child_survives_crlf() {
        let t = parse_outline("- parent\r\n  - child\r\n    created:: 2026-08-03T14:11:47.891Z\r\n    id:: x\r\n");
        assert_eq!(t.nodes.len(), 2);
        let parent = &t.nodes[0];
        let child = &t.nodes[1];
        assert_eq!(parent.content, "parent", "property lines must not leak into the parent");
        assert_eq!(child.content, "child");
        assert_eq!(child.id, "x");
        assert_eq!(child.parent.as_deref(), Some(parent.id.as_str()));
        assert_eq!(child.created_at.as_deref(), Some("2026-08-03T14:11:47.891Z"));
    }

    /// The shape actually found in the vault: an otherwise-LF file with one
    /// stray `\r` at the end of a single bullet line
    /// (`dailynote/2024/2024-05-16.note.md:115`).
    #[test]
    fn a_single_stray_cr_terminated_line_in_an_lf_file() {
        let t = parse_outline("- p\n  - abc\r\n    x\n    id:: k\n");
        assert_eq!(t.nodes.len(), 2);
        assert_eq!(t.nodes[0].content, "p");
        assert_eq!(t.nodes[1].id, "k");
        assert_eq!(t.nodes[1].content, "abc\nx");
    }

    /// A lone mid-line `\r` is normalised away too — the same way on both
    /// ports, which is the point. (Leaving it as content would mean relying on
    /// `.` behaving identically in two different regex engines; it does not.)
    #[test]
    fn a_lone_mid_line_cr_is_normalised_the_same_way() {
        let t = parse_outline("- a\rb\n");
        assert_eq!(t.nodes.len(), 1);
        assert_eq!(t.nodes[0].content, "ab");
    }

    #[test]
    fn mixed_line_endings_in_one_file() {
        let t = parse_outline("- a\r\n- b\n- c\r\n");
        assert_eq!(t.nodes.iter().map(|n| n.content.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c"]);
    }

    /// Raw fence mode takes its lines verbatim, so it would otherwise carry the
    /// `\r` straight into the answer body — and the two ports would then differ
    /// on where the fence closes. Entry-level stripping covers it for free.
    #[test]
    fn cr_is_normalised_inside_a_raw_fence_too() {
        let crlf = "- ```\r\n  type:: answer\r\n  x\r\n  ```\r\n";
        let lf = crlf.replace("\r\n", "\n");
        assert_eq!(serialize_outline(&parse_outline(crlf)), serialize_outline(&parse_outline(&lf)));
        let t = parse_outline(crlf);
        assert_eq!(t.nodes.len(), 1);
        assert_eq!(t.nodes[0].content, "```\ntype:: answer\nx\n```");
    }

    #[test]
    fn crlf_frontmatter_still_splits() {
        let t = parse_outline("---\r\ntitle: x\r\ncreated: y\r\n---\r\n- A\r\n");
        assert_eq!(t.frontmatter.as_deref(), Some("title: x\ncreated: y"));
        assert_eq!(t.nodes.len(), 1);
        assert_eq!(t.nodes[0].content, "A");
        assert_eq!(serialize_outline(&t), "---\ntitle: x\ncreated: y\n---\n- A\n");
    }

    #[test]
    fn a_file_without_any_cr_is_untouched() {
        assert_eq!(serialize_outline(&parse_outline(CRLF_LF_TWIN)), CRLF_LF_TWIN);
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
        assert!(fm.contains("title: \"2026-08-02\""));
        assert!(fm.contains("created: 2026-08-02T00:00:00.000Z"));
        assert!(fm.contains("updated: 2026-08-03T09:00:00.000Z"));
    }

    /// OKF v0.2 §4.1: `type` is REQUIRED on every concept document, so a note
    /// this plugin writes must carry one — as `Daily Note`, the type the host
    /// derives from the daily folder, not the `Outline Note` default.
    #[test]
    fn frontmatter_touch_stamps_the_okf_type_first_on_a_fresh_block() {
        let fm = touch_frontmatter(None, CONCEPT_TYPE_DAILY_NOTE, "T", "C", "N");
        assert_eq!(fm, "type: Daily Note\ntitle: T\ncreated: C\nupdated: \"N\"");
    }

    /// …and a type the file already declares is never rewritten (an existing
    /// key's value is not ours to change), while an *existing* block gets the
    /// missing key appended, not prepended — the host's key order survives.
    #[test]
    fn an_existing_type_is_kept_and_a_missing_one_is_appended() {
        let kept = touch_frontmatter(Some("type: Outline Note\ntitle: T"), CONCEPT_TYPE_DAILY_NOTE, "T", "C", "N");
        assert_eq!(kept, "type: Outline Note\ntitle: T\ncreated: C\nupdated: \"N\"");
        let stamped = touch_frontmatter(Some("title: T\nupdated: old"), CONCEPT_TYPE_DAILY_NOTE, "T", "C", "N");
        assert_eq!(stamped, "title: T\nupdated: \"N\"\ntype: Daily Note\ncreated: C");
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
            "title: 2026-08-02\ngenerated:\n  by: claude-code\n  type: not-the-top-level-one\nupdated: \"N\"\ntype: Daily Note\ncreated: C"
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

    /// The plain/quoted decision, clause by clause. The cross-language fixture
    /// (`tests/fixtures/frontmatter-touch.json`) pins the *titles this plugin
    /// actually writes* against the host's `yaml` package; this pins the
    /// predicate itself, including shapes a Roam title is unlikely to take but
    /// which `yaml` still decides one way rather than the other.
    #[test]
    fn yaml_scalar_quotes_exactly_what_the_host_yaml_package_quotes() {
        // Plain: nothing in these needs a quote, and adding one would churn
        // every existing file in the vault.
        for plain in [
            "回顾系统", "回顾/系统", "Daily Note", "Wiki Page",
            "2026-08-01T16:00:05.019Z",     // a `:` not followed by a space
            "Foo:Bar", "a:b:c", ":leading", "x?", "~x", "a#b", "ok!", "x|y", "a>b",
            "He said \"no\"",               // a quote that is not leading
            "back\\slash", "it's", "a - b", "v1.2.3", "1_000",
        ] {
            assert_eq!(yaml_scalar(plain), plain, "needlessly quoted {plain:?}");
        }

        // Double-quoted: an indicator, a structural sequence, or a value that
        // would resolve as something other than a string.
        for (raw, want) in [
            // YAML 1.1 readers (PyYAML) resolve these to a date / a bool, so both
            // sides quote them even though YAML 1.2 would keep them strings —
            // see `yaml11_ambiguous_pattern` and the host's `YAML11_AMBIGUOUS`.
            ("2026-08-02", "\"2026-08-02\""),
            ("yes", "\"yes\""),
            ("no", "\"no\""),
            ("on", "\"on\""),
            ("Off", "\"Off\""),
            ("Book: Thinking Fast and Slow", "\"Book: Thinking Fast and Slow\""),
            ("PKM #2", "\"PKM #2\""),
            ("*star", "\"*star\""),
            ("@home", "\"@home\""),
            ("[[nested]]", "\"[[nested]]\""),
            ("", "\"\""),
            ("- 待办", "\"- 待办\""),
            ("-", "\"-\""),
            ("?", "\"?\""),
            ("trailing:", "\"trailing:\""),
            ("trailing ", "\"trailing \""),
            ("  padded  ", "\"  padded  \""),
            ("#hash", "\"#hash\""),
            ("&anchor", "\"&anchor\""),
            ("!bang", "\"!bang\""),
            ("|pipe", "\"|pipe\""),
            (">gt", "\">gt\""),
            ("%percent", "\"%percent\""),
            ("`backtick", "\"`backtick\""),
            ("'squote", "\"'squote\""),
            (",comma", "\",comma\""),
            ("{brace", "\"{brace\""),
            ("2026", "\"2026\""),
            ("+5", "\"+5\""),
            ("1.5", "\"1.5\""),
            ("1e3", "\"1e3\""),
            ("0x1f", "\"0x1f\""),
            ("0o17", "\"0o17\""),
            (".inf", "\".inf\""),
            (".nan", "\".nan\""),
            ("true", "\"true\""),
            ("FALSE", "\"FALSE\""),
            ("null", "\"null\""),
            ("~", "\"~\""),
            // Both quote characters present: double-quoted, with escapes.
            ("it's: \"both\"", "\"it's: \\\"both\\\"\""),
            // A backslash only has to be escaped once quoting is on.
            ("a: b\\c", "\"a: b\\\\c\""),
        ] {
            assert_eq!(yaml_scalar(raw), want, "for {raw:?}");
        }

        // Single-quoted: needs quoting, holds a `"` and no `'`, so `yaml`
        // picks the style that leaves the double quotes alone.
        assert_eq!(yaml_scalar("Review: \"Dune\""), "'Review: \"Dune\"'");
        assert_eq!(yaml_scalar("\"leading quote\""), "'\"leading quote\"'");
    }

    /// The one place the two implementations deliberately differ, recorded so
    /// the divergence is a decision rather than a discovery. `yaml` writes a
    /// value containing a line break as a block scalar; this writes a
    /// double-quoted `\n` escape. Different bytes, identical string on
    /// read-back — and unreachable in practice, since a Roam page title is a
    /// single-line field (a newline in one would have produced a file name
    /// with a newline in it long before reaching here).
    #[test]
    fn a_line_break_is_escaped_rather_than_written_as_a_block_scalar() {
        assert_eq!(yaml_scalar("line\nbreak"), "\"line\\nbreak\"");
    }

    /// …and the escape wins over the single-quote style, which is the one
    /// combination that produced a file nobody could read back. A value with a
    /// `"`, no `'` and a line break used to take single quotes, and a raw line
    /// break inside them ends the scalar: the host's reader reports
    /// `Missing closing 'quote`, hands back `say "hi` as the title and drops
    /// `created`/`updated`/`type` with it. Exactly the unparsable front-matter
    /// the quoting exists to prevent, so it is pinned in both styles' terms.
    #[test]
    fn a_line_break_beats_the_single_quote_style_that_cannot_hold_one() {
        assert_eq!(yaml_scalar("say \"hi\"\nthere"), "\"say \\\"hi\\\"\\nthere\"");
        assert_eq!(yaml_scalar("say \"hi\"\r\nthere"), "\"say \\\"hi\\\"\\r\\nthere\"");
        // Still single-quoted where there is no line break to force the issue.
        assert_eq!(yaml_scalar("say: \"hi\""), "'say: \"hi\"'");
    }

    /// The other, deliberately-open divergence. `yaml` forces double quotes on
    /// a control character; this leaves the value plain, because the host's
    /// own reader parses the plain form without an error and hands the
    /// character back intact — a byte difference, not a disagreement about
    /// what the file says. Recorded so a later reader knows it was measured
    /// rather than missed.
    #[test]
    fn a_lone_control_character_is_left_plain_where_yaml_would_quote_it() {
        for c in ['\r', '\u{7}', '\u{7f}', '\u{9b}'] {
            let v = format!("a{c}b");
            assert_eq!(yaml_scalar(&v), v, "{:?} started being quoted", c);
        }
    }

    /// I3. `touch_frontmatter` deliberately never overwrites an existing
    /// title (the shared fixture pins that), so after a Roam rename the file
    /// moves and its front-matter keeps the old name forever. This is the
    /// narrow exception: only the rename case calls it.
    #[test]
    fn refresh_frontmatter_title_replaces_a_title_in_place() {
        assert_eq!(
            refresh_frontmatter_title("type: Wiki Page\ntitle: 旧名\ncreated: C", "新名"),
            "type: Wiki Page\ntitle: 新名\ncreated: C",
        );
        // Quoted through the same encoder, or the rename writes the very file
        // shape C1 exists to prevent.
        assert_eq!(
            refresh_frontmatter_title("title: 旧名", "Book: Thinking Fast and Slow"),
            "title: \"Book: Thinking Fast and Slow\"",
        );
        // A nested `title:` belongs to some other key's value.
        assert_eq!(
            refresh_frontmatter_title("sources:\n  - title: nested\ntitle: 旧名", "新名"),
            "sources:\n  - title: nested\ntitle: 新名",
        );
        // No top-level title to replace: left alone, `touch_frontmatter` adds one.
        assert_eq!(refresh_frontmatter_title("type: Wiki Page", "新名"), "type: Wiki Page");
        // Not a mapping: untouched, exactly like `touch_frontmatter`.
        assert_eq!(refresh_frontmatter_title("just a sentence", "新名"), "just a sentence");
        assert_eq!(refresh_frontmatter_title("", "新名"), "");
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
        assert_eq!(touch(Some("title:")), "title:\ntype: TY\ncreated: C\nupdated: \"N\"");
        // A nested sequence under a key: the `- a` line is not a top-level
        // key, but `tags:` above it is — the block is still a mapping.
        assert_eq!(touch(Some("title: x\ntags:\n  - a")), "title: x\ntags:\n  - a\ntype: TY\ncreated: C\nupdated: \"N\"");
        // A value that contains a colon of its own.
        assert_eq!(touch(Some("home: https://notemd.net")), "home: https://notemd.net\ntype: TY\ntitle: T\ncreated: C\nupdated: \"N\"");
        // Empty / whitespace-only reads as "no front-matter at all".
        assert_eq!(touch(Some("")), touch(None));
        assert_eq!(touch(Some("\n   \n")), touch(None));
        // Comments are not content the keys may be appended *into*, but they
        // are kept — with the blank separator the host's serializer writes.
        assert_eq!(touch(Some("# a\n# b")), "# a\n# b\n\ntype: TY\ntitle: T\ncreated: C\nupdated: \"N\"");
    }

    /// An empty Roam block is serialized as `- ` — a dash, a space, and
    /// nothing else — which makes the *trailing space* load-bearing. Editors,
    /// formatters and git hooks strip trailing whitespace as a matter of
    /// course, and file-over-app treats an externally edited vault file as
    /// normal input. Once the space is gone, `-` used to fall through to the
    /// "unclassifiable line" branch: flat, it degraded into a node whose
    /// content is `-` (re-serialized as `- -`, one level worse every save);
    /// nested, the child vanished entirely and its `created::`/`id::` lines
    /// became literal text inside the *parent*. So a line of nothing but
    /// indentation and `-` is an empty bullet, exactly as `- ` is. The
    /// serializer is deliberately unchanged: fixing the parser repairs every
    /// file already on disk without touching a byte of anyone's vault.
    #[test]
    fn a_bare_dash_is_an_empty_bullet() {
        let t = parse_outline("-\n");
        assert_eq!(t.nodes.len(), 1);
        assert_eq!(t.nodes[0].content, "");
    }

    #[test]
    fn properties_after_a_bare_dash_attach_to_it_not_to_the_previous_node() {
        let t = parse_outline("- kept\n-\n  created:: 2026-08-03T14:11:47.891Z\n  id:: X\n");
        assert_eq!(t.nodes.len(), 2);
        assert_eq!(t.nodes[0].content, "kept");
        assert_eq!(t.nodes[0].created_at, None);
        assert_eq!(t.nodes[1].content, "");
        assert_eq!(t.nodes[1].created_at.as_deref(), Some("2026-08-03T14:11:47.891Z"));
        assert_eq!(t.nodes[1].id, "X");
    }

    /// The case that actually destroyed a user's data: the child disappeared
    /// and `created:: …` showed up as a node's visible name.
    #[test]
    fn a_nested_bare_dash_stays_a_real_child() {
        let t = parse_outline("- parent\n  -\n    created:: 2026-08-03T14:11:47.891Z\n    id:: x\n");
        assert_eq!(t.nodes.len(), 2);
        let parent = &t.nodes[0];
        let child = &t.nodes[1];
        assert_eq!(parent.content, "parent", "property lines must not leak into the parent");
        assert_eq!(child.content, "");
        assert_eq!(child.id, "x");
        assert_eq!(child.parent.as_deref(), Some(parent.id.as_str()));
        assert_eq!(child.created_at.as_deref(), Some("2026-08-03T14:11:47.891Z"));
    }

    #[test]
    fn a_file_that_still_has_the_trailing_space_parses_the_same_way() {
        let with_space = parse_outline("- parent\n  - \n    created:: 2026-08-03T14:11:47.891Z\n    id:: x\n");
        let stripped = parse_outline("- parent\n  -\n    created:: 2026-08-03T14:11:47.891Z\n    id:: x\n");
        let shape = |t: &Tree| -> Vec<(String, Option<String>, Option<String>)> {
            t.nodes.iter().map(|n| (n.content.clone(), n.parent.clone(), n.created_at.clone())).collect()
        };
        assert_eq!(shape(&stripped), shape(&with_space));
    }

    /// The serializer is untouched, so a healed file is written back *with*
    /// the trailing space — and parsing that again is a fixed point. No
    /// `- -` degradation, no oscillation between the two spellings.
    #[test]
    fn a_stripped_empty_bullet_heals_and_then_holds_still() {
        let stripped = "- parent\n  -\n    created:: 2026-08-03T14:11:47.891Z\n    id:: x\n";
        let healed = "- parent\n  - \n    created:: 2026-08-03T14:11:47.891Z\n    id:: x\n";
        assert_eq!(serialize_outline(&parse_outline(stripped)), healed);
        assert_eq!(serialize_outline(&parse_outline(healed)), healed);
        assert_eq!(serialize_outline(&parse_outline("-\n")), "- \n");
        assert_eq!(serialize_outline(&parse_outline("- \n")), "- \n");
    }

    /// The bullet rule must stay narrow: `-` counts only when the line ends
    /// there or a single space follows. `- ?` would swallow `--` and `---`.
    #[test]
    fn dash_runs_and_a_dash_as_content_are_unaffected() {
        // A `---` in the *body* is not front-matter (that split happens on the
        // whole text, before any bullet is scanned) and not a bullet either.
        let rule = parse_outline("- A\n---\n");
        assert_eq!(rule.frontmatter, None);
        assert_eq!(rule.nodes.iter().map(|n| n.content.as_str()).collect::<Vec<_>>(), vec!["A", "---"]);
        let dashes = parse_outline("- A\n--\n");
        assert_eq!(dashes.nodes.iter().map(|n| n.content.as_str()).collect::<Vec<_>>(), vec!["A", "--"]);
        // Real front-matter still gets split off ahead of the bullet scan.
        let fm = parse_outline("---\ntitle: x\n---\n- A\n");
        assert_eq!(fm.frontmatter.as_deref(), Some("title: x"));
        assert_eq!(fm.nodes.len(), 1);
        assert_eq!(fm.nodes[0].content, "A");
        // `- -` is still a bullet whose content is "-".
        let dash_content = parse_outline("- -\n");
        assert_eq!(dash_content.nodes.len(), 1);
        assert_eq!(dash_content.nodes[0].content, "-");
        assert_eq!(serialize_outline(&dash_content), "- -\n");
    }

    /// Indentation is still counted in two-space units: an odd-indent `-` must
    /// do whatever an odd-indent `- x` does today (be absorbed as the previous
    /// node's continuation line), not quietly become a bullet.
    #[test]
    fn an_odd_indent_bare_dash_behaves_like_an_odd_indent_bullet() {
        assert_eq!(parse_outline("- A\n   - x\n").nodes[0].content, "A\n - x");
        assert_eq!(parse_outline("- A\n   -\n").nodes[0].content, "A\n -");
        // With no previous node to continue, both demote to a root node.
        assert_eq!(parse_outline("   - x\n").nodes[0].content, "- x");
        assert_eq!(parse_outline("   -\n").nodes[0].content, "-");
    }

    /// The two id guards must keep holding for empty bullets: a repeated
    /// `id::` is refused, and a `local-N`-shaped one cannot steal the
    /// placeholder a later node will be handed.
    #[test]
    fn the_id_guards_still_hold_for_empty_bullets() {
        let t = parse_outline("- parent\n  id:: dup\n  -\n    id:: dup\n");
        assert_eq!(t.nodes.len(), 2);
        let child = &t.nodes[1];
        assert_eq!(child.content, "");
        assert_ne!(child.id, "dup");
        assert!(!child.persist_id);
        assert_eq!(child.parent.as_deref(), Some("dup"));
        assert_eq!(serialize_outline(&t), "- parent\n  id:: dup\n  - \n");

        let t = parse_outline("-\n  id:: local-2\n  - b\n");
        let b = t.nodes.iter().find(|n| n.content == "b").unwrap();
        assert_ne!(b.parent.as_deref(), Some(b.id.as_str()));
        assert_eq!(serialize_outline(&t), "- \n  - b\n");
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
