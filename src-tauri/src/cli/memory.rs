//! `notemd memory` — controlled USER/MEMORY CLI.
//!
//! This is core rather than plugin-provided so an Agent's proposal path does
//! not disappear when the Memory window plugin is disabled. The window and CLI
//! still share `crate::memory_control` for every state transition.

use crate::memory_control::{self, v2 as memory_v2};
use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[derive(Debug, Clone, Default)]
pub struct MemoryArgs {
    pub action: String,
    pub positionals: Vec<String>,
    pub vault: Option<String>,
    pub json: bool,
    pub flags: HashMap<String, String>,
    pub bools: HashSet<String>,
    pub errors: Vec<String>,
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
        } else if matches!(token.as_str(), "--all" | "--external-transfer") {
            let name = token.trim_start_matches("--").to_string();
            if !out.bools.insert(name.clone()) {
                out.errors
                    .push(format!("--{name} may only be specified once"));
            }
        } else if token.starts_with("--") {
            let name = token.trim_start_matches("--");
            match rest.get(i + 1).filter(|value| !value.starts_with("--")) {
                Some(value) => {
                    if out.flags.insert(name.to_string(), value.clone()).is_some() {
                        out.errors
                            .push(format!("--{name} may only be specified once"));
                    }
                    if token == "--vault" {
                        out.vault = Some(value.clone());
                    }
                    i += 1;
                }
                None => out.errors.push(format!("--{name} requires a value")),
            }
        } else if token.starts_with('-') {
            out.errors.push(format!("unsupported flag: {token}"));
        } else {
            out.positionals.push(token.clone());
        }
        i += 1;
    }
    out
}

const ACTIONS: &[&str] = &[
    "snapshot",
    "list",
    "show",
    "owner",
    "pending",
    "conflicts",
    "propose",
    "context",
    "context-manifest",
    "check",
    "doctor",
    "rebuild",
    "reconcile",
    "purge-plan",
];

const PROPOSE_COMMON_FLAGS: &[&str] = &[
    "vault",
    "request-id",
    "recorded-by",
    "asserted-by",
    "basis",
    "device-id",
    "source",
    "guidance",
    "avoid-error",
];

fn validate_positionals(
    args: &MemoryArgs,
    min: usize,
    max: usize,
    usage: &str,
) -> Result<(), String> {
    if args.positionals.len() < min || args.positionals.len() > max {
        return Err(format!("usage: notemd memory {usage}"));
    }
    Ok(())
}

fn validate_allowed_flags(args: &MemoryArgs, allowed: &[&str]) -> Result<(), String> {
    let mut unknown = args
        .flags
        .keys()
        .filter(|name| !allowed.contains(&name.as_str()))
        .map(|name| format!("--{name}"))
        .collect::<Vec<_>>();
    unknown.extend(
        args.bools
            .iter()
            .filter(|name| !allowed.contains(&name.as_str()))
            .map(|name| format!("--{name}")),
    );
    unknown.sort();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "unsupported flag(s) for {}: {}",
            args.action,
            unknown.join(", ")
        ))
    }
}

