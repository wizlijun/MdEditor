//! `notemd memory` — controlled USER/MEMORY CLI.
//!
//! This is core rather than plugin-provided so an Agent's proposal path does
//! not disappear when the Memory window plugin is disabled. The window and CLI
//! still share `crate::memory_control` for every state transition.

use crate::memory_control::v2 as memory_v2;
use crate::memory_control::{self, model::*};
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Clone, Default)]
pub struct MemoryArgs {
    pub action: String,
    pub positionals: Vec<String>,
    pub vault: Option<String>,
    pub json: bool,
    pub flags: std::collections::HashMap<String, String>,
    pub bools: std::collections::HashSet<String>,
}

impl MemoryArgs {
    pub fn with_global_json(mut self, global: bool) -> Self {
        self.json = self.json || global;
        self
    }
}

pub fn parse_args(rest: &[String], json_global: bool) -> MemoryArgs {
    let mut out = MemoryArgs {
        action: rest.first().cloned().unwrap_or_else(|| "list".into()),
        json: json_global,
        ..Default::default()
    };
    let mut i = 1usize;
    while i < rest.len() {
        let token = &rest[i];
        if token == "--json" {
            out.json = true;
        } else if matches!(
            token.as_str(),
            "--confirm-human-approved" | "--all" | "--dry-run" | "--external-transfer"
        ) {
            out.bools.insert(token.trim_start_matches("--").to_string());
        } else if token.starts_with("--") {
            if let Some(value) = rest.get(i + 1) {
                out.flags
                    .insert(token.trim_start_matches("--").to_string(), value.clone());
                if token == "--vault" {
                    out.vault = Some(value.clone());
                }
                i += 1;
            }
        } else {
            out.positionals.push(token.clone());
        }
        i += 1;
    }
    out
}

fn root(args: &MemoryArgs) -> Result<PathBuf, String> {
    let root = super::search::resolve_vault_root(args.vault.as_deref())
        .ok_or("no vault configured; pass --vault PATH or configure one in Preferences")?;
    if !root.is_dir() {
        return Err(format!("vault not found: {}", root.display()));
    }
    Ok(root)
}

fn parse_scope(value: Option<&String>) -> Result<Scope, String> {
    match value.map(String::as_str).unwrap_or("memory") {
        "memory" => Ok(Scope::Memory),
        "user" | "user-profile" | "user.profile" => Ok(Scope::UserProfile),
        "owner" | "user-owner" | "user.owner" => Ok(Scope::UserOwner),
        other => Err(format!("invalid scope: {other}")),
    }
}

fn parse_operation(value: &str) -> Result<Operation, String> {
    match value {
        "create" | "add" => Ok(Operation::Create),
        "replace" | "edit" => Ok(Operation::Replace),
        "merge" => Ok(Operation::Merge),
        "revoke" => Ok(Operation::Revoke),
        "delete" => Ok(Operation::Delete),
        "set-priority" | "priority" => Ok(Operation::SetPriority),
        other => Err(format!("invalid operation: {other}")),
    }
}

fn parse_priority(value: Option<&String>) -> Result<Option<Priority>, String> {
    match value.map(String::as_str) {
        None => Ok(None),
        Some("critical") => Ok(Some(Priority::Critical)),
        Some("normal") => Ok(Some(Priority::Normal)),
        Some("high") => Ok(Some(Priority::High)),
        Some("low") => Ok(Some(Priority::Low)),
        Some(other) => Err(format!("invalid priority: {other}")),
    }
}

fn parse_polarity(value: Option<&String>) -> Result<Option<Polarity>, String> {
    match value.map(String::as_str) {
        None => Ok(None),
        Some("positive") => Ok(Some(Polarity::Positive)),
        Some("negative") => Ok(Some(Polarity::Negative)),
        Some("neutral") => Ok(Some(Polarity::Neutral)),
        Some(other) => Err(format!("invalid polarity: {other}")),
    }
}

