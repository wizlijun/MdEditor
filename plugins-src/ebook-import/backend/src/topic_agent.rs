//! 电子书主题 Agent 的纯数据协议与任务模板。
//!
//! Agent 只读取插件生成的 inventory 快照，并只写 proposal；canonical
//! `topics.yml`、每书 `meta.yml` 与生成索引均由插件在校验和用户确认后处理。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub const TASK_ID: &str = "organize-ebook-topics";
pub const INVENTORY_REL: &str = ".notemd/ebook-import/topic-design/inventory.yml";
pub const PROPOSAL_REL: &str = ".notemd/ebook-import/topic-design/topics.proposal.yml";
pub const APPLY_JOURNAL_REL: &str = ".notemd/ebook-import/topic-design/apply-journal.json";
pub const MIN_AGENT_TOPICS: usize = 2;
pub const MAX_TOPICS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub schema_version: u32,
    pub books: Vec<InventoryBook>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryBook {
    pub rel: String,
    pub title: String,
    pub creator: Option<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub added_at: Option<String>,
    pub current_topic_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub schema_version: u32,
    pub inventory_sha256: String,
    pub topics: Vec<ProposalTopic>,
    pub assignments: Vec<Assignment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalTopic {
    pub id: String,
    pub label: String,
    pub description: String,
    pub index_file: String,
    pub vocabulary: Vec<Vocabulary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vocabulary {
    pub term: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub book: String,
    pub topic_id: String,
}

#[derive(Debug)]
pub enum TopicAgentError {
    Yaml(serde_yaml::Error),
    Invalid(String),
}

impl fmt::Display for TopicAgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yaml(error) => write!(f, "YAML 无法解析: {error}"),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for TopicAgentError {}

impl From<serde_yaml::Error> for TopicAgentError {
    fn from(value: serde_yaml::Error) -> Self {
        Self::Yaml(value)
    }
}

/// 生成稳定的 inventory 字节。proposal 的哈希必须覆盖这里返回的原始字节，
/// 而不是重新格式化后的等价 YAML。
pub fn inventory_yaml(inventory: &Inventory) -> Result<Vec<u8>, TopicAgentError> {
    validate_inventory(inventory)?;
    Ok(serde_yaml::to_string(inventory)?.into_bytes())
}

pub fn inventory_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 解析 inventory 与 proposal，并以 inventory 文件的精确字节校验新鲜度。
pub fn parse_and_validate_proposal(
    proposal_yaml: &str,
    inventory_bytes: &[u8],
) -> Result<Proposal, TopicAgentError> {
    let inventory: Inventory = serde_yaml::from_slice(inventory_bytes)?;
    validate_inventory(&inventory)?;
    let proposal: Proposal = serde_yaml::from_str(proposal_yaml)?;
    let hash = inventory_sha256(inventory_bytes);
    validate_proposal(&proposal, &inventory, &hash)?;
    Ok(proposal)
}

pub fn validate_proposal(
    proposal: &Proposal,
    inventory: &Inventory,
    expected_inventory_sha256: &str,
) -> Result<(), TopicAgentError> {
    validate_inventory(inventory)?;
    if proposal.schema_version != 1 {
        return invalid("proposal.schema_version 必须为 1");
    }
    if proposal.inventory_sha256 != expected_inventory_sha256 {
        return invalid("proposal 已过期：inventory_sha256 与当前 inventory 不一致");
    }
    if !(MIN_AGENT_TOPICS..=MAX_TOPICS).contains(&proposal.topics.len()) {
        return invalid(format!(
            "Agent proposal 必须包含 {MIN_AGENT_TOPICS}–{MAX_TOPICS} 个主题"
        ));
    }

    let mut ids = BTreeSet::new();
    let mut labels = BTreeSet::new();
    let mut index_files = BTreeSet::new();
    for topic in &proposal.topics {
        if !valid_topic_id(&topic.id) {
            return invalid(format!("非法主题 id: {}", topic.id));
        }
        if !ids.insert(topic.id.as_str()) {
            return invalid(format!("主题 id 重复: {}", topic.id));
        }
        if topic.label.trim().is_empty() {
            return invalid(format!("主题 {} 的 label 不能为空", topic.id));
        }
        if !labels.insert(topic.label.trim()) {
            return invalid(format!("主题 label 重复: {}", topic.label));
        }
        if topic.description.trim().is_empty() {
            return invalid(format!("主题 {} 的 description 不能为空", topic.id));
        }
        if !safe_index_file(&topic.index_file) {
            return invalid(format!("主题 {} 的 index_file 不安全", topic.id));
        }
        if !index_files.insert(topic.index_file.to_lowercase()) {
            return invalid(format!("index_file 重复: {}", topic.index_file));
        }
        if topic.vocabulary.len() < 2 {
            return invalid(format!("主题 {} 至少需要 2 个相关词汇", topic.id));
        }
        let mut terms = BTreeSet::new();
        for item in &topic.vocabulary {
            if item.term.trim().is_empty() || item.description.trim().is_empty() {
                return invalid(format!("主题 {} 的词汇及说明均不能为空", topic.id));
            }
            if !terms.insert(item.term.trim()) {
                return invalid(format!("主题 {} 的词汇重复: {}", topic.id, item.term));
            }
        }
    }

    let inventory_books: BTreeSet<&str> = inventory
        .books
        .iter()
        .map(|book| book.rel.as_str())
        .collect();
    let mut assigned = BTreeSet::new();
    for assignment in &proposal.assignments {
        if !safe_book_rel(&assignment.book) || !inventory_books.contains(assignment.book.as_str()) {
            return invalid(format!(
                "assignment 引用了 inventory 之外的书: {}",
                assignment.book
            ));
        }
        if !assigned.insert(assignment.book.as_str()) {
            return invalid(format!("一本书被重复归类: {}", assignment.book));
        }
        if !ids.contains(assignment.topic_id.as_str()) {
            return invalid(format!(
                "书 {} 引用了未知主题: {}",
                assignment.book, assignment.topic_id
            ));
        }
    }
    if assigned != inventory_books {
        let missing: Vec<_> = inventory_books.difference(&assigned).copied().collect();
        return invalid(format!("proposal 没有覆盖全部书籍: {}", missing.join(", ")));
    }
    Ok(())
}

fn validate_inventory(inventory: &Inventory) -> Result<(), TopicAgentError> {
    if inventory.schema_version != 1 {
        return invalid("inventory.schema_version 必须为 1");
    }
    let mut rels = BTreeSet::new();
    for book in &inventory.books {
        if book.title.trim().is_empty() {
            return invalid(format!("inventory 书名不能为空: {}", book.rel));
        }
        if !safe_book_rel(&book.rel) {
            return invalid(format!("inventory 包含不安全的书籍路径: {}", book.rel));
        }
        if !rels.insert(book.rel.as_str()) {
            return invalid(format!("inventory 书籍路径重复: {}", book.rel));
        }
    }
    Ok(())
}

fn valid_topic_id(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn safe_index_file(file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    !file.trim().is_empty()
        && file == file.trim()
        && !file.chars().any(char::is_control)
        && !file.contains('/')
        && !file.contains('\\')
        && !file.contains("..")
        && lower.ends_with(".index.md")
        && lower != "index.md"
        && lower != "log.md"
}

fn safe_book_rel(rel: &str) -> bool {
    if rel.trim().is_empty()
        || rel != rel.trim()
        || rel.contains('\\')
        || rel.starts_with('/')
        || rel.ends_with('/')
    {
        return false;
    }
    rel.split('/')
        .all(|component| !component.is_empty() && component != "." && component != "..")
}

fn invalid<T>(message: impl Into<String>) -> Result<T, TopicAgentError> {
    Err(TopicAgentError::Invalid(message.into()))
}

const TASK_TEMPLATES: &[(&str, &str)] = &[
    (
        "task.json",
        include_str!("../templates/organize-ebook-topics/task.json"),
    ),
    (
        "CLAUDE.md",
        include_str!("../templates/organize-ebook-topics/CLAUDE.md"),
    ),
    (
        ".claude/settings.json",
        include_str!("../templates/organize-ebook-topics/settings.json"),
    ),
    (
        ".claude/settings.scoped.json",
        include_str!("../templates/organize-ebook-topics/settings.scoped.json"),
    ),
    (
        "AGENTS.md",
        include_str!("../templates/organize-ebook-topics/AGENTS.md"),
    ),
    (
        "CODEX.md",
        include_str!("../templates/organize-ebook-topics/CODEX.md"),
    ),
    (
        "policy.json",
        include_str!("../templates/organize-ebook-topics/policy.json"),
    ),
];

pub fn task_dir(vault: &Path) -> PathBuf {
    vault.join(".notemd/agent-tasks").join(TASK_ID)
}

/// 首次创建默认任务。任何已存在的文件都视为用户维护版本，不覆盖。
pub fn seed_task_templates(vault: &Path) -> std::io::Result<Vec<String>> {
    let root = task_dir(vault);
    let mut written = Vec::new();
    for (relative, body) in TASK_TEMPLATES {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                if let Err(error) = file.write_all(body.as_bytes()) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(error);
                }
                written.push(format!("{TASK_ID}/{relative}"));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory() -> Inventory {
        Inventory {
            schema_version: 1,
            books: vec![
                InventoryBook {
                    rel: "2026-08/Seven Powers".into(),
                    title: "Seven Powers".into(),
                    creator: Some("Hamilton Helmer".into()),
                    publisher: Some("Stripe Press".into()),
                    language: Some("en".into()),
                    added_at: Some("2026-08-27T06:40:15Z".into()),
                    current_topic_id: None,
                },
                InventoryBook {
                    rel: "2026-09/Designing Data-Intensive Applications".into(),
                    title: "Designing Data-Intensive Applications".into(),
                    creator: Some("Martin Kleppmann".into()),
                    publisher: Some("O'Reilly Media".into()),
                    language: Some("en".into()),
                    added_at: Some("2026-09-01T08:12:03Z".into()),
                    current_topic_id: None,
                },
            ],
        }
    }

    fn proposal(inventory_yaml: &[u8]) -> String {
        format!(
            r#"schema_version: 1
inventory_sha256: {}
topics:
  - id: business-strategy
    label: 商业战略
    description: 研究企业如何建立并维持竞争优势。
    index_file: 商业战略.index.md
    vocabulary:
      - term: 竞争优势
        description: 企业持续创造超额价值的能力。
      - term: 护城河
        description: 阻止竞争者复制价值获取方式的屏障。
  - id: software-engineering
    label: 软件工程
    description: 软件系统的设计、交付与演化。
    index_file: 软件工程.index.md
    vocabulary:
      - term: 架构
        description: 系统边界、组成及关键关系的设计。
      - term: 可靠性
        description: 系统在约束条件下持续正确服务的能力。
assignments:
  - book: 2026-08/Seven Powers
    topic_id: business-strategy
  - book: 2026-09/Designing Data-Intensive Applications
    topic_id: software-engineering
"#,
            inventory_sha256(inventory_yaml)
        )
    }

    #[test]
    fn inventory_yaml_is_stable_and_its_sha_covers_exact_bytes() {
        let inv = inventory();
        let first = inventory_yaml(&inv).unwrap();
        let second = inventory_yaml(&inv).unwrap();
        assert_eq!(first, second);
        assert_eq!(inventory_sha256(&first).len(), 64);
        assert_ne!(inventory_sha256(&first), inventory_sha256(b"changed"));
    }

    #[test]
    fn accepts_two_topics_and_exactly_one_assignment_per_book() {
        let bytes = inventory_yaml(&inventory()).unwrap();
        let parsed = parse_and_validate_proposal(&proposal(&bytes), &bytes).unwrap();
        assert_eq!(parsed.topics.len(), 2);
        assert_eq!(parsed.assignments.len(), 2);
    }

    #[test]
    fn accepts_the_inclusive_five_topic_upper_bound() {
        let bytes = inventory_yaml(&inventory()).unwrap();
        let mut parsed = parse_and_validate_proposal(&proposal(&bytes), &bytes).unwrap();
        for n in 3..=5 {
            parsed.topics.push(ProposalTopic {
                id: format!("topic-{n}"),
                label: format!("主题{n}"),
                description: format!("第{n}个稳定领域。"),
                index_file: format!("主题{n}.index.md"),
                vocabulary: vec![
                    Vocabulary {
                        term: format!("术语{n}a"),
                        description: "领域词汇说明。".into(),
                    },
                    Vocabulary {
                        term: format!("术语{n}b"),
                        description: "领域词汇说明。".into(),
                    },
                ],
            });
        }
        validate_proposal(&parsed, &inventory(), &inventory_sha256(&bytes)).unwrap();
    }

    #[test]
    fn rejects_stale_sha_missing_duplicate_and_foreign_assignments() {
        let inv = inventory();
        let bytes = inventory_yaml(&inv).unwrap();
        let good = parse_and_validate_proposal(&proposal(&bytes), &bytes).unwrap();

        let mut stale = good.clone();
        stale.inventory_sha256 = "0".repeat(64);
        assert!(validate_proposal(&stale, &inv, &inventory_sha256(&bytes)).is_err());

        let mut missing = good.clone();
        missing.assignments.pop();
        assert!(validate_proposal(&missing, &inv, &inventory_sha256(&bytes)).is_err());

        let mut duplicate = good.clone();
        duplicate.assignments.push(duplicate.assignments[0].clone());
        assert!(validate_proposal(&duplicate, &inv, &inventory_sha256(&bytes)).is_err());

        let mut foreign = good;
        foreign.assignments[0].book = "../../outside".into();
        assert!(validate_proposal(&foreign, &inv, &inventory_sha256(&bytes)).is_err());
    }

    #[test]
    fn rejects_unsafe_index_unknown_topic_and_six_topics() {
        let inv = inventory();
        let bytes = inventory_yaml(&inv).unwrap();
        let hash = inventory_sha256(&bytes);
        let good = parse_and_validate_proposal(&proposal(&bytes), &bytes).unwrap();

        let mut unsafe_index = good.clone();
        unsafe_index.topics[0].index_file = "../escape.index.md".into();
        assert!(validate_proposal(&unsafe_index, &inv, &hash).is_err());

        let mut unknown = good.clone();
        unknown.assignments[0].topic_id = "other".into();
        assert!(validate_proposal(&unknown, &inv, &hash).is_err());

        let mut six = good;
        for n in 3..=6 {
            six.topics.push(ProposalTopic {
                id: format!("topic-{n}"),
                label: format!("主题{n}"),
                description: "稳定领域。".into(),
                index_file: format!("主题{n}.index.md"),
                vocabulary: vec![
                    Vocabulary {
                        term: format!("a{n}"),
                        description: "说明".into(),
                    },
                    Vocabulary {
                        term: format!("b{n}"),
                        description: "说明".into(),
                    },
                ],
            });
        }
        assert!(validate_proposal(&six, &inv, &hash).is_err());
    }

    #[test]
    fn rejects_one_topic_and_duplicate_or_incomplete_topic_fields() {
        let inv = inventory();
        let bytes = inventory_yaml(&inv).unwrap();
        let hash = inventory_sha256(&bytes);
        let good = parse_and_validate_proposal(&proposal(&bytes), &bytes).unwrap();

        let mut one = good.clone();
        one.topics.pop();
        one.assignments[1].topic_id = one.topics[0].id.clone();
        assert!(validate_proposal(&one, &inv, &hash).is_err());

        let mut duplicate_id = good.clone();
        duplicate_id.topics[1].id = duplicate_id.topics[0].id.clone();
        assert!(validate_proposal(&duplicate_id, &inv, &hash).is_err());

        let mut duplicate_label = good.clone();
        duplicate_label.topics[1].label = duplicate_label.topics[0].label.clone();
        assert!(validate_proposal(&duplicate_label, &inv, &hash).is_err());

        let mut duplicate_file = good.clone();
        duplicate_file.topics[1].index_file = "商业战略.INDEX.MD".into();
        assert!(validate_proposal(&duplicate_file, &inv, &hash).is_err());

        let mut incomplete_vocabulary = good;
        incomplete_vocabulary.topics[0].vocabulary.truncate(1);
        assert!(validate_proposal(&incomplete_vocabulary, &inv, &hash).is_err());
    }

    #[test]
    fn seeds_all_harness_assets_create_only() {
        let vault = tempfile::tempdir().unwrap();
        let written = seed_task_templates(vault.path()).unwrap();
        assert_eq!(written.len(), 7);
        let task = task_dir(vault.path());
        for rel in [
            "task.json",
            "CLAUDE.md",
            ".claude/settings.json",
            ".claude/settings.scoped.json",
            "AGENTS.md",
            "CODEX.md",
            "policy.json",
        ] {
            assert!(task.join(rel).is_file(), "missing {rel}");
        }
        std::fs::write(task.join("AGENTS.md"), "custom").unwrap();
        assert!(seed_task_templates(vault.path()).unwrap().is_empty());
        assert_eq!(
            std::fs::read_to_string(task.join("AGENTS.md")).unwrap(),
            "custom"
        );
    }

    #[test]
    fn every_prompt_fixes_the_only_writable_output() {
        for body in [
            include_str!("../templates/organize-ebook-topics/CLAUDE.md"),
            include_str!("../templates/organize-ebook-topics/AGENTS.md"),
            include_str!("../templates/organize-ebook-topics/CODEX.md"),
        ] {
            assert!(body.contains(PROPOSAL_REL));
            assert!(body.contains(INVENTORY_REL));
            assert!(body.contains("2–5"));
            assert!(body.contains("恰好一次"));
            assert!(body.contains("不要修改"));
        }
    }
}
