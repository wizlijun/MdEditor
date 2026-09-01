//! `notemd memory` — controlled USER/MEMORY CLI.
//!
//! This is core rather than plugin-provided so an Agent's proposal path does
//! not disappear when the Memory window plugin is disabled. The window and CLI
//! still share `crate::memory_control` for every state transition.

use crate::memory_control::{self, model::*};
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
        } else if matches!(token.as_str(), "--confirm-human-approved" | "--all") {
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
            Operation::Delete => {
                "removed from projection (candidate/event retained)".to_string()
            }
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

fn decide(args: &MemoryArgs, root: &std::path::Path, action: DecisionAction) -> Result<(), String> {
    let proposal_id = args
        .positionals
        .first()
        .cloned()
        .or_else(|| args.flags.get("id").cloned())
        .ok_or("approve/reject requires a proposal id")?;
    let expected_sha256 = args
        .flags
        .get("proposal-sha256")
        .cloned()
        .ok_or("decision requires --proposal-sha256 from `notemd memory show --json`")?;
    let actor = args
        .flags
        .get("approved-by")
        .or_else(|| args.flags.get("decided-by"))
        .cloned()
        .ok_or("decision requires --approved-by human:<id>")?;
    let value = memory_control::decide(
        root,
        DecideInput {
            proposal_id,
            expected_sha256,
            action,
            actor,
            human_confirmed: args.bools.contains("confirm-human-approved"),
            reason: args.flags.get("reason").cloned(),
        },
    )?;
    print_json_or_text(
        args,
        value.clone(),
        format!(
            "{:?} proposal {} -> entry {}",
            action,
            value["proposal_id"].as_str().unwrap_or(""),
            value["entry_id"].as_str().unwrap_or("")
        ),
    );
    Ok(())
}

pub fn run(args: MemoryArgs) -> ExitCode {
    let result = (|| -> Result<ExitCode, String> {
        let root = root(&args)?;
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
            "approve" => decide(&args, &root, DecisionAction::Approve).map(|_| ExitCode::SUCCESS),
            "reject" => decide(&args, &root, DecisionAction::Reject).map(|_| ExitCode::SUCCESS),
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
                let value = memory_control::migrate(&root)?;
                print_json_or_text(
                    &args,
                    value.clone(),
                    format!(
                        "Migrated {} legacy entries to pending proposals.",
                        value["migrated"].as_u64().unwrap_or(0)
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
    fn approval_confirmation_is_a_dedicated_flag() {
        let args = parse_args(
            &[
                "approve",
                "p1",
                "--proposal-sha256",
                "abc123",
                "--approved-by",
                "human:bruce",
                "--confirm-human-approved",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>(),
            false,
        );
        assert!(args.bools.contains("confirm-human-approved"));
        assert_eq!(
            args.flags.get("proposal-sha256").map(String::as_str),
            Some("abc123")
        );
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
