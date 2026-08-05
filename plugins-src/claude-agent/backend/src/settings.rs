//! A template's `.claude/settings.json` stays portable by writing `${VAULT}`
//! instead of a machine path. Before each run the placeholders are substituted
//! into `.claude/settings.local.json` — Claude Code's own local override layer,
//! already gitignored on the vault side.
//!
//! A run aimed at ONE note uses a narrower policy, `settings.scoped.json`, if
//! the template ships one. That is what actually confines the run: telling a
//! model in its prompt to look at a single file does not stop it from grepping
//! the vault, and it did exactly that until the permissions said otherwise.
use crate::mirror::{self, MirrorMeta};
use std::path::{Path, PathBuf};

/// Tools that fetch information rather than reach into the vault. Headless has
/// nobody to answer a prompt, so an un-granted tool is an unusable one; the file
/// scope is held by the deny list, not by keeping the run ignorant.
const INFORMATION_TOOLS: [&str; 4] = ["WebSearch", "WebFetch", "Task", "Skill"];

/// The one note a run is aimed at, and the source document behind it — the
/// protocol needs both, since a question's `line::` points into the source.
#[derive(Debug, Clone)]
pub struct Scope {
    pub note: PathBuf,
    pub source: PathBuf,
    /// The source's directory: the run's real working context. A mirrored
    /// document's neighbours live here, not in the vault.
    pub source_dir: PathBuf,
}

impl Scope {
    /// `X.note.md` sits beside `X.md`. When that `X.md` is a vault mirror whose
    /// original is on this machine, the source is the ORIGINAL — the vault copy
    /// is a snapshot, the original is where the document actually lives. A path
    /// that isn't a sidecar note gets itself as the source, which is harmless:
    /// it only widens Read by one file the run was already pointed at.
    pub fn for_note(vault: &Path, note: &Path, metas: &[MirrorMeta]) -> Scope {
        let s = note.to_string_lossy();
        let beside = s
            .strip_suffix(".note.md")
            .or_else(|| s.strip_suffix(".notes.md"))
            .map(|stem| PathBuf::from(format!("{stem}.md")))
            .unwrap_or_else(|| note.to_path_buf());
        let source = mirror::source_for_mirror(vault, &beside, metas).unwrap_or(beside);
        let source_dir = source
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| source.clone());
        Scope {
            note: note.to_path_buf(),
            source,
            source_dir,
        }
    }
}