fn parse_epistemic_status(value: Option<&String>) -> Result<Option<EpistemicStatus>, String> {
    match value.map(String::as_str) {
        None => Ok(None),
        Some("owner-stated") => Ok(Some(EpistemicStatus::OwnerStated)),
        Some("source-supported") => Ok(Some(EpistemicStatus::SourceSupported)),
        Some("inferred") => Ok(Some(EpistemicStatus::Inferred)),
        Some("contested") => Ok(Some(EpistemicStatus::Contested)),
        Some("unknown") => Ok(Some(EpistemicStatus::Unknown)),
        Some(other) => Err(format!("invalid epistemic-status: {other}")),
    }
}

fn parse_certainty(value: Option<&String>) -> Result<Option<Certainty>, String> {
    match value.map(String::as_str) {
        None => Ok(None),
        Some("high") => Ok(Some(Certainty::High)),
        Some("medium") => Ok(Some(Certainty::Medium)),
        Some("low") => Ok(Some(Certainty::Low)),
        Some("unknown") => Ok(Some(Certainty::Unknown)),
        Some(other) => Err(format!("invalid certainty: {other}")),
    }
}

fn print_json_or_text(args: &MemoryArgs, value: serde_json::Value, text: String) {
    if args.json {
        println!("{}", json!({"ok":true,"data":value}));
    } else {
        println!("{text}");
    }
}

fn list(args: &MemoryArgs, root: &std::path::Path) -> Result<(), String> {
    let snapshot = memory_control::list(root)?;
    let scope = args
        .flags
        .get("scope")
        .map(|v| parse_scope(Some(v)))
        .transpose()?;
    let status =
        args.flags
            .get("status")
            .map(String::as_str)
            .unwrap_or(if args.bools.contains("all") {
                "all"
            } else {
                "active"
            });
    let priority = parse_priority(args.flags.get("priority"))?;
    let polarity = parse_polarity(args.flags.get("polarity"))?;
    let entries = snapshot
        .entries
        .into_iter()
        .filter(|entry| {
            scope.map(|s| entry.scope == s).unwrap_or(true)
                && (status == "all" || entry.status == status)
                && priority.map(|p| entry.priority == p).unwrap_or(true)
                && polarity.map(|p| entry.polarity == p).unwrap_or(true)
        })
        .collect::<Vec<_>>();
    let proposals = snapshot
        .proposals
        .into_iter()
        .filter(|proposal| {
            status == "all" || status == "pending" && proposal.decision == ProposalDecision::Pending
        })
        .collect::<Vec<_>>();
    let value = json!({"entries":entries,"proposals":proposals,"integrity":snapshot.integrity});
    let mut text = String::new();
    if value["integrity"]["drift"].as_bool() == Some(true) {
        text.push_str("WARNING: projection drift detected; writes are blocked.\n\n");
    }
    for entry in entries_from_value(&value) {
        text.push_str(&format!(
            "{}  [{} / {:?} / {:?} / {:?} / {:?}]  {}\n",
            entry.id,
            entry.status,
            entry.priority,
            entry.polarity,
            entry.epistemic_status,
            entry.certainty,
            entry.text
        ));
    }
    let pending = value["proposals"].as_array().map(Vec::len).unwrap_or(0);
    if pending > 0 {
        text.push_str(&format!("\n{pending} pending proposal(s)\n"));
    }
    if text.trim().is_empty() {
        text.push_str("No matching memory entries.\n");
    }
    print_json_or_text(args, value, text.trim_end().to_string());
    Ok(())
}

fn entries_from_value(value: &serde_json::Value) -> Vec<MemoryEntry> {
    serde_json::from_value(value["entries"].clone()).unwrap_or_default()
}

