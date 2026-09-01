use super::canonical::{canonical_yaml, raw_sha256, CanonicalPayload};
use super::model::*;
use fs2::FileExt;
use serde::Serialize;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

const RAW_PREFIX: usize = 16;

#[derive(Debug, Clone)]
pub struct Published<T> {
    pub path: PathBuf,
    pub raw_sha256: String,
    pub value: T,
}

#[derive(Debug)]
pub struct WriterError {
    pub code: &'static str,
    pub message: String,
}

impl WriterError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for WriterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WriterError {}

struct WriteLock(File);

impl Drop for WriteLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

pub struct RepositoryWriter {
    root: PathBuf,
}

impl RepositoryWriter {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn initialize(
        &self,
        vault_id: String,
        protocol: ProtocolRevision,
        authority: AuthorityRevision,
    ) -> Result<Bootstrap, WriterError> {
        let _guard = self.lock()?;
        create_new_or_same(
            &self.tmp_dir(),
            &self.root.join(".notemd/memory/.gitignore"),
            b".local/\n",
        )?;
        let protocol = self.publish_protocol_unlocked(protocol)?;
        let authority = self.publish_authority_unlocked(authority)?;
        let bootstrap = Bootstrap {
            schema: "notemd.memory/bootstrap/v2".into(),
            vault_id,
            protocol_family: "notemd.memory".into(),
            initial_protocol_revision: RevisionRef {
                revision_id: protocol.value.revision_id.clone(),
                payload_sha256: protocol.value.payload_sha256.clone(),
            },
            initial_authority_revision: RevisionRef {
                revision_id: authority.value.revision_id.clone(),
                payload_sha256: authority.value.payload_sha256.clone(),
            },
        };
        let raw = serde_yaml::to_string(&bootstrap)
            .map_err(|error| WriterError::new("MEMORY_INVALID_PAYLOAD", error.to_string()))?;
        create_new_or_same(
            &self.tmp_dir(),
            &self.root.join(".notemd/memory/bootstrap.yaml"),
            raw.as_bytes(),
        )?;
        Ok(bootstrap)
    }

    pub fn publish_protocol(
        &self,
        value: ProtocolRevision,
    ) -> Result<Published<ProtocolRevision>, WriterError> {
        let _guard = self.lock()?;
        self.publish_protocol_unlocked(value)
    }

    pub fn publish_authority(
        &self,
        value: AuthorityRevision,
    ) -> Result<Published<AuthorityRevision>, WriterError> {
        let _guard = self.lock()?;
        self.publish_authority_unlocked(value)
    }

    pub fn publish_claim(
        &self,
        value: MemoryClaimRevision,
    ) -> Result<Published<MemoryClaimRevision>, WriterError> {
        let _guard = self.lock()?;
        let shard = value.claim_id.get(..2).unwrap_or("__");
        let dir = self
            .root
            .join(".notemd/memory/claims")
            .join(shard)
            .join(&value.claim_id)
            .join("revisions");
        publish(&self.tmp_dir(), &dir, &value.revision_id.clone(), value)
    }

    pub fn publish_operation(
        &self,
        value: MemoryOperation,
    ) -> Result<Published<MemoryOperation>, WriterError> {
        let _guard = self.lock()?;
        publish(
            &self.tmp_dir(),
            &self.root.join(".notemd/memory/operations"),
            &value.operation_id.clone(),
            value,
        )
    }

    pub fn publish_context_manifest(
        &self,
        value: ContextManifest,
    ) -> Result<Published<ContextManifest>, WriterError> {
        let _guard = self.lock()?;
        let month = value
            .request
            .as_of_valid_time
            .get(..7)
            .filter(|value| value.len() == 7 && value.as_bytes().get(4) == Some(&b'-'))
            .unwrap_or("unknown");
        publish(
            &self.tmp_dir(),
            &self
                .root
                .join(".notemd/memory/context-manifests")
                .join(month),
            &value.manifest_id.clone(),
            value,
        )
    }

    fn publish_protocol_unlocked(
        &self,
        value: ProtocolRevision,
    ) -> Result<Published<ProtocolRevision>, WriterError> {
        publish(
            &self.tmp_dir(),
            &self.root.join(".notemd/memory/protocol-revisions"),
            &value.revision_id.clone(),
            value,
        )
    }

