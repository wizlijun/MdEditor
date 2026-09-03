use super::canonical::{payload_sha256, raw_sha256, CanonicalPayload};
use super::model::*;
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const MEMORY_DIR: &str = ".notemd/memory";
const MAX_OBJECT_BYTES: u64 = 1024 * 1024;
const MIN_RAW_HASH_PREFIX: usize = 12;

#[derive(Debug, Clone)]
pub struct Loaded<T> {
    pub path: PathBuf,
    pub raw_sha256: String,
    pub value: T,
}

#[derive(Debug, Clone)]
pub struct RepositorySnapshot {
    pub mode: RepositoryMode,
    pub bootstrap: Option<Bootstrap>,
    pub protocols: Vec<Loaded<ProtocolRevision>>,
    pub authorities: Vec<Loaded<AuthorityRevision>>,
    pub context_registries: Vec<Loaded<ContextRegistryRevision>>,
    pub claims: Vec<Loaded<MemoryClaimRevision>>,
    pub operations: Vec<Loaded<MemoryOperation>>,
    pub context_manifests: Vec<Loaded<ContextManifest>>,
    pub diagnostics: Vec<String>,
}

impl RepositorySnapshot {
    pub fn is_v2_active(&self) -> bool {
        self.mode == RepositoryMode::V2Active
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryError {
    pub code: &'static str,
    pub path: Option<PathBuf>,
    pub message: String,
}

impl RepositoryError {
    fn new(
        code: &'static str,
        path: impl Into<Option<PathBuf>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(
                formatter,
                "{}: {}: {}",
                self.code,
                path.display(),
                self.message
            )
        } else {
            write!(formatter, "{}: {}", self.code, self.message)
        }
    }
}

impl std::error::Error for RepositoryError {}

pub struct V2Repository {
    root: PathBuf,
}

impl V2Repository {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn load(&self) -> Result<RepositorySnapshot, RepositoryError> {
        let memory = self.root.join(MEMORY_DIR);
        let bootstrap_path = memory.join("bootstrap.yaml");
        let has_v2_assets = [
            "protocol-revisions",
            "authority-revisions",
            "context-registry-revisions",
            "claims",
            "operations",
        ]
        .iter()
        .any(|name| memory.join(name).exists());
        if !bootstrap_path.exists() {
            return Ok(RepositorySnapshot {
                mode: if has_v2_assets {
                    RepositoryMode::V2Incomplete
                } else {
                    RepositoryMode::Absent
                },
                bootstrap: None,
                protocols: Vec::new(),
                authorities: Vec::new(),
                context_registries: Vec::new(),
                claims: Vec::new(),
                operations: Vec::new(),
                context_manifests: Vec::new(),
                diagnostics: has_v2_assets
                    .then(|| "v2 assets exist without bootstrap activation".to_string())
                    .into_iter()
                    .collect(),
            });
        }

        let bootstrap = read_plain::<Bootstrap>(&bootstrap_path)?;
        if bootstrap.schema != "notemd.memory/bootstrap/v2"
            || bootstrap.protocol_family != "notemd.memory"
        {
            return Err(RepositoryError::new(
                "MEMORY_PROTOCOL_UNSUPPORTED",
                Some(bootstrap_path),
                "unsupported bootstrap schema or protocol family",
            ));
        }

        let protocols = self
            .load_revision_dir::<ProtocolRevision>(&memory.join("protocol-revisions"), |value| {
                &value.revision_id
            })?;
        let authorities = self.load_revision_dir::<AuthorityRevision>(
            &memory.join("authority-revisions"),
            |value| &value.revision_id,
        )?;
        let context_registries = self.load_revision_dir::<ContextRegistryRevision>(
            &memory.join("context-registry-revisions"),
            |value| &value.revision_id,
        )?;
        let claims = self.load_claims(&memory.join("claims"))?;
        let operations = self
            .load_revision_dir::<MemoryOperation>(&memory.join("operations"), |value| {
                &value.operation_id
            })?;
        let context_manifests = self
            .load_revision_dir::<ContextManifest>(&memory.join("context-manifests"), |value| {
                &value.manifest_id
            })?;
        ensure_unique(
            "protocol revision",
            protocols.iter().map(|item| &item.value.revision_id),
        )?;
        ensure_unique(
            "authority revision",
            authorities.iter().map(|item| &item.value.revision_id),
        )?;
        ensure_unique(
            "context registry revision",
            context_registries
                .iter()
                .map(|item| &item.value.revision_id),
        )?;
        ensure_unique(
            "claim revision",
            claims.iter().map(|item| &item.value.revision_id),
        )?;
        ensure_unique(
            "operation",
            operations.iter().map(|item| &item.value.operation_id),
        )?;
        ensure_unique(
            "context manifest",
            context_manifests.iter().map(|item| &item.value.manifest_id),
        )?;