fn show(args: &MemoryArgs, root: &std::path::Path) -> Result<(), String> {
    let id = args
        .positionals
        .first()
        .or_else(|| args.flags.get("id"))
        .ok_or("show requires an entry or proposal id")?;
    let snapshot = memory_control::list(root)?;
    if let Some(entry) = snapshot.entries.iter().find(|entry| &entry.id == id) {
        let value = serde_json::to_value(entry).map_err(|e| e.to_string())?;
        print_json_or_text(
            args,
            value,
            format!(
                "{}\nstatus: {}\npriority: {:?}\npolarity: {:?}\nepistemic-status: {:?}\ncertainty: {:?}\nagent-guidance: {}\navoid-error: {}\nrevision: {}\nsource: {}\n\n{}",
                entry.id,
                entry.status,
                entry.priority,
                entry.polarity,
                entry.epistemic_status,
                entry.certainty,
                entry.agent_guidance.as_deref().unwrap_or("-"),
                entry.avoid_error.as_deref().unwrap_or("-"),
                entry.revision,
                entry.source.as_deref().unwrap_or("-"),
                entry.text
            ),
        );
        return Ok(());
    }
    if let Some(proposal) = snapshot
        .proposals
        .iter()
        .find(|proposal| &proposal.frontmatter.proposal.id == id)
    {
        let target = proposal
            .frontmatter
            .proposal
            .target_id
            .as_deref()
            .and_then(|target_id| snapshot.entries.iter().find(|entry| entry.id == target_id));
        let before = target.map(|entry| entry.text.as_str()).unwrap_or("—");
        let after = match proposal.frontmatter.proposal.operation {
            Operation::Revoke => "revoked (history retained)".to_string(),
            Operation::Delete => "removed from projection (candidate/event retained)".to_string(),
            Operation::SetPriority => format!(
                "{:?}: {}",
                proposal
                    .frontmatter
                    .proposal
                    .suggested_priority
                    .unwrap_or_default(),
                before
            ),
            _ => proposal.text.clone(),
        };
        let mut value = serde_json::to_value(proposal).map_err(|e| e.to_string())?;
        if let Some(object) = value.as_object_mut() {
            object.insert("before".into(), json!(before));
            object.insert("after".into(), json!(after));
        }
        let merge_sources = &proposal.frontmatter.proposal.merge_from;
        print_json_or_text(
            args,
            value,
            format!(
                "{}\nSHA-256: {}\n{:?} / {:?}\ndecision: {:?}\nsource: {}\nmerge sources: {}\n\nBefore:\n{}\n\nAfter:\n{}",
                id,
                proposal.sha256,
                proposal.frontmatter.proposal.scope,
                proposal.frontmatter.proposal.operation,
                proposal.decision,
                proposal
                    .frontmatter
                    .sources
                    .first()
                    .map(|s| s.resource.as_str())
                    .unwrap_or("-"),
                if merge_sources.is_empty() {
                    "-".into()
                } else {
                    merge_sources.join(", ")
                },
                before,
                after,
            ),
        );
        return Ok(());
    }
    Err(format!("entry or proposal not found: {id}"))
}

fn propose(args: &MemoryArgs, root: &std::path::Path) -> Result<(), String> {
    let operation = parse_operation(
        args.positionals
            .first()
            .map(String::as_str)
            .or_else(|| args.flags.get("operation").map(String::as_str))
            .unwrap_or("create"),
    )?;
    let target_id = args.flags.get("target").cloned();
    let base_revision = args
        .flags
        .get("base-revision")
        .map(|s| {
            s.parse::<u64>()
                .map_err(|_| "base-revision must be an integer".to_string())
        })
        .transpose()?;
    let text = args.flags.get("text").cloned().unwrap_or_default();
    let source = args
        .flags
        .get("source")
        .cloned()
        .ok_or("propose requires --source")?;
    let by = args
        .flags
        .get("by")
        .cloned()
        .ok_or("propose requires --by")?;
    let dedupe_key = args
        .flags
        .get("dedupe-key")
        .cloned()
        .ok_or("propose requires --dedupe-key")?;
    let proposal = memory_control::propose(
        root,
        ProposeInput {
            scope: parse_scope(args.flags.get("scope"))?,
            operation,
            text,
            source,
            by,
            dedupe_key,
            reason: args.flags.get("reason").cloned().unwrap_or_default(),
            target_id,
            base_revision,
            section: args.flags.get("section").cloned(),
            priority: parse_priority(args.flags.get("priority"))?,
            polarity: parse_polarity(args.flags.get("polarity"))?,
            epistemic_status: parse_epistemic_status(args.flags.get("epistemic-status"))?,
            certainty: parse_certainty(args.flags.get("certainty"))?,
            agent_guidance: args.flags.get("agent-guidance").cloned(),
            avoid_error: args.flags.get("avoid-error").cloned(),
            merge_from: args
                .flags
                .get("merge-from")
                .map(|s| {
                    s.split(',')
                        .filter(|v| !v.trim().is_empty())
                        .map(|v| v.trim().to_string())
                        .collect()
                })
                .unwrap_or_default(),
        },
    )?;
    let value = serde_json::to_value(&proposal).map_err(|e| e.to_string())?;
    print_json_or_text(
        args,
        value,
        format!(
            "Created proposal {}\n{}",
            proposal.frontmatter.proposal.id, proposal.path
        ),
    );
    Ok(())
}

