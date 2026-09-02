//! `notemd memory` — controlled USER/MEMORY CLI.
//!
//! This is core rather than plugin-provided so an Agent's proposal path does
//! not disappear when the Memory window plugin is disabled. The window and CLI
//! still share `crate::memory_control` for every state transition.

use crate::memory_control::{self, v2 as memory_v2};
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::fs;
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
        } else if matches!(token.as_str(), "--all" | "--external-transfer") {
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
    args.flags
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("--{name} is required"))
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
        other => Err(format!("unknown v2 memory action: {other}")),
    }
}

pub fn run(args: MemoryArgs) -> ExitCode {
    let result = (|| -> Result<ExitCode, String> {
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
    fn purpose_accepts_several_comma_separated_values() {
        let args = parse_args(
            &["propose", "create", "--purpose", "planning, writing ,projection"]
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