        let mut diagnostics = Vec::new();
        let protocol_root = protocols.iter().any(|item| {
            item.value.revision_id == bootstrap.initial_protocol_revision.revision_id
                && item.value.payload_sha256 == bootstrap.initial_protocol_revision.payload_sha256
        });
        let authority_root = authorities.iter().any(|item| {
            item.value.revision_id == bootstrap.initial_authority_revision.revision_id
                && item.value.payload_sha256 == bootstrap.initial_authority_revision.payload_sha256
        });
        if !protocol_root {
            diagnostics.push("bootstrap initial protocol revision is missing".into());
        }
        if !authority_root {
            diagnostics.push("bootstrap initial authority revision is missing".into());
        }

        Ok(RepositorySnapshot {
            mode: if protocol_root && authority_root {
                RepositoryMode::V2Active
            } else {
                RepositoryMode::V2Incomplete
            },
            bootstrap: Some(bootstrap),
            protocols,
            authorities,
            context_registries,
            claims,
            operations,
            context_manifests,
            diagnostics,
        })
    }

    fn load_claims(&self, dir: &Path) -> Result<Vec<Loaded<MemoryClaimRevision>>, RepositoryError> {
        let mut loaded = Vec::new();
        for path in yaml_files(dir)? {
            if !path
                .components()
                .any(|component| component.as_os_str() == "revisions")
            {
                continue;
            }
            let item = read_revision::<MemoryClaimRevision>(&path, |value| &value.revision_id)?;
            let claim_dir_matches = path
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                == Some(item.value.claim_id.as_str());
            if !claim_dir_matches {
                return Err(RepositoryError::new(
                    "MEMORY_TAMPERED_ASSET",
                    Some(path),
                    "claim directory does not match claim_id",
                ));
            }
            loaded.push(item);
        }
        loaded.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(loaded)
    }

    fn load_revision_dir<T: CanonicalPayload + DeserializeOwned>(
        &self,
        dir: &Path,
        id: fn(&T) -> &String,
    ) -> Result<Vec<Loaded<T>>, RepositoryError> {
        let mut loaded = yaml_files(dir)?
            .into_iter()
            .map(|path| read_revision::<T>(&path, id))
            .collect::<Result<Vec<_>, _>>()?;
        loaded.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(loaded)
    }
}

fn read_plain<T: DeserializeOwned>(path: &Path) -> Result<T, RepositoryError> {
    reject_unsafe_file(path)?;
    let raw = fs::read(path).map_err(|error| {
        RepositoryError::new("MEMORY_IO", Some(path.to_path_buf()), error.to_string())
    })?;
    serde_yaml::from_slice(&raw).map_err(|error| {
        RepositoryError::new(
            "MEMORY_INVALID_YAML",
            Some(path.to_path_buf()),
            error.to_string(),
        )
    })
}

fn read_revision<T: CanonicalPayload + DeserializeOwned>(
    path: &Path,
    id: fn(&T) -> &String,
) -> Result<Loaded<T>, RepositoryError> {
    reject_unsafe_file(path)?;
    let raw = fs::read(path).map_err(|error| {
        RepositoryError::new("MEMORY_IO", Some(path.to_path_buf()), error.to_string())
    })?;
    let raw_hash = raw_sha256(&raw);
    let value: T = serde_yaml::from_slice(&raw).map_err(|error| {
        RepositoryError::new(
            "MEMORY_INVALID_YAML",
            Some(path.to_path_buf()),
            error.to_string(),
        )
    })?;
    validate_filename(path, id(&value), &raw_hash)?;
    let computed = payload_sha256(&value).map_err(|message| {
        RepositoryError::new("MEMORY_INVALID_PAYLOAD", Some(path.to_path_buf()), message)
    })?;
    let declared = value.declared_payload_sha256().to_string();
    if computed != declared {
        return Err(RepositoryError::new(
            "MEMORY_TAMPERED_ASSET",
            Some(path.to_path_buf()),
            format!("payload hash mismatch: declared {declared}, computed {computed}"),
        ));
    }
    Ok(Loaded {
        path: path.to_path_buf(),
        raw_sha256: raw_hash,
        value,
    })
}

fn validate_filename(path: &Path, id: &str, raw_hash: &str) -> Result<(), RepositoryError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            RepositoryError::new(
                "MEMORY_TAMPERED_ASSET",
                Some(path.to_path_buf()),
                "revision filename is not UTF-8",
            )
        })?;
    let stem = name.strip_suffix(".yaml").ok_or_else(|| {
        RepositoryError::new(
            "MEMORY_TAMPERED_ASSET",
            Some(path.to_path_buf()),
            "revision filename must end in .yaml",
        )
    })?;
    let (file_id, prefix) = stem.rsplit_once('.').ok_or_else(|| {
        RepositoryError::new(
            "MEMORY_TAMPERED_ASSET",
            Some(path.to_path_buf()),
            "revision filename must include a raw hash prefix",
        )
    })?;
    if file_id != id
        || prefix.len() < MIN_RAW_HASH_PREFIX
        || !prefix
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || !raw_hash.starts_with(prefix)
    {
        return Err(RepositoryError::new(
            "MEMORY_TAMPERED_ASSET",
            Some(path.to_path_buf()),
            "revision filename ID/raw hash does not match its bytes",
        ));
    }
    Ok(())
}