    fn publish_authority_unlocked(
        &self,
        value: AuthorityRevision,
    ) -> Result<Published<AuthorityRevision>, WriterError> {
        publish(
            &self.tmp_dir(),
            &self.root.join(".notemd/memory/authority-revisions"),
            &value.revision_id.clone(),
            value,
        )
    }

    fn tmp_dir(&self) -> PathBuf {
        self.root.join(".notemd/memory/.local/tmp")
    }

    fn lock(&self) -> Result<WriteLock, WriterError> {
        let path = git_common_dir(&self.root)
            .map(|dir| dir.join("notemd-memory-v2.lock"))
            .unwrap_or_else(|| self.root.join(".notemd/memory/.local/control.lock"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| WriterError::new("MEMORY_IO", error.to_string()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| WriterError::new("MEMORY_IO", error.to_string()))?;
        file.lock_exclusive()
            .map_err(|error| WriterError::new("MEMORY_IO", error.to_string()))?;
        Ok(WriteLock(file))
    }
}

fn git_common_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let marker = fs::read_to_string(&dot_git).ok()?;
        let value = marker.trim().strip_prefix("gitdir:")?.trim();
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            root.join(path)
        }
    };
    let common = git_dir.join("commondir");
    if common.is_file() {
        let value = fs::read_to_string(common).ok()?;
        let path = PathBuf::from(value.trim());
        let resolved = if path.is_absolute() {
            path
        } else {
            git_dir.join(path)
        };
        Some(fs::canonicalize(&resolved).unwrap_or(resolved))
    } else {
        Some(fs::canonicalize(&git_dir).unwrap_or(git_dir))
    }
}