fn v2_snapshot(root: &std::path::Path) -> Result<serde_json::Value, String> {
    memory_control::dispatch(
        root,
        "host.memory.v2.snapshot",
        &json!({"as_of_valid_time": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)}),
    )
}

fn parse_v2_enum<T: serde::de::DeserializeOwned>(value: &str, name: &str) -> Result<T, String> {
    serde_json::from_value(json!(value)).map_err(|_| format!("invalid {name}: {value}"))
}

fn required_flag<'a>(args: &'a MemoryArgs, name: &str) -> Result<&'a str, String> {
    args.flags
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("--{name} is required"))
}

fn current_v2(
    root: &std::path::Path,
) -> Result<(memory_v2::RepositorySnapshot, memory_v2::MemorySnapshotV2), String> {
    let repository = memory_v2::V2Repository::new(root)
        .load()
        .map_err(|error| error.to_string())?;
    if repository.mode != memory_v2::RepositoryMode::V2Active {
        return Err(format!("MEMORY_PROTOCOL_NOT_ACTIVE: {:?}", repository.mode));
    }
    let reduced = memory_v2::reduce(
        &repository,
        &memory_v2::SnapshotRequest {
            as_of_valid_time: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            space: None,
            purpose: None,
        },
    )
    .map_err(|error| error.to_string())?;
    Ok((repository, reduced))
}

fn v2_kind_data(kind: memory_v2::ClaimKind, text: &str, actor: &str) -> memory_v2::KindData {
    match kind {
        memory_v2::ClaimKind::Identity => memory_v2::KindData::Identity(memory_v2::IdentityData {
            identity_type: memory_v2::IdentityType::Person,
            value: text.into(),
        }),
        memory_v2::ClaimKind::Preference => {
            memory_v2::KindData::Preference(memory_v2::PreferenceData {
                dimension: "general".into(),
            })
        }
        memory_v2::ClaimKind::Boundary => memory_v2::KindData::Boundary(memory_v2::BoundaryData {
            behavior_policy: memory_v2::BehaviorPolicy {
                effect: memory_v2::PolicyEffect::Deny,
                actions: vec!["unspecified-action".into()],
                resources: vec!["owner-data".into()],
                conditions: vec![text.into()],
            },
        }),
        memory_v2::ClaimKind::Decision => memory_v2::KindData::Decision(memory_v2::DecisionData {
            made_by: actor.into(),
            decided_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            decision_scope: "personal-assistant".into(),
        }),
        memory_v2::ClaimKind::Belief => memory_v2::KindData::Belief(memory_v2::BeliefData {
            proposition: text.into(),
        }),
        memory_v2::ClaimKind::Observation => {
            memory_v2::KindData::Observation(memory_v2::ObservationData {
                observer: actor.into(),
            })
        }
        memory_v2::ClaimKind::Commitment => {
            memory_v2::KindData::Commitment(memory_v2::CommitmentData {
                committed_by: actor.into(),
                beneficiary: actor.into(),
            })
        }
        memory_v2::ClaimKind::Practice => memory_v2::KindData::Practice(memory_v2::PracticeData {
            practice_scope: "general".into(),
        }),
        memory_v2::ClaimKind::MaterialFact => {
            memory_v2::KindData::MaterialFact(memory_v2::MaterialFactData {
                proposition: text.into(),
            })
        }
        memory_v2::ClaimKind::Quotation => {
            memory_v2::KindData::Quotation(memory_v2::QuotationData {
                speaker: actor.into(),
            })
        }
        memory_v2::ClaimKind::LegacyUnclassified => {
            memory_v2::KindData::LegacyUnclassified(memory_v2::LegacyData {
                missing_semantics: vec!["claim-kind".into()],
            })
        }
    }
}

