use super::model::{MemoryEntry, Priority, Scope};
use serde_yaml::{Mapping, Value};
use sha2::{Digest, Sha256};

pub const CONTROL_NOTICE_MARKER: &str = "<!-- notemd-memory-control -->";

#[derive(Debug, Clone)]
pub struct ParsedBlock {
    pub start: usize,
    pub end: usize,
    pub entry: MemoryEntry,
}

fn property(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("  ")?;
    let (key, value) = rest.split_once("::")?;
    let key = key.trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    Some((key, value.trim()))
}

fn frontmatter_bounds(lines: &[String]) -> Option<(usize, usize)> {
    if lines.first().map(|line| line.as_str()) != Some("---") {
        return None;
    }
    lines
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(i, line)| (line == "---").then_some((0, i)))
}

fn frontmatter(content: &str) -> Result<(Value, usize), String> {
    let lines: Vec<String> = content.lines().map(str::to_string).collect();
    let (_, end) = frontmatter_bounds(&lines).ok_or("memory: document has no YAML frontmatter")?;
    let yaml = lines[1..end].join("\n");
    let value = serde_yaml::from_str(&yaml).map_err(|e| format!("memory: invalid YAML: {e}"))?;
    Ok((value, end))
}

pub fn owner_actor(content: &str) -> Result<Option<String>, String> {
    let (fm, _) = frontmatter(content)?;
    Ok(fm
        .get("owner")
        .and_then(|v| v.get("actor"))
        .and_then(Value::as_str)
        .map(str::to_string))
}

pub fn owner_revision(content: &str) -> u64 {
    frontmatter(content)
        .ok()
        .and_then(|(fm, _)| fm.get("owner")?.get("revision")?.as_u64())
        .unwrap_or(0)
}

pub fn owner_entry(content: &str) -> Result<Option<MemoryEntry>, String> {
    let (fm, _) = frontmatter(content)?;
    let Some(owner) = fm.get("owner") else {
        return Ok(None);
    };
    let Some(actor) = owner
        .get("actor")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    let names = owner
        .get("names")
        .and_then(Value::as_sequence)
        .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let id = owner
        .get("entry_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let revision = owner.get("revision").and_then(Value::as_u64).unwrap_or(0);
    let legacy = id.is_empty() || revision == 0;
    Ok(Some(MemoryEntry {
        id,
        scope: Scope::UserOwner,
        section: "Owner".into(),
        text: format!("{} ({})", actor, names.join(", ")),
        revision,
        status: if owner.get("confirmed").and_then(Value::as_bool) == Some(true) {
            "active".into()
        } else {
            "pending".into()
        },
        priority: Priority::High,
        proposal: owner
            .get("proposal")
            .and_then(Value::as_str)
            .map(str::to_string),
        approved_by: owner
            .get("confirmed_by")
            .and_then(Value::as_str)
            .map(str::to_string),
        approved_at: owner
            .get("confirmed_at")
            .and_then(Value::as_str)
            .map(str::to_string),
        source: None,
        document: "USER.md".into(),
        legacy,
    }))
}

pub fn proposed_owner(text: &str) -> Result<(String, Vec<String>), String> {
    let value: Value = serde_yaml::from_str(text)
        .map_err(|e| format!("memory: owner proposal must be YAML/JSON: {e}"))?;
    let actor = value
        .get("actor")
        .and_then(Value::as_str)
        .filter(|s| s.starts_with("human:"))
        .ok_or("memory: owner proposal requires actor: human:<id>")?
        .to_string();
    let names = value
        .get("names")
        .and_then(Value::as_sequence)
        .ok_or("memory: owner proposal requires names[]")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Err("memory: owner proposal requires at least one name".into());
    }
    Ok((actor, names))
}

