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
    Ok(ProjectionBundle {
        user: render_target(
            "USER",
            ProjectionTarget::User,
            protocol
                .value
                .category_registry
                .get("user")
                .cloned()
                .unwrap_or_default(),
            &views,
            action_sensitive_conflict,
        ),
        memory: render_target(
            "MEMORY",
            ProjectionTarget::Memory,
            protocol
                .value
                .category_registry
                .get("memory")
                .cloned()
                .unwrap_or_default(),
            &views,
            action_sensitive_conflict,
        ),
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
            space: None,
            purpose: None,
        },
    )
    .map_err(|error| error.to_string())?;
    let bundle = project(&repository, &snapshot).map_err(|error| error.to_string())?;
    atomic_replace(root, &root.join("USER.md"), &bundle.user)?;
    atomic_replace(root, &root.join("MEMORY.md"), &bundle.memory)?;
    Ok(bundle)
}

fn repository_error(error: RepositoryError) -> String {
    error.to_string()
}

fn render_target(
    title: &str,
    target: ProjectionTarget,
    registry: Vec<String>,
    claims: &[&ClaimView],
    action_sensitive_conflict: bool,
) -> String {
    let mut grouped = BTreeMap::<String, Vec<&ClaimView>>::new();
    for claim in claims {
        let Some(projection) = &claim.projection else {
            continue;
        };
        if projection.target == target {
            grouped
                .entry(projection.category.clone())
                .or_default()
                .push(claim);
        }
    }
    let mut categories = registry
        .into_iter()
        .filter(|category| grouped.contains_key(category))
        .collect::<Vec<_>>();
    for category in grouped.keys() {
        if !categories.contains(category) {
            categories.push(category.clone());
        }
    }
    let mut out = format!(
        "<!-- notemd-memory-control -->\n<!-- GENERATED / READ-ONLY: derived from .notemd/memory YAML; do not edit manually. -->\n# {title}\n"
    );
    if action_sensitive_conflict {
        out.push_str("\n> 存在未解决的权限或边界冲突，相关行动已暂停。\n");
    }
    for category in categories {
        let Some(entries) = grouped.get(&category) else {
            continue;
        };
        out.push_str(&format!("\n## {category}\n"));
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
    out
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
        let claim = ClaimView {
            claim_id: "c".into(),
            as_of_valid_time: "2026-09-01T00:00:00Z".into(),
            workflow_state: WorkflowState::Approved,
            lifecycle_state: Some(LifecycleState::Active),
            application_state: ApplicationState::Current,
            current_heads: vec![],
            projection_eligible: true,
            context_eligible: true,
            do_not_rely: false,
            conflict: None,
            text: Some("第一行\n## 伪标题\n- 伪条目".into()),
            projection: Some(Projection {
                target: ProjectionTarget::User,
                category: "preferences".into(),
                visibility: Visibility::Projection,
            }),
            claim_kind: Some(ClaimKind::Preference),
            salience: Some(Salience::Pinned),
        };
        let rendered = render_target(
            "USER",
            ProjectionTarget::User,
            vec!["preferences".into()],
            &[&claim],
            false,
        );
        assert_eq!(
            rendered,
            "<!-- notemd-memory-control -->\n<!-- GENERATED / READ-ONLY: derived from .notemd/memory YAML; do not edit manually. -->\n# USER\n\n## preferences\n\n- 第一行\n  \\## 伪标题\n  \\- 伪条目\n"
        );
    }

    #[test]
    fn action_sensitive_conflict_emits_only_a_generic_safety_notice() {
        let rendered = render_target("USER", ProjectionTarget::User, vec![], &[], true);
        assert!(rendered.contains("存在未解决的权限或边界冲突，相关行动已暂停"));
        assert!(!rendered.contains("允许发送"));
        assert!(!rendered.contains("禁止发送"));
    }
}