fn validate_args(args: &MemoryArgs) -> Result<(), String> {
    if !args.errors.is_empty() {
        return Err(args.errors.join("; "));
    }
    if matches!(
        args.action.as_str(),
        "approve" | "reject" | "ignore" | "delete" | "resolve"
    ) {
        return Err("MEMORY_UNAUTHORIZED: human memory decisions are only accepted from the trusted Memory UI".into());
    }
    if !ACTIONS.contains(&args.action.as_str()) {
        return Err(format!("unknown memory action: {}", args.action));
    }

    let read_only = &["vault"];
    match args.action.as_str() {
        "snapshot" | "owner" | "pending" | "conflicts" | "check" | "doctor" | "rebuild"
        | "reconcile" => {
            validate_positionals(args, 0, 0, &args.action)?;
            validate_allowed_flags(args, read_only)?;
        }
        "show" | "context-manifest" | "purge-plan" => {
            validate_positionals(args, 1, 1, &format!("{} <id>", args.action))?;
            validate_allowed_flags(args, read_only)?;
        }
        "list" => {
            validate_positionals(args, 0, 0, "list [--scope user|memory] [--status STATUS]")?;
            validate_allowed_flags(args, &["vault", "scope", "status", "all"])?;
            if args.bools.contains("all") && args.flags.contains_key("status") {
                return Err("--all conflicts with --status".into());
            }
            if let Some(scope) = args.flags.get("scope") {
                if !matches!(scope.as_str(), "user" | "memory") {
                    return Err(format!("invalid --scope: {scope}; expected user or memory"));
                }
            }
            if let Some(status) = args.flags.get("status") {
                if !matches!(
                    status.as_str(),
                    "current" | "pending" | "revoked" | "deleted" | "conflict" | "all"
                ) {
                    return Err(format!(
                        "invalid --status: {status}; expected current, pending, revoked, deleted, conflict, or all"
                    ));
                }
            }
        }
        "context" => {
            validate_positionals(
                args,
                0,
                0,
                "context --space SPACE --purpose PURPOSE --caller CALLER",
            )?;
            validate_allowed_flags(
                args,
                &[
                    "vault",
                    "space",
                    "purpose",
                    "caller",
                    "provider",
                    "model",
                    "tool",
                    "as-of",
                    "external-transfer",
                ],
            )?;
            for name in ["space", "purpose", "caller"] {
                required_flag(args, name)?;
            }
            if args.bools.contains("external-transfer") {
                for name in ["provider", "model"] {
                    required_flag(args, name)
                        .map_err(|_| format!("--{name} is required with --external-transfer"))?;
                }
            } else if args.flags.get("provider").map(String::as_str) != Some("local")
                && args.flags.contains_key("provider")
            {
                return Err("non-local --provider requires --external-transfer".into());
            }
            if let Some(as_of) = args.flags.get("as-of") {
                chrono::DateTime::parse_from_rfc3339(as_of)
                    .map_err(|error| format!("invalid --as-of RFC3339 time: {error}"))?;
            }
        }
        "propose" => {
            validate_positionals(args, 1, 1, "propose <create|replace|revoke> [claim flags]")?;
            let operation = v2_propose_operation(args)?;
            let mut allowed = PROPOSE_COMMON_FLAGS.to_vec();
            match operation {
                "create" => allowed.extend([
                    "text",
                    "claim-kind",
                    "scope",
                    "category",
                    "space",
                    "purpose",
                    "provider-policy",
                    "trust-tier",
                    "risk-class",
                    "salience",
                    "polarity",
                    "sensitivity",
                    "valid-from",
                    "valid-until",
                ]),
                "replace" => allowed.extend(["target", "text"]),
                "revoke" => allowed.push("target"),
                _ => unreachable!(),
            }
            validate_allowed_flags(args, &allowed)?;
            for name in ["request-id", "recorded-by"] {
                required_flag(args, name)?;
            }
            match operation {
                "create" => {
                    for name in ["text", "claim-kind", "category", "space", "purpose"] {
                        required_flag(args, name)?;
                    }
                    let _: memory_v2::ClaimKind =
                        parse_v2_enum(required_flag(args, "claim-kind")?, "claim-kind")?;
                    let _: memory_v2::ProjectionTarget = parse_v2_enum(
                        args.flags
                            .get("scope")
                            .map(String::as_str)
                            .unwrap_or("memory"),
                        "scope",
                    )?;
                    let _: memory_v2::ExternalProviderPolicy = parse_v2_enum(
                        args.flags
                            .get("provider-policy")
                            .map(String::as_str)
                            .unwrap_or("deny"),
                        "provider-policy",
                    )?;
                    let _: memory_v2::TrustTier = parse_v2_enum(
                        args.flags
                            .get("trust-tier")
                            .map(String::as_str)
                            .unwrap_or("contextual"),
                        "trust-tier",
                    )?;
                    let _: memory_v2::RiskClass = parse_v2_enum(
                        args.flags
                            .get("risk-class")
                            .map(String::as_str)
                            .unwrap_or("informational"),
                        "risk-class",
                    )?;
                    let _: memory_v2::Salience = parse_v2_enum(
                        args.flags
                            .get("salience")
                            .map(String::as_str)
                            .unwrap_or("normal"),
                        "salience",
                    )?;
                    let _: memory_v2::Polarity = parse_v2_enum(
                        args.flags
                            .get("polarity")
                            .map(String::as_str)
                            .unwrap_or("neutral"),
                        "polarity",
                    )?;
                    let sensitivity: memory_v2::Sensitivity = parse_v2_enum(
                        args.flags
                            .get("sensitivity")
                            .map(String::as_str)
                            .unwrap_or("normal"),
                        "sensitivity",
                    )?;
                    if sensitivity == memory_v2::Sensitivity::Restricted {
                        return Err(
                            "MEMORY_RESTRICTED_PERSISTENCE_DENIED: restricted content cannot enter Git"
                                .into(),
                        );
                    }
                    for name in ["valid-from", "valid-until"] {
                        if let Some(value) = args.flags.get(name) {
                            chrono::DateTime::parse_from_rfc3339(value).map_err(|error| {
                                format!("invalid --{name} RFC3339 time: {error}")
                            })?;
                        }
                    }
                }
                "replace" => {
                    required_flag(args, "target")?;
                    required_flag(args, "text")?;
                }
                "revoke" => {
                    required_flag(args, "target")?;
                }
                _ => unreachable!(),
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn root(args: &MemoryArgs) -> Result<PathBuf, String> {
    let root = super::search::resolve_vault_root(args.vault.as_deref())
        .ok_or("no vault configured; pass --vault PATH or configure one in Preferences")?;
    if !root.is_dir() {
        return Err(format!("vault not found: {}", root.display()));
    }
    Ok(root)
}

fn print_json_or_text(args: &MemoryArgs, value: serde_json::Value, text: String) {
    if args.json {
        println!("{}", json!({"ok":true,"data":value}));
    } else {
        println!("{text}");
    }
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
    match args.flags.get(name).map(String::as_str) {
        Some(value) if !value.trim().is_empty() => Ok(value),
        Some(_) => Err(format!("--{name} must not be empty")),
        None => Err(format!("--{name} is required")),
    }
}

/// A required flag carrying a comma-separated list, so one Claim can allow the
/// several retrieval purposes the Memory UI already offers.
fn required_list(args: &MemoryArgs, name: &str) -> Result<Vec<String>, String> {
    let values = required_flag(args, name)?
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err(format!("--{name} must not be empty"));
    }
    Ok(values)
}

fn v2_propose_operation(args: &MemoryArgs) -> Result<&str, String> {
    let operation = args
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or("create");
    if matches!(operation, "create" | "replace" | "revoke") {
        Ok(operation)
    } else {
        Err("v2 propose operation must be create, replace, or revoke".into())
    }
}

fn projection_allows_auto_initialize(root: &std::path::Path, name: &str) -> Result<(), String> {
    let path = root.join(name);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("MEMORY_IO: {}: {error}", path.display())),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "MEMORY_AUTO_INITIALIZE_BLOCKED: {} is not a regular projection file",
            path.display()
        ));
    }
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("MEMORY_IO: {}: {error}", path.display()))?;
    let expected_heading = format!("# {}", name.trim_end_matches(".md"));
    if content.trim().is_empty() || content.trim() == expected_heading {
        Ok(())
    } else {
        Err(format!(
            "MEMORY_AUTO_INITIALIZE_BLOCKED: {} already contains content; initialize from the trusted Memory UI so it is not overwritten",
            path.display()
        ))
    }
}