pub fn owner_proposal_text(content: &str) -> Result<Option<String>, String> {
    let (fm, _) = frontmatter(content)?;
    let Some(owner) = fm.get("owner") else {
        return Ok(None);
    };
    let Some(actor) = owner
        .get("actor")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    let names = owner
        .get("names")
        .and_then(Value::as_sequence)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut payload = Mapping::new();
    payload.insert(Value::String("actor".into()), Value::String(actor.into()));
    payload.insert(
        Value::String("names".into()),
        Value::Sequence(names.into_iter().map(Value::String).collect()),
    );
    serde_yaml::to_string(&Value::Mapping(payload))
        .map(Some)
        .map_err(|e| format!("memory: serialize owner proposal: {e}"))
}

pub fn update_owner(
    content: &str,
    entry_id: &str,
    text: &str,
    revision: u64,
    proposal: &str,
    actor: &str,
    at: &str,
) -> Result<String, String> {
    let (proposed_actor, names) = proposed_owner(text)?;
    if proposed_actor != actor {
        return Err("memory: owner claim must be approved by the proposed actor".into());
    }
    let lines: Vec<String> = content.lines().map(str::to_string).collect();
    let (_, fm_end) =
        frontmatter_bounds(&lines).ok_or("memory: document has no YAML frontmatter")?;
    let mut fm: Value = serde_yaml::from_str(&lines[1..fm_end].join("\n"))
        .map_err(|e| format!("memory: invalid YAML: {e}"))?;
    let root = fm
        .as_mapping_mut()
        .ok_or("memory: frontmatter must be a mapping")?;
    let mut owner = Mapping::new();
    owner.insert(Value::String("actor".into()), Value::String(proposed_actor));
    owner.insert(
        Value::String("names".into()),
        Value::Sequence(names.into_iter().map(Value::String).collect()),
    );
    owner.insert(Value::String("confirmed".into()), Value::Bool(true));
    owner.insert(
        Value::String("entry_id".into()),
        Value::String(entry_id.into()),
    );
    owner.insert(
        Value::String("revision".into()),
        Value::Number(revision.into()),
    );
    owner.insert(
        Value::String("proposal".into()),
        Value::String(proposal.into()),
    );
    owner.insert(
        Value::String("confirmed_by".into()),
        Value::String(actor.into()),
    );
    owner.insert(
        Value::String("confirmed_at".into()),
        Value::String(at.into()),
    );
    root.insert(Value::String("owner".into()), Value::Mapping(owner));
    let yaml = serde_yaml::to_string(&fm).map_err(|e| format!("memory: serialize owner: {e}"))?;
    let mut body = lines[fm_end + 1..].join("\n");
    if content.ends_with('\n') {
        body.push('\n');
    }
    Ok(format!("---\n{}---\n{}", yaml, body))
}

pub fn is_managed(content: &str) -> bool {
    frontmatter(content)
        .ok()
        .and_then(|(v, _)| {
            v.get("managed")?
                .get("by")?
                .as_str()
                .map(|s| s == "notemd.memory")
        })
        .unwrap_or(false)
}