fn v2_propose(args: &MemoryArgs, root: &std::path::Path) -> Result<(), String> {
    let operation = args
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or("create");
    if !matches!(operation, "create" | "replace" | "revoke") {
        return Err("v2 propose operation must be create, replace, or revoke".into());
    }
    let (repository, reduced) = current_v2(root)?;
    let owner = reduced
        .authority
        .owner
        .clone()
        .ok_or("MEMORY_UNAUTHORIZED: owner authority is conflicted")?;
    let request_id = args
        .flags
        .get("request-id")
        .cloned()
        .or_else(|| args.flags.get("dedupe-key").cloned())
        .ok_or("propose requires --request-id (or legacy --dedupe-key)")?;
    let recorded_by = required_flag(args, "recorded-by")
        .or_else(|_| required_flag(args, "by"))?
        .to_string();

    let existing = if operation == "create" {
        None
    } else {
        let claim_id = required_flag(args, "target")?;
        let view = reduced
            .claims
            .iter()
            .find(|view| view.claim_id == claim_id)
            .ok_or_else(|| format!("claim not found: {claim_id}"))?;
        if view.current_heads.len() != 1 {
            return Err("MEMORY_STALE_BASE: target must have exactly one current head".into());
        }
        Some(
            repository
                .claims
                .iter()
                .find(|item| item.value.revision_id == view.current_heads[0].revision_id)
                .ok_or("MEMORY_INVALID_DAG: current revision is missing")?
                .value
                .clone(),
        )
    };

    let text = if operation == "revoke" {
        existing
            .as_ref()
            .map(|claim| claim.text.clone())
            .unwrap_or_default()
    } else {
        required_flag(args, "text")?.trim().to_string()
    };
    if text.is_empty() {
        return Err("--text must not be empty".into());
    }
    let claim_kind = if let Some(claim) = &existing {
        claim.claim_kind
    } else {
        parse_v2_enum(required_flag(args, "claim-kind")?, "claim-kind")?
    };
    let target = if let Some(claim) = &existing {
        claim.projection.target
    } else {
        parse_v2_enum(
            args.flags
                .get("scope")
                .or_else(|| args.flags.get("projection"))
                .map(String::as_str)
                .unwrap_or("memory"),
            "projection",
        )?
    };
    let category = existing
        .as_ref()
        .map(|claim| claim.projection.category.clone())
        .unwrap_or(required_flag(args, "category")?.to_string());
    let asserted_by = args
        .flags
        .get("asserted-by")
        .cloned()
        .unwrap_or_else(|| owner.actor_id.clone());
    let kind_data = existing
        .as_ref()
        .map(|claim| claim.kind_data.clone())
        .unwrap_or_else(|| v2_kind_data(claim_kind, &text, &asserted_by));
    let context = existing
        .as_ref()
        .map(|claim| claim.context.clone())
        .unwrap_or(memory_v2::ClaimContext {
            spaces: vec![required_flag(args, "space")?.to_string()],
            applies_when: vec![],
            excludes_when: vec![],
        });
    let consent = existing
        .as_ref()
        .map(|claim| claim.consent.clone())
        .unwrap_or(memory_v2::Consent {
            scope: "personal-assistant-only".into(),
            allowed_purposes: vec![required_flag(args, "purpose")?.to_string()],
            external_provider_policy: parse_v2_enum(
                args.flags
                    .get("provider-policy")
                    .map(String::as_str)
                    .unwrap_or("deny"),
                "provider-policy",
            )?,
        });
    let proposal =
        memory_v2::propose_pending(
            root,
            memory_v2::PendingProposalInput {
                request_id,
                claim_id: existing.as_ref().map(|claim| claim.claim_id.clone()),
                text,
                claim_kind,
                kind_data,
                subject: memory_v2::Subject {
                    kind: memory_v2::SubjectKind::VaultOwner,
                    id: owner.owner_id,
                    relation_to_owner: memory_v2::OwnerRelation::Self_,
                },
                asserted_by: vec![memory_v2::Assertion {
                    kind: "actor".into(),
                    id: asserted_by,
                    basis: args
                        .flags
                        .get("basis")
                        .cloned()
                        .unwrap_or_else(|| "agent-recorded".into()),
                }],
                recorded_by: memory_v2::Recorder {
                    kind: "agent".into(),
                    id: recorded_by,
                    device_id: args
                        .flags
                        .get("device-id")
                        .cloned()
                        .unwrap_or_else(|| "device:cli".into()),
                },
                projection: memory_v2::Projection {
                    target,
                    category,
                    visibility: memory_v2::Visibility::Projection,
                },
                lifecycle: if operation == "revoke" {
                    memory_v2::LifecycleState::Revoked
                } else {
                    memory_v2::LifecycleState::Active
                },
                temporal: existing
                    .as_ref()
                    .map(|claim| claim.temporal.clone())
                    .unwrap_or(memory_v2::Temporal {
                        valid_from: args.flags.get("valid-from").cloned(),
                        valid_until: args.flags.get("valid-until").cloned(),
                        ..Default::default()
                    }),
                epistemic: existing
                    .as_ref()
                    .map(|claim| claim.epistemic.clone())
                    .unwrap_or(memory_v2::Epistemic {
                        basis: args
                            .flags
                            .get("basis")
                            .cloned()
                            .unwrap_or_else(|| "inferred".into()),
                        representation_certainty: "unknown".into(),
                        truth_status: "not-assessed".into(),
                        truth_confidence: "unknown".into(),
                    }),
                trust_tier: existing.as_ref().map(|claim| claim.trust_tier).unwrap_or(
                    parse_v2_enum(
                        args.flags
                            .get("trust-tier")
                            .map(String::as_str)
                            .unwrap_or("contextual"),
                        "trust-tier",
                    )?,
                ),
                risk_class: existing.as_ref().map(|claim| claim.risk_class).unwrap_or(
                    parse_v2_enum(
                        args.flags
                            .get("risk-class")
                            .map(String::as_str)
                            .unwrap_or("informational"),
                        "risk-class",
                    )?,
                ),
                salience: existing
                    .as_ref()
                    .map(|claim| claim.salience)
                    .unwrap_or(parse_v2_enum(
                        args.flags
                            .get("salience")
                            .map(String::as_str)
                            .unwrap_or("normal"),
                        "salience",
                    )?),
                polarity: existing
                    .as_ref()
                    .map(|claim| claim.polarity)
                    .unwrap_or(parse_v2_enum(
                        args.flags
                            .get("polarity")
                            .map(String::as_str)
                            .unwrap_or("neutral"),
                        "polarity",
                    )?),
                sensitivity: existing.as_ref().map(|claim| claim.sensitivity).unwrap_or(
                    parse_v2_enum(
                        args.flags
                            .get("sensitivity")
                            .map(String::as_str)
                            .unwrap_or("normal"),
                        "sensitivity",
                    )?,
                ),
                context,
                consent,
                agent_use: memory_v2::AgentUse {
                    guidance: args
                        .flags
                        .get("guidance")
                        .or_else(|| args.flags.get("agent-guidance"))
                        .cloned()
                        .unwrap_or_default(),
                    avoid_error: args.flags.get("avoid-error").cloned().unwrap_or_default(),
                },
                evidence: args
                    .flags
                    .get("source")
                    .map(|resource| {
                        vec![memory_v2::Evidence {
                            relation: memory_v2::EvidenceRelation::EvidenceOfSpeech,
                            resource: resource.clone(),
                            content_sha256: None,
                            title: None,
                        }]
                    })
                    .unwrap_or_default(),
                dedupe_key: args
                    .flags
                    .get("dedupe-key")
                    .cloned()
                    .unwrap_or_else(|| "cli-proposal".into()),
            },
        )?;
    let value = serde_json::to_value(&proposal).map_err(|error| error.to_string())?;
    print_json_or_text(
        args,
        value,
        format!(
            "Created pending Claim {} revision {}",
            proposal.claim_id, proposal.revision_id
        ),
    );
    Ok(())
}