fn reject_unsafe_file(path: &Path) -> Result<(), RepositoryError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        RepositoryError::new("MEMORY_IO", Some(path.to_path_buf()), error.to_string())
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RepositoryError::new(
            "MEMORY_TAMPERED_ASSET",
            Some(path.to_path_buf()),
            "Memory assets must be regular files, not symlinks",
        ));
    }
    if metadata.len() > MAX_OBJECT_BYTES {
        return Err(RepositoryError::new(
            "MEMORY_INVALID_PAYLOAD",
            Some(path.to_path_buf()),
            format!("asset exceeds {MAX_OBJECT_BYTES} bytes"),
        ));
    }
    Ok(())
}

fn yaml_files(dir: &Path) -> Result<Vec<PathBuf>, RepositoryError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut pending = vec![dir.to_path_buf()];
    let mut files = Vec::new();
    while let Some(current) = pending.pop() {
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            RepositoryError::new("MEMORY_IO", Some(current.clone()), error.to_string())
        })?;
        if metadata.file_type().is_symlink() {
            return Err(RepositoryError::new(
                "MEMORY_TAMPERED_ASSET",
                Some(current),
                "symlink found in Memory asset tree",
            ));
        }
        for entry in fs::read_dir(&current).map_err(|error| {
            RepositoryError::new("MEMORY_IO", Some(current.clone()), error.to_string())
        })? {
            let path = entry
                .map_err(|error| RepositoryError::new("MEMORY_IO", None, error.to_string()))?
                .path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                RepositoryError::new("MEMORY_IO", Some(path.clone()), error.to_string())
            })?;
            if metadata.file_type().is_symlink() {
                return Err(RepositoryError::new(
                    "MEMORY_TAMPERED_ASSET",
                    Some(path),
                    "symlink found in Memory asset tree",
                ));
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("yaml") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn ensure_unique<'a>(
    label: &str,
    ids: impl Iterator<Item = &'a String>,
) -> Result<(), RepositoryError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id.clone()) {
            return Err(RepositoryError::new(
                "MEMORY_TAMPERED_ASSET",
                None,
                format!("duplicate {label} id {id}"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_absent_and_unactivated_v2() {
        let absent = tempfile::TempDir::new().unwrap();
        assert_eq!(
            V2Repository::new(absent.path()).load().unwrap().mode,
            RepositoryMode::Absent
        );

        let incomplete = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(incomplete.path().join(".notemd/memory/claims")).unwrap();
        let snapshot = V2Repository::new(incomplete.path()).load().unwrap();
        assert_eq!(snapshot.mode, RepositoryMode::V2Incomplete);
        assert!(snapshot.bootstrap.is_none());
    }

    #[test]
    fn owner_relation_self_has_the_frozen_wire_value() {
        assert_eq!(
            serde_yaml::to_string(&OwnerRelation::Self_).unwrap().trim(),
            "self"
        );
        assert_eq!(
            serde_yaml::from_str::<OwnerRelation>("self").unwrap(),
            OwnerRelation::Self_
        );
    }
}