pub fn projection_hash(content: &str) -> String {
    let normalized = content
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("projection_hash:") {
                let indent = &line[..line.len() - line.trim_start().len()];
                format!("{indent}projection_hash: ''")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if content.ends_with('\n') { "\n" } else { "" };
    hex::encode(Sha256::digest(normalized.as_bytes()))
}

pub fn ensure_control_notice(content: &str) -> String {
    if content.contains(CONTROL_NOTICE_MARKER) {
        return content.to_string();
    }
    let notice = [
        CONTROL_NOTICE_MARKER,
        "> **GENERATED / READ-ONLY.** Do not edit this file directly. Use the Memory plugin",
        "> or `notemd memory propose`; direct filesystem changes are drift, not approval.",
        "> Entries with `status:: pending` are candidates and are not confirmed memory.",
    ];
    let mut lines = content.lines().map(str::to_string).collect::<Vec<_>>();
    let frontmatter_end = frontmatter_bounds(&lines).map(|(_, end)| end).unwrap_or(0);
    let heading = lines
        .iter()
        .enumerate()
        .skip(frontmatter_end + 1)
        .find_map(|(index, line)| line.starts_with("# ").then_some(index));
    let insert_at = heading
        .map(|index| index + 1)
        .unwrap_or(frontmatter_end + 1);
    let mut inserted = vec![String::new()];
    inserted.extend(notice.into_iter().map(str::to_string));
    inserted.push(String::new());
    lines.splice(insert_at..insert_at, inserted);
    lines.join("\n") + if content.ends_with('\n') { "\n" } else { "" }
}

pub fn stamp_managed(content: &str, revision: u64) -> Result<String, String> {
    let lines: Vec<String> = content.lines().map(str::to_string).collect();
    let (_, fm_end) =
        frontmatter_bounds(&lines).ok_or("memory: document has no YAML frontmatter")?;
    let yaml = lines[1..fm_end].join("\n");
    let mut fm: Value =
        serde_yaml::from_str(&yaml).map_err(|e| format!("memory: invalid YAML: {e}"))?;
    let root = fm
        .as_mapping_mut()
        .ok_or("memory: frontmatter must be a mapping")?;
    let mut managed = Mapping::new();
    managed.insert(
        Value::String("by".into()),
        Value::String("notemd.memory".into()),
    );
    managed.insert(Value::String("protocol".into()), Value::Number(1u64.into()));
    managed.insert(
        Value::String("revision".into()),
        Value::Number(revision.into()),
    );
    managed.insert(
        Value::String("projection_hash".into()),
        Value::String(String::new()),
    );
    root.insert(Value::String("managed".into()), Value::Mapping(managed));

    let yaml = serde_yaml::to_string(&fm).map_err(|e| format!("memory: serialize YAML: {e}"))?;
    let mut body = lines[fm_end + 1..].join("\n");
    if content.ends_with('\n') {
        body.push('\n');
    }
    let mut next = format!("---\n{}---\n{}", yaml, body);
    let hash = projection_hash(&next);
    next = next.replacen(
        "  projection_hash: ''",
        &format!("  projection_hash: {hash}"),
        1,
    );
    next = next.replacen(
        "  projection_hash: \"\"",
        &format!("  projection_hash: {hash}"),
        1,
    );
    Ok(next)
}

pub fn stored_projection_hash(content: &str) -> Option<String> {
    frontmatter(content)
        .ok()?
        .0
        .get("managed")?
        .get("projection_hash")?
        .as_str()
        .map(str::to_string)
}

pub fn parse_blocks(document: &str, content: &str, scope: Scope) -> Vec<ParsedBlock> {
    let lines: Vec<String> = content.lines().map(str::to_string).collect();
    let mut section = String::new();
    let mut starts = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(s) = line.strip_prefix("## ") {
            section = s.trim().to_string();
        }
        if line.starts_with("- ") {
            starts.push((i, section.clone()));
        }
    }

    let mut out = Vec::new();
    for (idx, (start, section)) in starts.iter().enumerate() {
        let next_start = starts.get(idx + 1).map(|(i, _)| *i).unwrap_or(lines.len());
        let mut end = next_start;
        for i in start + 1..next_start {
            if lines[i].starts_with("## ") || lines[i].starts_with("[^") {
                end = i;
                break;
            }
        }
        let block = &lines[*start..end];
        let props = block
            .iter()
            .filter_map(|line| property(line))
            .collect::<std::collections::HashMap<_, _>>();
        // Maintenance bullets and prose lists are not managed entries.
        if !props.contains_key("id") && !props.contains_key("source") {
            continue;
        }

        let text_lines = block
            .iter()
            .take_while(|line| property(line).is_none())
            .enumerate()
            .map(|(i, line)| {
                if i == 0 {
                    line.trim_start_matches("- ").trim()
                } else {
                    line.trim()
                }
            })
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let id = props.get("id").copied().unwrap_or("").to_string();
        let revision = props
            .get("revision")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let status = props
            .get("status")
            .copied()
            .unwrap_or(if id.is_empty() { "legacy" } else { "active" })
            .to_string();
        let priority = match props.get("priority").copied() {
            Some("high") => Priority::High,
            _ => Priority::Normal,
        };
        out.push(ParsedBlock {
            start: *start,
            end,
            entry: MemoryEntry {
                id,
                scope,
                section: section.clone(),
                text: text_lines.join(" "),
                revision,
                status,
                priority,
                proposal: props.get("proposal").map(|s| s.to_string()),
                approved_by: props.get("approved-by").map(|s| s.to_string()),
                approved_at: props.get("approved-at").map(|s| s.to_string()),
                source: props.get("source").map(|s| s.to_string()),
                document: document.to_string(),
                legacy: !props.contains_key("revision") || !props.contains_key("status"),
            },
        });
    }
    out
}