fn publish<T: CanonicalPayload + Serialize>(
    tmp_dir: &Path,
    dir: &Path,
    id: &str,
    value: T,
) -> Result<Published<T>, WriterError> {
    let (value, raw) = canonical_yaml(&value)
        .map_err(|error| WriterError::new("MEMORY_INVALID_PAYLOAD", error))?;
    let raw_hash = raw_sha256(&raw);
    let path = dir.join(format!("{id}.{}.yaml", &raw_hash[..RAW_PREFIX]));
    if dir.exists() {
        let prefix = format!("{id}.");
        for entry in
            fs::read_dir(dir).map_err(|error| WriterError::new("MEMORY_IO", error.to_string()))?
        {
            let existing = entry
                .map_err(|error| WriterError::new("MEMORY_IO", error.to_string()))?
                .path();
            if existing
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".yaml"))
                && existing != path
            {
                return Err(WriterError::new(
                    "MEMORY_TAMPERED_ASSET",
                    format!("logical object {id} already exists with different bytes"),
                ));
            }
        }
    }
    create_new_or_same(tmp_dir, &path, &raw)?;
    Ok(Published {
        path,
        raw_sha256: raw_hash,
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn protocol() -> ProtocolRevision {
        ProtocolRevision {
            schema: "notemd.memory/protocol-revision/v2".into(),
            revision_id: "01900000-0000-7000-8000-000000000001".into(),
            base_heads: vec![],
            causal_context: CausalContext::default(),
            protocol_major: 2,
            protocol_minor: 0,
            renderer_version: "notemd.memory.projector/2".into(),
            claim_schema: "notemd.memory/claim-revision/v2".into(),
            category_registry: BTreeMap::from([
                ("user".into(), vec!["preferences".into()]),
                ("memory".into(), vec!["context".into()]),
            ]),
            decision: ControlDecision {
                verdict: Verdict::Approve,
                actor_id: "human:bruce".into(),
                authority_context: AuthorityContext {
                    heads: vec![],
                    capability: "bootstrap".into(),
                },
            },
            transition: ControlTransition {
                operation: ControlOperation::Initialize,
            },
            payload_sha256: String::new(),
        }
    }

    fn authority() -> AuthorityRevision {
        AuthorityRevision {
            schema: "notemd.memory/authority-revision/v2".into(),
            revision_id: "01900000-0000-7000-8000-000000000002".into(),
            base_heads: vec![],
            causal_context: CausalContext::default(),
            owner: AuthorityOwner {
                owner_id: "owner:bruce".into(),
                actor_id: "human:bruce".into(),
            },
            principals: vec![Principal {
                actor_id: "human:bruce".into(),
                capabilities: vec!["memory.claim.approve".into()],
            }],
            recovery: Recovery::LocalOwnerSetup,
            decision: ControlDecision {
                verdict: Verdict::Approve,
                actor_id: "human:bruce".into(),
                authority_context: AuthorityContext {
                    heads: vec![],
                    capability: "bootstrap".into(),
                },
            },
            transition: ControlTransition {
                operation: ControlOperation::Initialize,
            },
            payload_sha256: String::new(),
        }
    }

    #[test]
    fn bootstrap_is_activation_last_and_loads_as_v2_active() {
        let dir = tempfile::TempDir::new().unwrap();
        let writer = RepositoryWriter::new(dir.path());
        let bootstrap = writer
            .initialize("vault:test".into(), protocol(), authority())
            .unwrap();
        let snapshot = super::super::repository::V2Repository::new(dir.path())
            .load()
            .unwrap();
        assert_eq!(snapshot.mode, RepositoryMode::V2Active);
        assert_eq!(snapshot.bootstrap.unwrap(), bootstrap);
        assert_eq!(snapshot.protocols.len(), 1);
        assert_eq!(snapshot.authorities.len(), 1);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".notemd/memory/.gitignore")).unwrap(),
            ".local/\n"
        );
    }

    #[test]
    fn identical_publish_is_idempotent_but_same_id_different_payload_is_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let writer = RepositoryWriter::new(dir.path());
        let first = writer.publish_protocol(protocol()).unwrap();
        let repeated = writer.publish_protocol(protocol()).unwrap();
        assert_eq!(first.path, repeated.path);
        let mut changed = protocol();
        changed.renderer_version = "different".into();
        let error = writer.publish_protocol(changed).unwrap_err();
        assert_eq!(error.code, "MEMORY_TAMPERED_ASSET");
    }

    #[test]
    fn temp_and_lock_assets_remain_under_local() {
        let dir = tempfile::TempDir::new().unwrap();
        let writer = RepositoryWriter::new(dir.path());
        writer.publish_protocol(protocol()).unwrap();
        assert!(dir
            .path()
            .join(".notemd/memory/.local/control.lock")
            .exists());
        assert!(dir.path().join(".notemd/memory/.local/tmp").exists());
        let tmp_files = fs::read_dir(dir.path().join(".notemd/memory/.local/tmp"))
            .unwrap()
            .count();
        assert_eq!(tmp_files, 0);
    }

    #[test]
    fn linked_worktrees_resolve_one_common_repository_lock() {
        let dir = tempfile::TempDir::new().unwrap();
        let worktree = dir.path().join("worktree");
        let git_dir = dir.path().join("repo.git/worktrees/w1");
        let common = dir.path().join("repo.git");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .unwrap();
        std::fs::write(git_dir.join("commondir"), "../..\n").unwrap();

        let common = std::fs::canonicalize(common).unwrap();
        assert_eq!(git_common_dir(&worktree).unwrap(), common);
        assert_eq!(
            git_common_dir(&worktree)
                .unwrap()
                .join("notemd-memory-v2.lock"),
            common.join("notemd-memory-v2.lock")
        );
    }
}

fn create_new_or_same(tmp_dir: &Path, target: &Path, bytes: &[u8]) -> Result<(), WriterError> {
    if target.exists() {
        let existing =
            fs::read(target).map_err(|error| WriterError::new("MEMORY_IO", error.to_string()))?;
        return if existing == bytes {
            Ok(())
        } else {
            Err(WriterError::new(
                "MEMORY_TAMPERED_ASSET",
                format!("no-clobber target differs: {}", target.display()),
            ))
        };
    }
    let parent = target
        .parent()
        .ok_or_else(|| WriterError::new("MEMORY_IO", "target has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| WriterError::new("MEMORY_IO", error.to_string()))?;
    fs::create_dir_all(tmp_dir)
        .map_err(|error| WriterError::new("MEMORY_IO", error.to_string()))?;
    let mut temp = NamedTempFile::new_in(tmp_dir)
        .map_err(|error| WriterError::new("MEMORY_IO", error.to_string()))?;
    temp.write_all(bytes)
        .map_err(|error| WriterError::new("MEMORY_IO", error.to_string()))?;
    temp.as_file()
        .sync_all()
        .map_err(|error| WriterError::new("MEMORY_IO", error.to_string()))?;
    temp.persist_noclobber(target)
        .map_err(|error| WriterError::new("MEMORY_TAMPERED_ASSET", error.error.to_string()))?;
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}
