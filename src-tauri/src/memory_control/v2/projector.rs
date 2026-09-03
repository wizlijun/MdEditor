use super::model::*;
use super::reducer::{reduce, ReducerError};
use super::repository::{RepositoryError, RepositorySnapshot, V2Repository};
use super::writer::RepositoryWriter;
use chrono::{SecondsFormat, Utc};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionBundle {
    pub user: String,
    pub memory: String,
}

pub fn project(
    repository: &RepositorySnapshot,
    snapshot: &MemorySnapshotV2,
) -> Result<ProjectionBundle, ReducerError> {
    if snapshot.protocol.conflict || snapshot.protocol.heads.len() != 1 {
        return Err(ReducerError {
            code: "MEMORY_AUTHORITY_CONFLICT",
            message: "protocol projection is not uniquely reducible".into(),
        });
    }
    let protocol = repository
        .protocols
        .iter()
        .find(|revision| {
            revision.value.revision_id == snapshot.protocol.heads[0].revision_id
                && revision.value.payload_sha256 == snapshot.protocol.heads[0].payload_sha256
        })
        .ok_or_else(|| ReducerError {
            code: "MEMORY_PROTOCOL_UNSUPPORTED",
            message: "current protocol payload is missing".into(),
        })?;
    let mut views = snapshot
        .claims
        .iter()
        .filter(|claim| claim.projection_eligible)
        .collect::<Vec<_>>();
    views.sort_by(|left, right| {
        salience_rank(left.salience)
            .cmp(&salience_rank(right.salience))
            .then_with(|| kind_rank(left.claim_kind).cmp(&kind_rank(right.claim_kind)))
            .then_with(|| left.claim_id.cmp(&right.claim_id))
    });
    let action_sensitive_conflict = snapshot.claims.iter().any(|claim| {
        claim
            .conflict
            .as_ref()
            .is_some_and(|conflict| conflict.risk_class == RiskClass::ActionSensitive)
    });
    let revisions = repository
        .claims
        .iter()
        .map(|item| (item.value.revision_id.as_str(), &item.value))
        .collect::<HashMap<_, _>>();
    let mut entries = Vec::with_capacity(views.len());
    for view in views {
        if view.current_heads.len() != 1 {
            continue;
        }
        let revision = revisions
            .get(view.current_heads[0].revision_id.as_str())
            .copied()
            .ok_or_else(|| ReducerError {
                code: "MEMORY_INVALID_DAG",
                message: format!("missing projection head for {}", view.claim_id),
            })?;
        entries.push((view, revision));
    }
    let registry = super::reducer::context_registry_head(repository)?
        .and_then(|head| {
            repository.context_registries.iter().find(|item| {
                item.value.revision_id == head.revision_id
                    && item.value.payload_sha256 == head.payload_sha256
            })
        })
        .map(|item| &item.value);
    let mut categories = protocol
        .value
        .category_registry
        .get("user")
        .cloned()
        .unwrap_or_default();
    categories.extend(
        protocol
            .value
            .category_registry
            .get("memory")
            .cloned()
            .unwrap_or_default(),
    );
    categories.dedup();
    Ok(ProjectionBundle {
        // Kept in the Rust API for source compatibility only. It is never
        // persisted; MEMORY.md is the sole root projection.
        user: String::new(),
        memory: render_memory(categories, &entries, registry, action_sensitive_conflict),
    })
}