fn prop_line(key: &str, value: &str) -> String {
    format!("  {key}:: {value}")
}

fn set_property(block: &mut Vec<String>, key: &str, value: &str) {
    if let Some(line) = block
        .iter_mut()
        .find(|line| property(line).map(|(k, _)| k == key).unwrap_or(false))
    {
        *line = prop_line(key, value);
        return;
    }
    block.push(prop_line(key, value));
}

fn content_prefix_len(block: &[String]) -> usize {
    block
        .iter()
        .position(|line| property(line).is_some())
        .unwrap_or(block.len())
}

fn replacement_text(text: &str) -> Vec<String> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let first = lines.next().unwrap_or("");
    let mut out = vec![format!("- {first}")];
    out.extend(lines.map(|line| format!("  {line}")));
    out
}

pub struct EntryUpdate<'a> {
    pub text: Option<&'a str>,
    pub revision: u64,
    pub status: &'a str,
    pub priority: Priority,
    pub proposal: &'a str,
    pub approved_by: &'a str,
    pub approved_at: &'a str,
    pub source: Option<&'a str>,
}

pub fn update_entry(content: &str, id: &str, update: EntryUpdate<'_>) -> Result<String, String> {
    let lines: Vec<String> = content.lines().map(str::to_string).collect();
    let scope = if content.contains("type: User Profile") {
        Scope::UserProfile
    } else {
        Scope::Memory
    };
    let blocks = parse_blocks(
        if scope == Scope::Memory {
            "MEMORY.md"
        } else {
            "USER.md"
        },
        content,
        scope,
    );
    let block = blocks
        .into_iter()
        .find(|b| b.entry.id == id)
        .ok_or_else(|| format!("memory: entry not found: {id}"))?;
    let mut next_block = lines[block.start..block.end].to_vec();
    if let Some(text) = update.text {
        let prefix = content_prefix_len(&next_block);
        let mut replaced = replacement_text(text);
        replaced.extend_from_slice(&next_block[prefix..]);
        next_block = replaced;
    }
    set_property(&mut next_block, "id", id);
    set_property(&mut next_block, "revision", &update.revision.to_string());
    set_property(&mut next_block, "status", update.status);
    set_property(
        &mut next_block,
        "priority",
        match update.priority {
            Priority::Normal => "normal",
            Priority::High => "high",
        },
    );
    set_property(&mut next_block, "proposal", update.proposal);
    set_property(&mut next_block, "approved-by", update.approved_by);
    set_property(&mut next_block, "approved-at", update.approved_at);
    if let Some(source) = update.source {
        set_property(&mut next_block, "source", source);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&lines[..block.start]);
    out.extend(next_block);
    out.extend_from_slice(&lines[block.end..]);
    Ok(out.join("\n") + if content.ends_with('\n') { "\n" } else { "" })
}

pub fn mark_pending(
    content: &str,
    old_start: usize,
    old_end: usize,
    id: &str,
    proposal: &str,
) -> String {
    let lines: Vec<String> = content.lines().map(str::to_string).collect();
    let mut block = lines[old_start..old_end].to_vec();
    set_property(&mut block, "id", id);
    set_property(&mut block, "revision", "0");
    set_property(&mut block, "status", "pending");
    set_property(&mut block, "priority", "normal");
    set_property(&mut block, "proposal", proposal);
    let mut out = Vec::new();
    out.extend_from_slice(&lines[..old_start]);
    out.extend(block);
    out.extend_from_slice(&lines[old_end..]);
    out.join("\n") + if content.ends_with('\n') { "\n" } else { "" }
}