/// The MCP servers configured on this machine, by name. Three places, because
/// that is where `claude mcp add` puts them: user scope in `~/.claude.json`,
/// local scope under that file's `projects.<dir>`, and project scope in a
/// `.mcp.json` beside the task. Sorted and deduped.
///
/// Which servers exist is a property of the machine, not of the task, so the
/// templates can't name them: a rule for a server that isn't installed is dead
/// weight, and a server the user did install would otherwise stay unusable.
pub fn mcp_server_names(home: &Path, dirs: &[PathBuf]) -> Vec<String> {
    let read = |p: PathBuf| {
        std::fs::read_to_string(p)
            .ok()
            .and_then(|b| serde_json::from_str::<serde_json::Value>(&b).ok())
    };
    let user = read(home.join(".claude.json"));
    let local = dirs.iter().filter_map(|d| {
        let projects = user.as_ref()?.get("projects")?;
        projects.get(d.to_string_lossy().as_ref()).cloned()
    });
    let mut names: Vec<String> = user
        .clone()
        .into_iter()
        .chain(dirs.iter().filter_map(|d| read(d.join(".mcp.json"))))
        .chain(local)
        .filter_map(|v| v.get("mcpServers")?.as_object().cloned())
        .flat_map(|o| o.keys().cloned().collect::<Vec<_>>())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Generate `.claude/settings.local.json`. A task without any settings template
/// is fine — silently skipped.
///
/// `servers` is what this machine has — passed in rather than read from the
/// environment, so a test never has to stand up a home directory.
pub fn materialize(
    task_dir: &Path,
    vault: &Path,
    scope: Option<&Scope>,
    metas: &[MirrorMeta],
    servers: &[String],
) -> std::io::Result<()> {
    let scoped = task_dir.join(".claude/settings.scoped.json");
    let src = match scope {
        Some(_) if scoped.is_file() => scoped,
        _ => task_dir.join(".claude/settings.json"),
    };
    let Ok(body) = std::fs::read_to_string(&src) else {
        return Ok(());
    };
    // Read access to the originals' directories is appended in code, not left to
    // the template: `seed_builtin_templates` never overwrites, so a vault seeded
    // by an older build would otherwise keep the agent stuck on the mirror
    // forever. Read only — a source dir never becomes writable.
    let dirs = match scope {
        Some(s) => vec![s.source_dir.clone()],
        None => mirror::local_source_dirs(metas),
    };
    // Headless has nobody to answer a permission prompt, so a tool that isn't
    // pre-approved here is a tool this run simply cannot use. MCP rules take no
    // wildcard — `mcp__server__*` matches nothing — so each server is named.
    let mut rules: Vec<String> = dirs
        .iter()
        .map(|d| format!("Read({}/**)", d.to_string_lossy()))
        .collect();
    // Granted in code for the same reason the source dirs are: a vault seeded by
    // an older build keeps its template forever, and `seed_builtin_templates`
    // will not overwrite it.
    rules.extend(INFORMATION_TOOLS.iter().map(|t| t.to_string()));
    rules.extend(servers.iter().map(|s| format!("mcp__{s}")));
    let out = append_allow(&substitute(&body, vault, scope), &rules);
    let dst = task_dir.join(".claude/settings.local.json");
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(dst, out)
}

pub fn substitute(body: &str, vault: &Path, scope: Option<&Scope>) -> String {
    let mut out = body.replace("${VAULT}", &vault.to_string_lossy());
    if let Some(s) = scope {
        out = out
            .replace("${NOTE}", &s.note.to_string_lossy())
            .replace("${SOURCE_DIR}", &s.source_dir.to_string_lossy())
            .replace("${SOURCE}", &s.source.to_string_lossy());
    }
    out
}

/// Append rules to `permissions.allow`. Returns the body unchanged when there is
/// nothing to add or the body isn't the object we expect — a settings file we
/// can't parse is left exactly as the template wrote it.
fn append_allow(body: &str, rules: &[String]) -> String {
    if rules.is_empty() {
        return body.to_string();
    }
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.to_string();
    };
    let Some(allow) = v
        .get_mut("permissions")
        .and_then(|p| p.get_mut("allow"))
        .and_then(|a| a.as_array_mut())
    else {
        return body.to_string();
    };
    for r in rules {
        let rule = serde_json::Value::String(r.clone());
        if !allow.contains(&rule) {
            allow.push(rule);
        }
    }
    serde_json::to_string_pretty(&v).unwrap_or_else(|_| body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Scope {
        Scope::for_note(Path::new("/v"), Path::new("/v/docs/a.note.md"), &[])
    }

    fn meta(mirror: &str, source: &str) -> MirrorMeta {
        serde_json::from_value(serde_json::json!({
            "mirror": mirror, "deviceId": "d", "deviceName": "n",
            "source": source, "syncedAt": 1, "checksum": "sha256:x",
        }))
        .unwrap()
    }

    fn write_templates(dir: &Path, broad: &str, narrow: Option<&str>) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join(".claude/settings.json"), broad).unwrap();
        if let Some(n) = narrow {
            std::fs::write(dir.join(".claude/settings.scoped.json"), n).unwrap();
        }
    }

    fn local(dir: &Path) -> String {
        std::fs::read_to_string(dir.join(".claude/settings.local.json")).unwrap()
    }

    #[test]
    fn a_sidecar_note_resolves_to_its_source_document() {
        assert_eq!(scope().source, PathBuf::from("/v/docs/a.md"));
        assert_eq!(scope().source_dir, PathBuf::from("/v/docs"));
        assert_eq!(
            Scope::for_note(Path::new("/v"), Path::new("/v/b.notes.md"), &[]).source,
            PathBuf::from("/v/b.md")
        );
        // Not a sidecar name: no second file is opened up.
        let plain = Scope::for_note(Path::new("/v"), Path::new("/v/c.md"), &[]);
        assert_eq!(plain.source, plain.note);
    }

    #[test]
    fn a_mirrors_note_resolves_to_the_original_outside_the_vault() {
        let v = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        let orig = proj.path().join("DESIGN.CN.md");
        std::fs::write(&orig, "# doc").unwrap();
        let metas = vec![meta(
            "Sync/2026-08-04-DESIGN.CN.md",
            &orig.to_string_lossy(),
        )];
        let note = v.path().join("Sync/2026-08-04-DESIGN.CN.note.md");
        let s = Scope::for_note(v.path(), &note, &metas);
        assert_eq!(s.source, orig);
        assert_eq!(s.source_dir, proj.path());
    }

    #[test]
    fn an_unresolvable_mirror_falls_back_to_the_vault_copy() {
        let v = tempfile::tempdir().unwrap();
        // Recorded on another machine: that path isn't on this disk.
        let metas = vec![meta("Sync/a.md", "/Users/someone-else/a.md")];
        let note = v.path().join("Sync/a.note.md");
        let s = Scope::for_note(v.path(), &note, &metas);
        assert_eq!(s.source, v.path().join("Sync/a.md"));
        assert_eq!(s.source_dir, v.path().join("Sync"));
    }

    #[test]
    fn replaces_every_placeholder() {
        let got = substitute(
            r#"["Read(${VAULT}/**)","Read(${NOTE})","Read(${SOURCE})","Read(${SOURCE_DIR}/**)"]"#,
            Path::new("/v"),
            Some(&scope()),
        );
        assert_eq!(
            got,
            r#"["Read(/v/**)","Read(/v/docs/a.note.md)","Read(/v/docs/a.md)","Read(/v/docs/**)"]"#
        );
    }

    #[test]
    fn a_sweep_may_read_every_local_original_but_write_none_of_them() {
        let d = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        std::fs::write(proj.path().join("a.md"), "x").unwrap();
        let metas = vec![
            meta("Sync/a.md", &proj.path().join("a.md").to_string_lossy()),
            meta("Sync/b.md", "/Users/someone-else/b.md"), // another device
        ];
        write_templates(
            d.path(),
            r#"{"permissions":{"allow":["Read(${VAULT}/**)"]}}"#,
            None,
        );
        materialize(d.path(), Path::new("/v"), None, &metas, &[]).unwrap();
        let got = local(d.path());
        assert!(
            got.contains(&format!("Read({}/**)", proj.path().to_string_lossy())),
            "the local original's directory must be readable: {got}"
        );
        assert!(
            !got.contains("someone-else"),
            "a foreign path must not leak in: {got}"
        );
        assert!(
            !got.contains(&format!("Write({}", proj.path().to_string_lossy())),
            "a sweep must never gain write access to a source dir: {got}"
        );
    }

    #[test]
    fn a_scoped_run_may_read_the_originals_directory_without_the_template_saying_so() {
        // A vault seeded by an older build still has the old settings.scoped.json;
        // the source dir must be granted in code regardless.
        let d = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        let orig = proj.path().join("a.md");
        std::fs::write(&orig, "x").unwrap();
        let v = tempfile::tempdir().unwrap();
        let metas = vec![meta("Sync/a.md", &orig.to_string_lossy())];
        let sc = Scope::for_note(v.path(), &v.path().join("Sync/a.note.md"), &metas);
        write_templates(
            d.path(),
            r#"{"permissions":{"allow":["Read(${VAULT}/**)"]}}"#,
            Some(r#"{"permissions":{"allow":["Read(${NOTE})","Read(${SOURCE})"]}}"#),
        );
        materialize(d.path(), v.path(), Some(&sc), &metas, &[]).unwrap();
        let got = local(d.path());
        assert!(got.contains(&orig.to_string_lossy().to_string()), "{got}");
        assert!(
            got.contains(&format!("Read({}/**)", proj.path().to_string_lossy())),
            "{got}"
        );
    }

    #[test]
    fn a_sweep_with_no_local_originals_leaves_the_policy_alone() {
        let d = tempfile::tempdir().unwrap();
        let body = r#"{"permissions":{"allow":["Read(${VAULT}/**)"]}}"#;
        write_templates(d.path(), body, None);
        materialize(d.path(), Path::new("/v"), None, &[], &[]).unwrap();
        let got = local(d.path());
        assert!(got.contains("Read(/v/**)"), "{got}");
        assert_eq!(
            got.matches("Read(").count(),
            1,
            "no extra directory opened up: {got}"
        );
    }

    #[test]
    fn a_note_run_gets_the_narrow_policy() {
        let d = tempfile::tempdir().unwrap();
        write_templates(
            d.path(),
            r#"{"allow":["Read(${VAULT}/**)"]}"#,
            Some(r#"{"allow":["Read(${NOTE})"],"deny":["Grep"]}"#),
        );
        materialize(d.path(), Path::new("/v"), Some(&scope()), &[], &[]).unwrap();
        let got = local(d.path());
        assert!(got.contains("Read(/v/docs/a.note.md)"), "got {got}");
        assert!(
            got.contains("Grep"),
            "the deny list must come through: {got}"
        );
        assert!(
            !got.contains("Read(/v/**)"),
            "must not fall back to vault-wide"
        );
    }

    #[test]
    fn a_whole_vault_run_keeps_the_broad_policy() {
        let d = tempfile::tempdir().unwrap();
        write_templates(
            d.path(),
            r#"{"allow":["Read(${VAULT}/**)"]}"#,
            Some(r#"{"allow":["Read(${NOTE})"]}"#),
        );
        materialize(d.path(), Path::new("/v"), None, &[], &[]).unwrap();
        assert!(local(d.path()).contains("Read(/v/**)"));
    }

    #[test]
    fn a_note_run_falls_back_when_the_template_has_no_narrow_policy() {
        let d = tempfile::tempdir().unwrap();
        write_templates(d.path(), r#"{"allow":["Read(${VAULT}/**)"]}"#, None);
        materialize(d.path(), Path::new("/v"), Some(&scope()), &[], &[]).unwrap();
        assert!(local(d.path()).contains("Read(/v/**)"));
    }

    #[test]
    fn the_portable_templates_are_never_rewritten() {
        let d = tempfile::tempdir().unwrap();
        write_templates(
            d.path(),
            r#"{"allow":["Read(${VAULT}/**)"]}"#,
            Some(r#"{"allow":["Read(${NOTE})"]}"#),
        );
        materialize(d.path(), Path::new("/v"), Some(&scope()), &[], &[]).unwrap();
        for f in [".claude/settings.json", ".claude/settings.scoped.json"] {
            let t = std::fs::read_to_string(d.path().join(f)).unwrap();
            assert!(t.contains("${"), "{f} lost its placeholders");
        }
    }

    #[test]
    fn is_a_no_op_when_the_task_has_no_settings_template() {
        let d = tempfile::tempdir().unwrap();
        materialize(d.path(), Path::new("/v"), None, &[], &[]).unwrap();
        assert!(!d.path().join(".claude/settings.local.json").exists());
    }

    #[test]
    fn the_information_tools_are_pre_approved_even_for_a_vault_seeded_long_ago() {
        // Same reason the source dir is granted in code: an old vault keeps its
        // old template forever, so the grant cannot live in the template.
        let d = tempfile::tempdir().unwrap();
        write_templates(
            d.path(),
            r#"{"permissions":{"allow":["Read(${VAULT}/**)"]}}"#,
            None,
        );
        materialize(d.path(), Path::new("/v"), None, &[], &[]).unwrap();
        let got = local(d.path());
        for tool in ["WebSearch", "WebFetch", "Task", "Skill"] {
            assert!(
                got.contains(&format!(r#""{tool}""#)),
                "{tool} missing from {got}"
            );
        }
    }

    #[test]
    fn every_configured_mcp_server_is_pre_approved() {
        // Headless has nobody to answer a permission prompt: a tool that isn't
        // pre-approved is a tool the run cannot use.
        let d = tempfile::tempdir().unwrap();
        write_templates(
            d.path(),
            r#"{"permissions":{"allow":["Read(${VAULT}/**)"]}}"#,
            None,
        );
        materialize(
            d.path(),
            Path::new("/v"),
            None,
            &[],
            &["exa".to_string(), "hemory-vault".to_string()],
        )
        .unwrap();
        let got = local(d.path());
        assert!(got.contains(r#""mcp__exa""#), "{got}");
        assert!(got.contains(r#""mcp__hemory-vault""#), "{got}");
    }

    #[test]
    fn a_machine_with_no_mcp_servers_gets_no_stray_rules() {
        let d = tempfile::tempdir().unwrap();
        let body = r#"{"permissions":{"allow":["Read(${VAULT}/**)"]}}"#;
        write_templates(d.path(), body, None);
        materialize(d.path(), Path::new("/v"), None, &[], &[]).unwrap();
        assert!(!local(d.path()).contains("mcp__"), "{}", local(d.path()));
    }

    #[test]
    fn a_server_the_template_already_named_is_not_listed_twice() {
        let d = tempfile::tempdir().unwrap();
        write_templates(d.path(), r#"{"permissions":{"allow":["mcp__exa"]}}"#, None);
        materialize(d.path(), Path::new("/v"), None, &[], &["exa".to_string()]).unwrap();
        assert_eq!(
            local(d.path()).matches("mcp__exa").count(),
            1,
            "{}",
            local(d.path())
        );
    }

    #[test]
    fn mcp_servers_are_collected_from_the_user_config_and_the_project_files() {
        let home = tempfile::tempdir().unwrap();
        let task = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join(".claude.json"),
            r#"{"mcpServers":{"exa":{"command":"npx"}},"projects":{}}"#,
        )
        .unwrap();
        std::fs::write(
            task.path().join(".mcp.json"),
            r#"{"mcpServers":{"exa":{},"local-thing":{}}}"#,
        )
        .unwrap();
        let got = mcp_server_names(home.path(), &[task.path().to_path_buf()]);
        assert_eq!(got, vec!["exa".to_string(), "local-thing".to_string()]);
    }

    #[test]
    fn a_server_added_locally_to_the_task_directory_counts_too() {
        // `claude mcp add` without `-s user` files the server under the cwd's
        // project entry, which for these runs is the task directory.
        let home = tempfile::tempdir().unwrap();
        let task = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join(".claude.json"),
            serde_json::json!({
                "projects": {
                    task.path().to_string_lossy(): {"mcpServers": {"local-only": {}}},
                    "/somewhere/else": {"mcpServers": {"not-ours": {}}}
                }
            })
            .to_string(),
        )
        .unwrap();
        let got = mcp_server_names(home.path(), &[task.path().to_path_buf()]);
        assert_eq!(got, vec!["local-only".to_string()]);
    }

    #[test]
    fn an_absent_or_broken_config_yields_no_servers() {
        let home = tempfile::tempdir().unwrap();
        assert!(mcp_server_names(home.path(), &[]).is_empty());
        std::fs::write(home.path().join(".claude.json"), "{ not json").unwrap();
        assert!(mcp_server_names(home.path(), &[]).is_empty());
    }

    #[test]
    fn rewrites_a_stale_local_override() {
        let d = tempfile::tempdir().unwrap();
        write_templates(d.path(), r#"{"allow":["Read(${VAULT}/**)"]}"#, None);
        std::fs::write(d.path().join(".claude/settings.local.json"), "OLD").unwrap();
        materialize(d.path(), Path::new("/new/vault"), None, &[], &[]).unwrap();
        let got = local(d.path());
        assert!(got.contains("/new/vault"));
        assert!(!got.contains("OLD"));
    }
}