pub fn select_context(
    repository: &RepositorySnapshot,
    request: ContextRequest,
) -> Result<MemoryContextV2, ReducerError> {
    let snapshot = reduce(
        repository,
        &SnapshotRequest {
            as_of_valid_time: request.as_of_valid_time.clone(),
            role: request.role.clone(),
            space: Some(request.space.clone()),
            purpose: Some(request.purpose.clone()),
        },
    )?;
    let revisions = repository
        .claims
        .iter()
        .map(|revision| (revision.value.revision_id.as_str(), &revision.value))
        .collect::<HashMap<_, _>>();
    let mut claims = Vec::new();
    for view in &snapshot.claims {
        if !view.context_eligible || view.current_heads.len() != 1 {
            continue;
        }
        let revision = revisions
            .get(view.current_heads[0].revision_id.as_str())
            .ok_or_else(|| ReducerError {
                code: "MEMORY_INVALID_DAG",
                message: format!("missing context head for {}", view.claim_id),
            })?;
        if revision.projection.visibility == Visibility::UiOnly
            || revision.consent.scope != "personal-assistant-only"
        {
            continue;
        }
        if request.external_transfer
            && revision.consent.external_provider_policy != ExternalProviderPolicy::Allow
        {
            continue;
        }
        claims.push(ContextClaim {
            claim_id: revision.claim_id.clone(),
            revision_id: revision.revision_id.clone(),
            payload_sha256: revision.payload_sha256.clone(),
            text: revision.text.clone(),
            claim_kind: revision.claim_kind,
            risk_class: revision.risk_class,
            do_not_rely: view.do_not_rely,
            guidance: revision.agent_use.guidance.clone(),
            avoid_error: revision.agent_use.avoid_error.clone(),
        });
    }
    claims.sort_by(|left, right| left.claim_id.cmp(&right.claim_id));
    let conflicts = snapshot
        .claims
        .iter()
        .filter_map(|claim| claim.conflict.as_ref())
        .filter(|conflict| {
            conflict.heads.iter().any(|head| {
                revisions
                    .get(head.revision_id.as_str())
                    .is_some_and(|revision| context_scope_matches(revision, &request))
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let action_sensitive_conflict = conflicts
        .iter()
        .any(|conflict| conflict.risk_class == RiskClass::ActionSensitive);
    Ok(MemoryContextV2 {
        request,
        action_allowed: snapshot.protocol.writable
            && snapshot.authority.action_allowed
            && !action_sensitive_conflict,
        claims,
        conflicts,
    })
}

fn context_scope_matches(revision: &MemoryClaimRevision, request: &ContextRequest) -> bool {
    revision.projection.visibility != Visibility::UiOnly
        && revision.consent.scope == "personal-assistant-only"
        && match &request.role {
            Some(role) => revision.context.roles.contains(role),
            None => revision.context.roles.is_empty(),
        }
        && revision.context.spaces.contains(&request.space)
        && revision.consent.allowed_purposes.contains(&request.purpose)
}

pub fn rebuild_projections(root: &Path) -> Result<ProjectionBundle, String> {
    let writer = RepositoryWriter::new(root);
    let _transaction = writer.begin().map_err(|error| error.to_string())?;
    rebuild_projections_unlocked(root)
}

pub(crate) fn rebuild_projections_unlocked(root: &Path) -> Result<ProjectionBundle, String> {
    let repository = V2Repository::new(root).load().map_err(repository_error)?;
    if repository.mode != RepositoryMode::V2Active {
        return Err(format!("MEMORY_PROTOCOL_NOT_ACTIVE: {:?}", repository.mode));
    }
    let as_of = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let snapshot = reduce(
        &repository,
        &SnapshotRequest {
            as_of_valid_time: as_of,
            role: None,
            space: None,
            purpose: None,
        },
    )
    .map_err(|error| error.to_string())?;
    let bundle = project(&repository, &snapshot).map_err(|error| error.to_string())?;
    atomic_replace(root, &root.join("MEMORY.md"), &bundle.memory)?;
    let legacy_user = root.join("USER.md");
    if legacy_user.exists() {
        fs::remove_file(&legacy_user).map_err(|error| format!("MEMORY_IO: {error}"))?;
    }
    Ok(bundle)
}

fn repository_error(error: RepositoryError) -> String {
    error.to_string()
}

fn render_memory(
    registry: Vec<String>,
    claims: &[(&ClaimView, &MemoryClaimRevision)],
    context_registry: Option<&ContextRegistryRevision>,
    action_sensitive_conflict: bool,
) -> String {
    let mut grouped = BTreeMap::<(String, String, String), Vec<&ClaimView>>::new();
    for (claim, revision) in claims {
        let Some(projection) = &claim.projection else {
            continue;
        };
        let roles = if revision.context.roles.is_empty() {
            vec!["role:unclassified".to_string()]
        } else {
            revision.context.roles.clone()
        };
        for scope in &revision.context.spaces {
            for role in &roles {
                grouped
                    .entry((scope.clone(), role.clone(), projection.category.clone()))
                    .or_default()
                    .push(*claim);
            }
        }
    }
    let role_map = context_registry
        .map(|value| {
            value
                .roles
                .iter()
                .map(|role| (role.role_id.as_str(), role))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let scope_map = context_registry
        .map(|value| {
            value
                .scopes
                .iter()
                .map(|scope| (scope.scope_id.as_str(), scope))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let mut out = "# MEMORY\n\n> Agent 使用规则：这是完整的人类可读投影，不代表所有分组都可用于当前任务。\n> 必须先通过 Memory context broker 确认当前 Role、Scope 与用途，只加载匹配切片；不要跨分组推断或混用事实。\n".to_string();
    if action_sensitive_conflict {
        out.push_str("\n> 存在未解决的权限或边界冲突，相关行动已暂停。\n");
    }
    let mut scopes = grouped
        .keys()
        .map(|(scope, _, _)| scope.clone())
        .collect::<Vec<_>>();
    scopes.sort_by_key(|id| {
        scope_map
            .get(id.as_str())
            .map(|scope| scope.display_name.to_lowercase())
            .unwrap_or_else(|| id.to_lowercase())
    });
    scopes.dedup();
    for scope_id in scopes {
        let scope = scope_map.get(scope_id.as_str()).copied();
        let scope_label = scope
            .map(|value| value.display_name.as_str())
            .unwrap_or(scope_id.as_str());
        out.push_str(&format!(
            "\n## Scope · {}\n\n<!-- memory:scope {} -->\n",
            escape_heading(scope_label),
            escape_comment(&scope_id)
        ));
        if let Some(scope) = scope {
            append_guidance(&mut out, "Scope", &scope.agent_use);
        }
        let mut roles = grouped
            .keys()
            .filter(|(scope, _, _)| scope == &scope_id)
            .map(|(_, role, _)| role.clone())
            .collect::<Vec<_>>();
        roles.sort_by_key(|id| {
            role_map
                .get(id.as_str())
                .map(|role| role.display_name.to_lowercase())
                .unwrap_or_else(|| id.to_lowercase())
        });
        roles.dedup();
        for role_id in roles {
            let role = role_map.get(role_id.as_str()).copied();
            let role_label = role
                .map(|value| value.display_name.as_str())
                .unwrap_or(role_id.as_str());
            out.push_str(&format!(
                "\n### Role · {}\n\n<!-- memory:role {} -->\n",
                escape_heading(role_label),
                escape_comment(&role_id)
            ));
            if let Some(role) = role {
                append_guidance(&mut out, "Role", &role.agent_use);
            }
            let mut categories = registry
                .iter()
                .filter(|category| {
                    grouped.contains_key(&(scope_id.clone(), role_id.clone(), (*category).clone()))
                })
                .cloned()
                .collect::<Vec<_>>();
            for (scope, role, category) in grouped.keys() {
                if scope == &scope_id && role == &role_id && !categories.contains(category) {
                    categories.push(category.clone());
                }
            }
            for category in categories {
                let Some(entries) =
                    grouped.get(&(scope_id.clone(), role_id.clone(), category.clone()))
                else {
                    continue;
                };
                out.push_str(&format!("\n#### {}\n", escape_heading(&category)));
                for entry in entries {
                    let Some(text) = &entry.text else {
                        continue;
                    };
                    out.push('\n');
                    for (index, line) in text.lines().enumerate() {
                        let line = escape_continuation(line);
                        if index == 0 {
                            out.push_str("- ");
                        } else {
                            out.push_str("  ");
                        }
                        out.push_str(&line);
                        out.push('\n');
                    }
                }
            }
        }
    }
    out
}

fn append_guidance(out: &mut String, label: &str, use_policy: &AgentUse) {
    if !use_policy.guidance.trim().is_empty() {
        out.push_str(&format!(
            "\n> {label} 指引：{}\n",
            use_policy.guidance.trim().replace('\n', " ")
        ));
    }
    if !use_policy.avoid_error.trim().is_empty() {
        out.push_str(&format!(
            "> 避免：{}\n",
            use_policy.avoid_error.trim().replace('\n', " ")
        ));
    }
}

fn escape_heading(value: &str) -> String {
    value.trim().replace(['\r', '\n'], " ").replace('#', "\\#")
}

fn escape_comment(value: &str) -> String {
    value.replace("--", "—").replace(['<', '>'], "")
}

fn escape_continuation(line: &str) -> String {
    let trimmed = line.trim_start();
    let structural = ["#", "- ", "+ ", "* ", ">", "`", "---"]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix));
    if structural {
        format!("\\{line}")
    } else {
        line.to_string()
    }
}

fn salience_rank(value: Option<Salience>) -> u8 {
    match value {
        Some(Salience::Pinned) => 0,
        _ => 1,
    }
}

fn kind_rank(value: Option<ClaimKind>) -> u8 {
    match value {
        Some(ClaimKind::Identity) => 0,
        Some(ClaimKind::Preference) => 1,
        Some(ClaimKind::Boundary) => 2,
        Some(ClaimKind::Decision) => 3,
        Some(ClaimKind::Belief) => 4,
        Some(ClaimKind::Observation) => 5,
        Some(ClaimKind::Commitment) => 6,
        Some(ClaimKind::Practice) => 7,
        Some(ClaimKind::MaterialFact) => 8,
        Some(ClaimKind::Quotation) => 9,
        _ => 10,
    }
}

fn atomic_replace(root: &Path, path: &Path, content: &str) -> Result<(), String> {
    let tmp_dir = root.join(".notemd/memory/.local/tmp");
    fs::create_dir_all(&tmp_dir).map_err(|error| format!("MEMORY_IO: {error}"))?;
    let mut temp = NamedTempFile::new_in(tmp_dir).map_err(|error| format!("MEMORY_IO: {error}"))?;
    temp.write_all(content.as_bytes())
        .map_err(|error| format!("MEMORY_IO: {error}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|error| format!("MEMORY_IO: {error}"))?;
    temp.persist(path)
        .map_err(|error| format!("MEMORY_IO: {}", error.error))?;
    if let Some(parent) = path.parent() {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_lines_cannot_escape_the_fact_bullet() {
        assert_eq!(escape_continuation("第一行"), "第一行");
        assert_eq!(escape_continuation("## 伪标题"), "\\## 伪标题");
        assert_eq!(escape_continuation("- 伪条目"), "\\- 伪条目");
    }

    #[test]
    fn action_sensitive_conflict_emits_only_a_generic_safety_notice() {
        let rendered = render_memory(vec![], &[], None, true);
        assert!(rendered.contains("存在未解决的权限或边界冲突，相关行动已暂停"));
        assert!(!rendered.contains("允许发送"));
        assert!(!rendered.contains("禁止发送"));
    }
}