fn validate_auto_initialize_create(
    args: &MemoryArgs,
    root: &std::path::Path,
) -> Result<(), String> {
    if v2_propose_operation(args)? != "create" {
        return Err(
            "MEMORY_PROTOCOL_UNINITIALIZED: only propose create can initialize an empty Memory v2 repository"
                .into(),
        );
    }
    let request_id = required_flag(args, "request-id")?;
    if request_id.trim().is_empty() || request_id.len() > 256 {
        return Err(
            "MEMORY_INVALID_REQUEST: request_id is required and must be at most 256 bytes".into(),
        );
    }
    required_flag(args, "recorded-by")?;
    if required_flag(args, "text")?.trim().is_empty() {
        return Err("--text must not be empty".into());
    }
    let _: memory_v2::ClaimKind = parse_v2_enum(required_flag(args, "claim-kind")?, "claim-kind")?;
    let _: memory_v2::ProjectionTarget = parse_v2_enum(
        args.flags
            .get("scope")
            .map(String::as_str)
            .unwrap_or("memory"),
        "projection",
    )?;
    required_flag(args, "category")?;
    required_flag(args, "space")?;
    required_flag(args, "purpose")?;
    let _: memory_v2::ExternalProviderPolicy = parse_v2_enum(
        args.flags
            .get("provider-policy")
            .map(String::as_str)
            .unwrap_or("deny"),
        "provider-policy",
    )?;
    let _: memory_v2::TrustTier = parse_v2_enum(
        args.flags
            .get("trust-tier")
            .map(String::as_str)
            .unwrap_or("contextual"),
        "trust-tier",
    )?;
    let _: memory_v2::RiskClass = parse_v2_enum(
        args.flags
            .get("risk-class")
            .map(String::as_str)
            .unwrap_or("informational"),
        "risk-class",
    )?;
    let _: memory_v2::Salience = parse_v2_enum(
        args.flags
            .get("salience")
            .map(String::as_str)
            .unwrap_or("normal"),
        "salience",
    )?;
    let _: memory_v2::Polarity = parse_v2_enum(
        args.flags
            .get("polarity")
            .map(String::as_str)
            .unwrap_or("neutral"),
        "polarity",
    )?;
    let sensitivity: memory_v2::Sensitivity = parse_v2_enum(
        args.flags
            .get("sensitivity")
            .map(String::as_str)
            .unwrap_or("normal"),
        "sensitivity",
    )?;
    if sensitivity == memory_v2::Sensitivity::Restricted {
        return Err(
            "MEMORY_RESTRICTED_PERSISTENCE_DENIED: restricted content cannot enter Git".into(),
        );
    }
    projection_allows_auto_initialize(root, "USER.md")?;
    projection_allows_auto_initialize(root, "MEMORY.md")?;
    Ok(())
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
    }
}