pub fn append_entry(
    content: &str,
    section: &str,
    id: &str,
    text: &str,
    revision: u64,
    priority: Priority,
    proposal: &str,
    approved_by: &str,
    approved_at: &str,
    source: &str,
) -> String {
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let heading = format!("## {section}");
    let mut insert = lines
        .iter()
        .position(|line| line == &heading)
        .map(|i| i + 1);
    if let Some(start) = insert {
        let mut i = start;
        while i < lines.len() && !lines[i].starts_with("## ") && !lines[i].starts_with("[^") {
            i += 1;
        }
        insert = Some(i);
    }
    let insert = insert.unwrap_or_else(|| {
        let i = lines
            .iter()
            .position(|line| line.starts_with("[^"))
            .unwrap_or(lines.len());
        lines.splice(i..i, vec![String::new(), heading.clone(), String::new()]);
        i + 3
    });
    let mut block = replacement_text(text);
    block.push(prop_line("id", id));
    block.push(prop_line("revision", &revision.to_string()));
    block.push(prop_line("status", "active"));
    block.push(prop_line(
        "priority",
        match priority {
            Priority::Normal => "normal",
            Priority::High => "high",
        },
    ));
    block.push(prop_line("proposal", proposal));
    block.push(prop_line("approved-by", approved_by));
    block.push(prop_line("approved-at", approved_at));
    block.push(prop_line("source", source));
    block.push(String::new());
    lines.splice(insert..insert, block);
    lines.join("\n") + "\n"
}

pub fn entry_hash(entry: &MemoryEntry) -> String {
    let raw = format!("{}\n{}\n{}", entry.document, entry.section, entry.text);
    hex::encode(Sha256::digest(raw.as_bytes()))
}

pub fn title_for(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(48)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\ntype: Memory\n---\n\n## Active\n\n- A durable fact\n  id:: 11111111-1111-4111-8111-111111111111\n  source:: /a.md#L1\n  recorded:: 2026-01-01T00:00:00Z\n  by:: agent/x\n\n## Maintenance\n\n- not an entry\n";

    #[test]
    fn parses_only_managed_claim_bullets() {
        let blocks = parse_blocks("MEMORY.md", DOC, Scope::Memory);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].entry.text, "A durable fact");
        assert_eq!(blocks[0].entry.section, "Active");
        assert!(blocks[0].entry.legacy);
    }

    #[test]
    fn updates_entry_without_losing_existing_properties() {
        let next = update_entry(
            DOC,
            "11111111-1111-4111-8111-111111111111",
            EntryUpdate {
                text: Some("Changed fact"),
                revision: 2,
                status: "active",
                priority: Priority::High,
                proposal: "p1",
                approved_by: "human:bruce",
                approved_at: "2026-09-01T00:00:00Z",
                source: Some("/b.md#L2"),
            },
        )
        .unwrap();
        assert!(next.contains("- Changed fact"));
        assert!(next.contains("  priority:: high"));
        assert!(next.contains("  by:: agent/x"));
        assert!(next.contains("  source:: /b.md#L2"));
    }

    #[test]
    fn managed_hash_ignores_its_own_value() {
        let stamped = stamp_managed(DOC, 1).unwrap();
        let stored = stored_projection_hash(&stamped).unwrap();
        assert_eq!(stored, projection_hash(&stamped));
    }

    #[test]
    fn control_notice_is_inserted_after_title_and_is_idempotent() {
        let with_title = DOC.replacen("## Active", "# Shared memory\n\n## Active", 1);
        let once = ensure_control_notice(&with_title);
        let twice = ensure_control_notice(&once);
        assert_eq!(once, twice);
        assert!(once.contains("# Shared memory\n\n<!-- notemd-memory-control -->"));
        assert!(once.contains("status:: pending"));
    }
}