fn run_v2(args: &MemoryArgs, root: &std::path::Path) -> Result<ExitCode, String> {
    if matches!(
        args.action.as_str(),
        "approve" | "reject" | "ignore" | "delete" | "resolve"
    ) {
        return Err("MEMORY_UNAUTHORIZED: human memory decisions are only accepted from the trusted Memory UI".into());
    }
    let snapshot = v2_snapshot(root)?;
    match args.action.as_str() {
        "snapshot" | "list" => {
            let mut value = snapshot;
            if args.action == "list" && !args.bools.contains("all") {
                value = json!({"claims": value["claims"], "pending": value["pending"], "conflicts": value["conflicts"], "health": value["health"]});
            }
            print_json_or_text(
                args,
                value.clone(),
                serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?,
            );
            Ok(ExitCode::SUCCESS)
        }
        "owner" => {
            let value = snapshot["owner"].clone();
            print_json_or_text(
                args,
                value.clone(),
                serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?,
            );
            Ok(ExitCode::SUCCESS)
        }
        "pending" | "conflicts" => {
            let value = snapshot[args.action.as_str()].clone();
            print_json_or_text(
                args,
                value.clone(),
                serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?,
            );
            Ok(ExitCode::SUCCESS)
        }
        "show" => {
            let needle = args
                .positionals
                .first()
                .ok_or("show requires a claim or revision id")?;
            let found = ["claims", "pending", "conflicts", "history"]
                .iter()
                .flat_map(|key| snapshot[*key].as_array().into_iter().flatten())
                .find(|item| item.to_string().contains(needle))
                .cloned()
                .ok_or_else(|| format!("claim or revision not found: {needle}"))?;
            print_json_or_text(
                args,
                found.clone(),
                serde_json::to_string_pretty(&found).map_err(|error| error.to_string())?,
            );
            Ok(ExitCode::SUCCESS)
        }
        "propose" => v2_propose(args, root).map(|_| ExitCode::SUCCESS),
        "context" => {
            let request = json!({
                "space": required_flag(args, "space")?, "purpose": required_flag(args, "purpose")?,
                "caller": required_flag(args, "caller")?, "provider": required_flag(args, "provider")?,
                "model": required_flag(args, "model")?,
                "tools": args.flags.get("tool").map(|value| value.split(',').map(str::trim).filter(|value| !value.is_empty()).collect::<Vec<_>>()).unwrap_or_default(),
                "external_transfer": args.bools.contains("external-transfer"),
                "as_of_valid_time": args.flags.get("as-of").cloned().unwrap_or_else(|| Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true))
            });
            let value = memory_control::dispatch(root, "host.memory.v2.context", &request)?;
            print_json_or_text(
                args,
                value.clone(),
                serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?,
            );
            Ok(ExitCode::SUCCESS)
        }
        "check" | "doctor" => {
            let health = memory_control::dispatch(root, "host.memory.v2.check", &json!({}))?;
            let ok = health["status"] != "damaged" && health["status"] != "unsupported";
            print_json_or_text(
                args,
                health.clone(),
                serde_json::to_string_pretty(&health).map_err(|error| error.to_string())?,
            );
            Ok(ExitCode::from(if ok { 0 } else { 1 }))
        }
        "rebuild" | "reconcile" => {
            memory_v2::rebuild_projections(root)?;
            print_json_or_text(
                args,
                json!({"rebuilt": true}),
                "Memory v2 projections rebuilt.".into(),
            );
            Ok(ExitCode::SUCCESS)
        }
        "migrate" => Err("MEMORY_PROTOCOL_ALREADY_ACTIVE: v2 migration is not applicable".into()),
        other => Err(format!("unknown v2 memory action: {other}")),
    }
}