fn v2_propose(args: &MemoryArgs, root: &std::path::Path) -> Result<(), String> {
    let operation = v2_propose_operation(args)?;
    let (repository, reduced) = current_v2(root)?;
    let owner = reduced
        .authority
        .owner
        .clone()
        .ok_or("MEMORY_UNAUTHORIZED: owner authority is conflicted")?;
    let request_id = required_flag(args, "request-id")?.to_string();
    let recorded_by = required_flag(args, "recorded-by")?.to_string();

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
                .map(String::as_str)
                .unwrap_or("memory"),
            "projection",
        )?
    };
    let category = match existing.as_ref() {
        Some(claim) => claim.projection.category.clone(),
        None => required_flag(args, "category")?.to_string(),
    };
    let asserted_by = args
        .flags
        .get("asserted-by")
        .cloned()
        .unwrap_or_else(|| owner.actor_id.clone());
    let kind_data = existing
        .as_ref()
        .map(|claim| claim.kind_data.clone())
        .unwrap_or_else(|| v2_kind_data(claim_kind, &text, &asserted_by));
    let context = match existing.as_ref() {
        Some(claim) => claim.context.clone(),
        None => memory_v2::ClaimContext {
            spaces: vec![required_flag(args, "space")?.to_string()],
            applies_when: vec![],
            excludes_when: vec![],
        },
    };
    let consent = match existing.as_ref() {
        Some(claim) => claim.consent.clone(),
        None => memory_v2::Consent {
            scope: "personal-assistant-only".into(),
            allowed_purposes: required_list(args, "purpose")?,
            external_provider_policy: parse_v2_enum(
                args.flags
                    .get("provider-policy")
                    .map(String::as_str)
                    .unwrap_or("deny"),
                "provider-policy",
            )?,
        },
    };
    let proposal =
        memory_v2::propose_pending(
            root,
            memory_v2::PendingProposalInput {
                request_id: request_id.clone(),
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
                    guidance: args.flags.get("guidance").cloned().unwrap_or_default(),
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
                dedupe_key: request_id,
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

fn projection_target(value: &Value) -> Option<&str> {
    value
        .pointer("/claim/projection/target")
        .or_else(|| value.pointer("/revision/projection/target"))
        .and_then(Value::as_str)
}

fn list_value(args: &MemoryArgs, snapshot: &Value) -> Value {
    let scope = args.flags.get("scope").map(String::as_str);
    let status = if args.bools.contains("all") {
        "all"
    } else {
        args.flags
            .get("status")
            .map(String::as_str)
            .unwrap_or("all")
    };
    let scope_matches = |item: &Value| scope.is_none() || projection_target(item) == scope;

    let claims = snapshot["claims"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| scope_matches(item))
        .filter(|item| match status {
            "all" => true,
            "current" => {
                item["application_state"] == "current"
                    && item
                        .pointer("/claim/lifecycle/state")
                        .and_then(Value::as_str)
                        == Some("active")
            }
            "revoked" => {
                item.pointer("/claim/lifecycle/state")
                    .and_then(Value::as_str)
                    == Some("revoked")
            }
            "deleted" => {
                item.pointer("/claim/lifecycle/state")
                    .and_then(Value::as_str)
                    == Some("deleted")
            }
            "pending" | "conflict" => false,
            _ => false,
        })
        .cloned()
        .collect::<Vec<_>>();
    let pending = snapshot["pending"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| scope_matches(item) && matches!(status, "all" | "pending"))
        .cloned()
        .collect::<Vec<_>>();
    let conflicts = snapshot["conflicts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| {
            let scope_ok = scope.is_none()
                || item["heads"].as_array().into_iter().flatten().any(|head| {
                    head.pointer("/projection/target").and_then(Value::as_str) == scope
                });
            scope_ok && matches!(status, "all" | "conflict")
        })
        .cloned()
        .collect::<Vec<_>>();
    json!({
        "claims": claims,
        "pending": pending,
        "conflicts": conflicts,
        "health": snapshot["health"],
        "filters": {"scope": scope, "status": status}
    })
}

fn show_exact(root: &Path, snapshot: &Value, needle: &str) -> Result<Value, String> {
    let repository = memory_v2::V2Repository::new(root)
        .load()
        .map_err(|error| error.to_string())?;
    if let Some(revision) = repository
        .claims
        .iter()
        .find(|item| item.value.revision_id == needle)
    {
        return serde_json::to_value(&revision.value).map_err(|error| error.to_string());
    }
    if let Some(claim) = snapshot["claims"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item.pointer("/claim/claim_id").and_then(Value::as_str) == Some(needle))
    {
        return Ok(claim.clone());
    }
    if let Some(pending) = snapshot["pending"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item.pointer("/revision/claim_id").and_then(Value::as_str) == Some(needle))
    {
        return Ok(pending.clone());
    }
    if let Some(conflict) = snapshot["conflicts"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["claim_id"].as_str() == Some(needle))
    {
        return Ok(conflict.clone());
    }
    Err(format!("claim or revision not found: {needle}"))
}

fn load_context_manifest(root: &Path, manifest_id: &str) -> Result<Value, String> {
    let repository = memory_v2::V2Repository::new(root)
        .load()
        .map_err(|error| error.to_string())?;
    let manifest = repository
        .context_manifests
        .iter()
        .find(|item| item.value.manifest_id == manifest_id)
        .ok_or_else(|| format!("context manifest not found: {manifest_id}"))?;
    serde_json::to_value(&manifest.value).map_err(|error| error.to_string())
}

fn check_passes(health: &Value) -> bool {
    health["status"] != "damaged"
        && health["status"] != "unsupported"
        && !health["projection_edited"].as_bool().unwrap_or(false)
}

fn path_for_output(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn collect_matching_local_files(
    root: &Path,
    dir: &Path,
    needles: &BTreeSet<String>,
    output: &mut BTreeSet<String>,
) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("MEMORY_IO: {}: {error}", dir.display())),
    };
    for entry in entries {
        let entry = entry.map_err(|error| format!("MEMORY_IO: {}: {error}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("MEMORY_IO: {}: {error}", path.display()))?;
        if file_type.is_symlink() {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|error| format!("MEMORY_IO: {}: {error}", path.display()))?;
        if file_type.is_dir() {
            collect_matching_local_files(root, &path, needles, output)?;
        } else if file_type.is_file() && metadata.len() <= 1024 * 1024 {
            let bytes = fs::read(&path)
                .map_err(|error| format!("MEMORY_IO: {}: {error}", path.display()))?;
            if needles.iter().any(|needle| {
                bytes
                    .windows(needle.len())
                    .any(|window| window == needle.as_bytes())
            }) {
                output.insert(path_for_output(root, &path));
            }
        }
    }
    Ok(())
}

fn git_lines(root: &Path, args: &[String]) -> Vec<String> {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn purge_plan(root: &Path, claim_id: &str) -> Result<Value, String> {
    let repository = memory_v2::V2Repository::new(root)
        .load()
        .map_err(|error| error.to_string())?;
    if !repository
        .claims
        .iter()
        .any(|item| item.value.claim_id == claim_id)
    {
        return Err(format!("claim not found: {claim_id}"));
    }

    let mut claim_ids = BTreeSet::from([claim_id.to_string()]);
    let mut revision_ids = repository
        .claims
        .iter()
        .filter(|item| item.value.claim_id == claim_id)
        .map(|item| item.value.revision_id.clone())
        .collect::<BTreeSet<_>>();
    let mut operation_ids = BTreeSet::new();
    loop {
        let before = (claim_ids.len(), revision_ids.len(), operation_ids.len());
        for item in &repository.claims {
            let derived = item
                .value
                .parents
                .iter()
                .any(|parent| revision_ids.contains(&parent.revision_id))
                || item.value.lineage.derived_from.iter().any(|source| {
                    claim_ids.contains(&source.claim_id)
                        || revision_ids.contains(&source.revision_id)
                });
            if claim_ids.contains(&item.value.claim_id) || derived {
                claim_ids.insert(item.value.claim_id.clone());
                revision_ids.insert(item.value.revision_id.clone());
            }
        }
        for operation in &repository.operations {
            let value = &operation.value;
            let refs =
                value
                    .merge_inputs
                    .sources
                    .iter()
                    .chain(std::iter::once(&value.merge_inputs.target))
                    .flat_map(|input| {
                        std::iter::once(input.claim_id.as_str()).chain(
                            input
                                .base_heads
                                .iter()
                                .map(|head| head.revision_id.as_str()),
                        )
                    })
                    .chain(std::iter::once(value.result.claim_id.as_str()))
                    .chain(std::iter::once(value.result.revision_id.as_str()))
                    .chain(value.effects.iter().flat_map(|effect| {
                        [
                            effect.claim_id.as_str(),
                            effect.revision_id.as_str(),
                            effect.merged_into.as_str(),
                        ]
                    }))
                    .chain(value.lineage.iter().flat_map(|source| {
                        [source.claim_id.as_str(), source.revision_id.as_str()]
                    }));
            if refs
                .into_iter()
                .any(|id| claim_ids.contains(id) || revision_ids.contains(id))
            {
                operation_ids.insert(value.operation_id.clone());
                claim_ids.insert(value.result.claim_id.clone());
                revision_ids.insert(value.result.revision_id.clone());
                for effect in &value.effects {
                    claim_ids.insert(effect.claim_id.clone());
                    revision_ids.insert(effect.revision_id.clone());
                }
            }
        }
        if before == (claim_ids.len(), revision_ids.len(), operation_ids.len()) {
            break;
        }
    }

    let claim_revisions = repository
        .claims
        .iter()
        .filter(|item| revision_ids.contains(&item.value.revision_id))
        .map(|item| {
            json!({
                "claim_id": item.value.claim_id,
                "revision_id": item.value.revision_id,
                "payload_sha256": item.value.payload_sha256,
                "path": path_for_output(root, &item.path)
            })
        })
        .collect::<Vec<_>>();
    let operations = repository
        .operations
        .iter()
        .filter(|item| operation_ids.contains(&item.value.operation_id))
        .map(|item| {
            json!({"operation_id": item.value.operation_id, "path": path_for_output(root, &item.path)})
        })
        .collect::<Vec<_>>();
    let context_manifests = repository
        .context_manifests
        .iter()
        .filter(|item| {
            item.value.selected.iter().any(|selected| {
                claim_ids.contains(&selected.claim_id)
                    || revision_ids.contains(&selected.revision_id)
            })
        })
        .map(|item| {
            json!({"manifest_id": item.value.manifest_id, "path": path_for_output(root, &item.path)})
        })
        .collect::<Vec<_>>();
    let projections = repository
        .claims
        .iter()
        .filter(|item| revision_ids.contains(&item.value.revision_id))
        .map(|item| match item.value.projection.target {
            memory_v2::ProjectionTarget::User => "USER.md",
            memory_v2::ProjectionTarget::Memory => "MEMORY.md",
        })
        .collect::<BTreeSet<_>>();

    let mut needles = claim_ids
        .iter()
        .chain(revision_ids.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    needles.extend(
        repository
            .claims
            .iter()
            .filter(|item| revision_ids.contains(&item.value.revision_id))
            .map(|item| item.value.payload_sha256.clone()),
    );
    let mut local_matches = BTreeSet::new();
    for relative in [
        ".notemd/memory/imports",
        ".notemd/memory/migrations",
        ".notemd/memory/legacy",
        ".notemd/memory/.local",
    ] {
        collect_matching_local_files(root, &root.join(relative), &needles, &mut local_matches)?;
    }

    let git_repository =
        git_lines(root, &["rev-parse".into(), "--is-inside-work-tree".into()]) == ["true"];
    let mut matching_commits = BTreeSet::new();
    let mut matching_reflog_commits = BTreeSet::new();
    if git_repository {
        for needle in &needles {
            let pickaxe = format!("-S{needle}");
            matching_commits.extend(git_lines(
                root,
                &[
                    "log".into(),
                    "--all".into(),
                    "--format=%H".into(),
                    pickaxe.clone(),
                    "--".into(),
                    ".notemd/memory".into(),
                    "USER.md".into(),
                    "MEMORY.md".into(),
                ],
            ));
            matching_reflog_commits.extend(git_lines(
                root,
                &[
                    "log".into(),
                    "-g".into(),
                    "--all".into(),
                    "--format=%H".into(),
                    pickaxe,
                    "--".into(),
                    ".notemd/memory".into(),
                    "USER.md".into(),
                    "MEMORY.md".into(),
                ],
            ));
        }
    }
    let remotes = if git_repository {
        git_lines(root, &["remote".into()])
    } else {
        Vec::new()
    };

    let generated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let safe_claim_id = claim_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let relative_plan_path = format!(
        ".notemd/memory/.local/purge/{}-{}.json",
        safe_claim_id,
        Utc::now().timestamp_millis()
    );
    let value = json!({
        "claim_id": claim_id,
        "generated_at": generated_at,
        "read_only": true,
        "plan_path": relative_plan_path,
        "affected_claim_ids": claim_ids,
        "authoritative_assets": {
            "claim_revisions": claim_revisions,
            "operations": operations,
            "context_manifests": context_manifests
        },
        "derived_assets": {
            "projections": projections,
            "local_import_migration_legacy_cache_matches": local_matches
        },
        "git": {
            "repository_detected": git_repository,
            "matching_commits_and_reachable_tags_or_branches": matching_commits,
            "matching_reflog_commits": matching_reflog_commits,
            "configured_remotes": remotes
        },
        "limitations": [
            "This plan does not rewrite Git history or delete any authoritative asset.",
            "Remote branches and other clones cannot be inspected locally; every remote and clone requires a separate purge audit.",
            "Encrypted backups, filesystem snapshots, caches outside the Vault, and copied exports require separate retention-system audits."
        ]
    });
    let plan_path = root.join(&relative_plan_path);
    if let Some(parent) = plan_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("MEMORY_IO: {}: {error}", parent.display()))?;
    }
    fs::write(
        &plan_path,
        serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("MEMORY_IO: {}: {error}", plan_path.display()))?;
    Ok(value)
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
        "snapshot" => {
            let value = snapshot;
            print_json_or_text(
                args,
                value.clone(),
                serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?,
            );
            Ok(ExitCode::SUCCESS)
        }
        "list" => {
            let value = list_value(args, &snapshot);
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
            let found = show_exact(root, &snapshot, needle)?;
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
                "caller": required_flag(args, "caller")?,
                "provider": args.flags.get("provider").map(String::as_str).unwrap_or("local"),
                "model": args.flags.get("model").map(String::as_str).unwrap_or("local"),
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
        "context-manifest" => {
            let manifest_id = args
                .positionals
                .first()
                .ok_or("context-manifest requires a manifest id")?;
            let value = load_context_manifest(root, manifest_id)?;
            print_json_or_text(
                args,
                value.clone(),
                serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?,
            );
            Ok(ExitCode::SUCCESS)
        }
        "check" | "doctor" => {
            let health = memory_control::dispatch(root, "host.memory.v2.check", &json!({}))?;
            let ok = check_passes(&health);
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
        "purge-plan" => {
            let claim_id = args
                .positionals
                .first()
                .ok_or("purge-plan requires a claim id")?;
            let value = purge_plan(root, claim_id)?;
            print_json_or_text(
                args,
                value.clone(),
                serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?,
            );
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown v2 memory action: {other}")),
    }
}

pub fn run(args: MemoryArgs) -> ExitCode {
    let result = (|| -> Result<ExitCode, String> {
        validate_args(&args)?;
        let root = root(&args)?;
        match memory_v2::V2Repository::new(&root)
            .load()
            .map_err(|error| error.to_string())?
            .mode
        {
            memory_v2::RepositoryMode::V2Active => run_v2(&args, &root),
            memory_v2::RepositoryMode::V2Incomplete => {
                Err("MEMORY_PROTOCOL_INCOMPLETE: repair v2 control assets before continuing".into())
            }
            memory_v2::RepositoryMode::Absent if args.action == "propose" => {
                validate_auto_initialize_create(&args, &root)?;
                memory_control::dispatch(&root, "host.memory.v2.initialize", &json!({}))?;
                run_v2(&args, &root)
            }
            memory_v2::RepositoryMode::Absent => Err(
                "MEMORY_PROTOCOL_UNINITIALIZED: initialize Memory Protocol v2 before using the CLI"
                    .into(),
            ),
        }
    })();
    match result {
        Ok(code) => code,
        Err(error) => {
            if args.json {
                let code = error
                    .split_once(':')
                    .map(|(prefix, _)| prefix)
                    .filter(|prefix| prefix.starts_with("MEMORY_"))
                    .unwrap_or("invalid_arguments");
                println!("{}", json!({"ok":false,"error":{"code":code,"message":error}}));
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
    fn purpose_accepts_several_comma_separated_values() {
        let args = parse_args(
            &[
                "propose",
                "create",
                "--purpose",
                "planning, writing ,projection",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>(),
            false,
        );
        assert_eq!(
            required_list(&args, "purpose").unwrap(),
            vec!["planning", "writing", "projection"]
        );
    }

    #[test]
    fn a_required_list_rejects_a_missing_or_empty_flag() {
        let args = parse_args(
            &["propose", "create", "--purpose", " , "]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            false,
        );
        assert!(required_list(&args, "purpose").is_err());
        assert!(required_list(&args, "space").is_err());
    }

    #[test]
    fn parses_v2_proposal_flags() {
        let args = parse_args(
            &[
                "propose",
                "replace",
                "--scope",
                "memory",
                "--target",
                "claim-x",
                "--text",
                "new",
                "--source",
                "a.md",
                "--recorded-by",
                "agent/x",
                "--request-id",
                "memory-inference/x",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>(),
            false,
        );
        assert_eq!(args.action, "propose");
        assert_eq!(args.positionals, vec!["replace"]);
        assert_eq!(
            args.flags.get("target").map(String::as_str),
            Some("claim-x")
        );
        assert_eq!(
            args.flags.get("request-id").map(String::as_str),
            Some("memory-inference/x")
        );
    }

    #[test]
    fn context_boolean_flag_does_not_consume_the_next_flag() {
        let args = parse_args(
            &["context", "--external-transfer", "--space", "work"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            false,
        );
        assert!(args.bools.contains("external-transfer"));
        assert_eq!(args.flags.get("space").map(String::as_str), Some("work"));
    }

    #[test]
    fn validation_rejects_unknown_missing_duplicate_and_conflicting_flags() {
        let unknown = parse_args(
            &["list", "--bogus", "value"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            false,
        );
        assert!(validate_args(&unknown).unwrap_err().contains("--bogus"));

        let missing = parse_args(
            &["context", "--space"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            false,
        );
        assert!(validate_args(&missing)
            .unwrap_err()
            .contains("--space requires a value"));

        let duplicate = parse_args(
            &["list", "--scope", "user", "--scope", "memory"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            false,
        );
        assert!(validate_args(&duplicate)
            .unwrap_err()
            .contains("--scope may only be specified once"));

        let conflicting = parse_args(
            &["list", "--all", "--status", "current"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            false,
        );
        assert_eq!(
            validate_args(&conflicting).unwrap_err(),
            "--all conflicts with --status"
        );
    }

    #[test]
    fn validation_enforces_action_specific_positionals_and_flags() {
        let extra = parse_args(
            &["owner", "extra"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            false,
        );
        assert!(validate_args(&extra).unwrap_err().starts_with("usage:"));

        let human_only = parse_args(
            &["approve", "revision-x"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            false,
        );
        assert!(validate_args(&human_only)
            .unwrap_err()
            .contains("MEMORY_UNAUTHORIZED"));

        let wrong_proposal_flag = parse_args(
            &[
                "propose",
                "revoke",
                "--request-id",
                "r1",
                "--recorded-by",
                "agent",
                "--target",
                "claim",
                "--text",
                "must-not-be-used",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>(),
            false,
        );
        assert!(validate_args(&wrong_proposal_flag)
            .unwrap_err()
            .contains("--text"));
    }

    #[test]
    fn context_provider_and_model_are_only_required_for_external_transfer() {
        let local = parse_args(
            &[
                "context",
                "--space",
                "work",
                "--purpose",
                "planning",
                "--caller",
                "agent:test",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>(),
            false,
        );
        assert!(validate_args(&local).is_ok());

        let external = parse_args(
            &[
                "context",
                "--space",
                "work",
                "--purpose",
                "planning",
                "--caller",
                "agent:test",
                "--external-transfer",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>(),
            false,
        );
        assert_eq!(
            validate_args(&external).unwrap_err(),
            "--provider is required with --external-transfer"
        );
    }

    #[test]
    fn list_filters_scope_and_status_without_mixing_result_kinds() {
        let snapshot = json!({
            "claims": [
                {"claim": {"projection": {"target": "user"}, "lifecycle": {"state": "active"}}, "application_state": "current"},
                {"claim": {"projection": {"target": "memory"}, "lifecycle": {"state": "revoked"}}, "application_state": "no-current"}
            ],
            "pending": [
                {"revision": {"projection": {"target": "user"}}},
                {"revision": {"projection": {"target": "memory"}}}
            ],
            "conflicts": [
                {"heads": [{"projection": {"target": "user"}}]},
                {"heads": [{"projection": {"target": "memory"}}]}
            ],
            "health": {"status": "attention"}
        });
        let current_user = parse_args(
            &["list", "--scope", "user", "--status", "current"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            false,
        );
        let value = list_value(&current_user, &snapshot);
        assert_eq!(value["claims"].as_array().unwrap().len(), 1);
        assert!(value["pending"].as_array().unwrap().is_empty());
        assert!(value["conflicts"].as_array().unwrap().is_empty());

        let pending_memory = parse_args(
            &["list", "--scope", "memory", "--status", "pending"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            false,
        );
        let value = list_value(&pending_memory, &snapshot);
        assert!(value["claims"].as_array().unwrap().is_empty());
        assert_eq!(value["pending"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn check_only_fails_attention_for_projection_drift() {
        assert!(check_passes(&json!({
            "status": "attention",
            "pending_count": 1,
            "projection_edited": false
        })));
        assert!(!check_passes(&json!({
            "status": "attention",
            "pending_count": 0,
            "projection_edited": true
        })));
        assert!(!check_passes(&json!({
            "status": "damaged",
            "projection_edited": false
        })));
    }

    fn initialize_v2(root: &Path) {
        memory_control::dispatch(root, "host.memory.v2.initialize", &json!({})).unwrap();
    }

    fn pending_claim(root: &Path) -> (String, String) {
        let args = parse_args(
            &[
                "propose",
                "create",
                "--request-id",
                "memory-cli-test-proposal",
                "--recorded-by",
                "agent:test",
                "--text",
                "A precise synthetic test preference.",
                "--claim-kind",
                "preference",
                "--category",
                "preferences",
                "--space",
                "tests",
                "--purpose",
                "testing",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>(),
            true,
        );
        validate_args(&args).unwrap();
        v2_propose(&args, root).unwrap();
        let repository = memory_v2::V2Repository::new(root).load().unwrap();
        let claim = &repository.claims[0].value;
        (claim.claim_id.clone(), claim.revision_id.clone())
    }

    #[test]
    fn show_uses_exact_claim_and_revision_ids() {
        let dir = tempfile::TempDir::new().unwrap();
        initialize_v2(dir.path());
        let (claim_id, revision_id) = pending_claim(dir.path());
        let snapshot = v2_snapshot(dir.path()).unwrap();
        assert_eq!(
            show_exact(dir.path(), &snapshot, &revision_id).unwrap()["revision_id"],
            revision_id
        );
        assert!(show_exact(dir.path(), &snapshot, &claim_id).is_ok());
        assert!(show_exact(dir.path(), &snapshot, &claim_id[..8]).is_err());
    }

    #[test]
    fn context_manifest_lookup_is_exact() {
        let dir = tempfile::TempDir::new().unwrap();
        initialize_v2(dir.path());
        let request = json!({
            "space": "tests", "purpose": "testing", "caller": "agent:test",
            "provider": "local", "model": "local", "tools": [],
            "external_transfer": false,
            "as_of_valid_time": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
        });
        let preview =
            memory_control::dispatch(dir.path(), "host.memory.v2.context", &request).unwrap();
        let mut manifest_request = request;
        manifest_request["preview_sha256"] = preview["preview_sha256"].clone();
        let receipt = memory_control::dispatch(
            dir.path(),
            "host.memory.v2.contextManifest",
            &manifest_request,
        )
        .unwrap();
        let id = receipt["manifest_id"].as_str().unwrap();
        assert_eq!(
            load_context_manifest(dir.path(), id).unwrap()["manifest_id"],
            id
        );
        assert!(load_context_manifest(dir.path(), &id[..8]).is_err());
    }

    #[test]
    fn purge_plan_is_local_read_only_and_tracks_authoritative_revision() {
        let dir = tempfile::TempDir::new().unwrap();
        initialize_v2(dir.path());
        let (claim_id, revision_id) = pending_claim(dir.path());
        let revision_path = memory_v2::V2Repository::new(dir.path())
            .load()
            .unwrap()
            .claims[0]
            .path
            .clone();
        let before = fs::read(&revision_path).unwrap();

        let plan = purge_plan(dir.path(), &claim_id).unwrap();
        assert_eq!(plan["claim_id"], claim_id);
        assert_eq!(
            plan["authoritative_assets"]["claim_revisions"][0]["revision_id"],
            revision_id
        );
        assert!(dir
            .path()
            .join(plan["plan_path"].as_str().unwrap())
            .is_file());
        assert_eq!(fs::read(revision_path).unwrap(), before);
    }

    #[test]
    fn auto_initialize_refuses_to_overwrite_existing_projection_content() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(
            dir.path().join("MEMORY.md"),
            "# MEMORY\n\nKeep this text.\n",
        )
        .unwrap();
        let args = parse_args(
            &[
                "propose",
                "create",
                "--request-id",
                "memory-auto-init-existing-projection",
                "--recorded-by",
                "codex/test",
                "--text",
                "Synthetic stable preference.",
                "--claim-kind",
                "preference",
                "--category",
                "context",
                "--space",
                "global",
                "--purpose",
                "writing",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>(),
            true,
        );

        let error = validate_auto_initialize_create(&args, dir.path()).unwrap_err();
        assert!(error.contains("MEMORY_AUTO_INITIALIZE_BLOCKED"), "{error}");
        assert_eq!(
            fs::read_to_string(dir.path().join("MEMORY.md")).unwrap(),
            "# MEMORY\n\nKeep this text.\n"
        );
        assert!(!dir.path().join(".notemd/memory/bootstrap.yaml").exists());
    }
}