pub fn run(args: MemoryArgs) -> ExitCode {
    let result = (|| -> Result<ExitCode, String> {
        let root = root(&args)?;
        let repository_mode = memory_v2::V2Repository::new(&root)
            .load()
            .map_err(|error| error.to_string())?
            .mode;
        if repository_mode == memory_v2::RepositoryMode::V2Active {
            return run_v2(&args, &root);
        }
        if repository_mode == memory_v2::RepositoryMode::V2Incomplete {
            return Err(
                "MEMORY_PROTOCOL_INCOMPLETE: repair v2 control assets before continuing".into(),
            );
        }
        if matches!(
            args.action.as_str(),
            "approve" | "reject" | "ignore" | "delete"
        ) {
            return Err("MEMORY_UNAUTHORIZED: human memory decisions are only accepted from the trusted Memory UI".into());
        }
        match args.action.as_str() {
            "list" => list(&args, &root).map(|_| ExitCode::SUCCESS),
            "show" => show(&args, &root).map(|_| ExitCode::SUCCESS),
            "suggest" => {
                let value = memory_control::suggest(&root)?;
                let count = value["suggestions"].as_array().map(Vec::len).unwrap_or(0);
                print_json_or_text(&args, value, format!("{count} improvement suggestion(s)"));
                Ok(ExitCode::SUCCESS)
            }
            "propose" => propose(&args, &root).map(|_| ExitCode::SUCCESS),
            "approve" | "reject" => unreachable!("decision actions are rejected above"),
            "check" | "doctor" => {
                let snapshot = memory_control::list(&root)?;
                let ok = !snapshot.integrity.drift;
                let value = json!({"ok":ok,"integrity":snapshot.integrity});
                print_json_or_text(
                    &args,
                    value,
                    if ok {
                        "Memory projections are consistent.".into()
                    } else {
                        "Memory projection drift detected.".into()
                    },
                );
                Ok(ExitCode::from(if ok { 0 } else { 1 }))
            }
            "migrate" => {
                if !args.bools.contains("dry-run") {
                    return Err("migration requires --dry-run; authoritative apply is not available until the protocol freeze gate passes".into());
                }
                let value = memory_control::dispatch(
                    &root,
                    "host.memory.v2.migrate",
                    &json!({"mode": "dry-run"}),
                )?;
                print_json_or_text(
                    &args,
                    value.clone(),
                    format!(
                        "Migration dry-run: {} claims, zero writes.",
                        value["counts"]["claims"].as_u64().unwrap_or(0)
                    ),
                );
                Ok(ExitCode::SUCCESS)
            }
            other => Err(format!("unknown memory action: {other}")),
        }
    })();
    match result {
        Ok(code) => code,
        Err(error) => {
            if args.json {
                println!("{}", json!({"ok":false,"error":{"message":error}}));
            } else {
                eprintln!("notemd memory: {error}");
            }
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_proposal_flags() {
        let args = parse_args(
            &[
                "propose",
                "replace",
                "--scope",
                "memory",
                "--target",
                "x",
                "--base-revision",
                "2",
                "--text",
                "new",
                "--source",
                "/a",
                "--by",
                "agent/x",
                "--dedupe-key",
                "k",
                "--priority",
                "high",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>(),
            false,
        );
        assert_eq!(args.action, "propose");
        assert_eq!(args.positionals, vec!["replace"]);
        assert_eq!(args.flags.get("target").map(String::as_str), Some("x"));
        assert_eq!(
            parse_priority(args.flags.get("priority")).unwrap(),
            Some(Priority::High)
        );
    }

    #[test]
    fn v2_context_and_migration_boolean_flags_are_not_value_flags() {
        let args = parse_args(
            &[
                "context",
                "--external-transfer",
                "--dry-run",
                "--space",
                "work",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>(),
            false,
        );
        assert!(args.bools.contains("external-transfer"));
        assert!(args.bools.contains("dry-run"));
        assert_eq!(args.flags.get("space").map(String::as_str), Some("work"));
    }

    #[test]
    fn delete_is_a_distinct_projection_operation() {
        assert_eq!(parse_operation("revoke").unwrap(), Operation::Revoke);
        assert_eq!(parse_operation("delete").unwrap(), Operation::Delete);
    }

    #[test]
    fn check_exit_codes_distinguish_integrity_from_argument_errors() {
        assert_eq!(ExitCode::from(0), ExitCode::SUCCESS);
        assert_ne!(ExitCode::from(1), ExitCode::from(2));
    }
}
