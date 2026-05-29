use crate::error::{Error, Result};
use crate::feature_gate::ProfileId;
use crate::util::truncate_content_to_max;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_MAX_SKILL_COUNT: usize = 256;
const DEFAULT_MAX_SKILL_DOC_BYTES: usize = 24 * 1024;
const MAX_SCAN_DEPTH: usize = 4;
const MAX_BODY_SUMMARY_CHARS: usize = 720;
const MAX_PROMPT_SNIPPET_CHARS: usize = 420;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSkillDirConfig {
    pub root: PathBuf,
    pub namespace: String,
    pub scope: AgentSkillScope,
    pub access: AgentSkillAccess,
    pub trust: AgentSkillTrust,
    pub max_skill_count: usize,
    pub max_skill_doc_bytes: usize,
    pub refresh_policy: AgentSkillRefreshPolicy,
}

impl AgentSkillDirConfig {
    pub fn read_only(root: impl Into<PathBuf>, namespace: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            namespace: namespace.into(),
            scope: AgentSkillScope::Global,
            access: AgentSkillAccess::ReadOnly,
            trust: AgentSkillTrust::HostProject,
            max_skill_count: DEFAULT_MAX_SKILL_COUNT,
            max_skill_doc_bytes: DEFAULT_MAX_SKILL_DOC_BYTES,
            refresh_policy: AgentSkillRefreshPolicy::StartupOnly,
        }
    }

    fn effective_max_skill_count(&self) -> usize {
        self.max_skill_count.max(1)
    }

    fn effective_max_skill_doc_bytes(&self) -> usize {
        self.max_skill_doc_bytes
            .clamp(512, DEFAULT_MAX_SKILL_DOC_BYTES)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentSkillScope {
    Global,
    Owner,
    Project { project_id: String },
    Workspace { workspace_id: String },
    Conversation { conversation_id: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSkillAccess {
    ReadOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSkillTrust {
    HostBuiltin,
    HostProject,
    UserMounted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSkillRefreshPolicy {
    StartupOnly,
    ManualRefresh,
    WatchIfSupported,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentSkillResourceSummary {
    pub has_agents_metadata: bool,
    pub script_count: usize,
    pub reference_count: usize,
    pub asset_count: usize,
    pub total_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSkillPackageStatus {
    Active,
    Invalid,
    DisabledByHost,
    HiddenByProfile,
    HiddenByPrivacy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSkillPackageWarning {
    pub code: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSkillDirectoryWarning {
    pub namespace: String,
    pub root_redacted: String,
    pub code: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSkillPackageRecord {
    pub id: String,
    pub namespace: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub body_summary: String,
    pub root: PathBuf,
    pub skill_file: PathBuf,
    pub relative_dir: String,
    pub fingerprint: String,
    pub indexed_at: u64,
    pub stale: bool,
    pub scope: AgentSkillScope,
    pub trust: AgentSkillTrust,
    pub resource_summary: AgentSkillResourceSummary,
    pub status: AgentSkillPackageStatus,
    pub warnings: Vec<AgentSkillPackageWarning>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentSkillRegistrySnapshot {
    pub packages: Vec<AgentSkillPackageRecord>,
    pub mounted_dirs: Vec<AgentSkillMountReport>,
    pub warnings: Vec<AgentSkillDirectoryWarning>,
    pub scanned_at: u64,
}

impl AgentSkillRegistrySnapshot {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn report(&self) -> AgentSkillDirectoryReport {
        let scanned_packages = self.packages.len();
        let active_packages = self
            .packages
            .iter()
            .filter(|record| record.status == AgentSkillPackageStatus::Active)
            .count();
        let invalid_packages = self
            .packages
            .iter()
            .filter(|record| record.status == AgentSkillPackageStatus::Invalid)
            .count();
        let hidden_packages = self
            .packages
            .iter()
            .filter(|record| {
                matches!(
                    record.status,
                    AgentSkillPackageStatus::HiddenByProfile
                        | AgentSkillPackageStatus::HiddenByPrivacy
                        | AgentSkillPackageStatus::DisabledByHost
                )
            })
            .count();
        let stale_packages = self.packages.iter().filter(|record| record.stale).count();
        AgentSkillDirectoryReport {
            mounted_dirs: self.mounted_dirs.len(),
            scanned_packages,
            active_packages,
            invalid_packages,
            hidden_packages,
            stale_packages,
            warnings: self.warnings.clone(),
            mounts: self.mounted_dirs.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentSkillDirectoryReport {
    pub mounted_dirs: usize,
    pub scanned_packages: usize,
    pub active_packages: usize,
    pub invalid_packages: usize,
    pub hidden_packages: usize,
    pub stale_packages: usize,
    pub warnings: Vec<AgentSkillDirectoryWarning>,
    pub mounts: Vec<AgentSkillMountReport>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSkillProjectionAudit {
    pub selected: Vec<AgentSkillProjectionSource>,
    pub rejected: Vec<AgentSkillProjectionRejection>,
    pub budget_limited: bool,
}

impl AgentSkillProjectionAudit {
    pub fn empty() -> Self {
        Self {
            selected: Vec::new(),
            rejected: Vec::new(),
            budget_limited: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSkillProjectionSource {
    pub package_id: String,
    pub namespace: String,
    pub name: String,
    pub fingerprint: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSkillProjectionRejection {
    pub package_id: String,
    pub namespace: String,
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSkillMountReport {
    pub namespace: String,
    pub scope: AgentSkillScope,
    pub trust: AgentSkillTrust,
    pub profile_allowed: bool,
    pub enabled: bool,
    pub root_redacted: String,
    pub package_count: usize,
    pub invalid_count: usize,
    pub last_scan_at: Option<u64>,
    pub last_scan_fingerprint: Option<String>,
    pub warnings: Vec<AgentSkillDirectoryWarning>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSkillRecallHit {
    pub package_id: String,
    pub namespace: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub score: u16,
    pub reasons: Vec<String>,
    pub fingerprint: String,
    pub host_execution_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedAgentSkillHint {
    pub package_id: String,
    pub namespace: String,
    pub name: String,
    pub reason: String,
    pub prompt_snippet: String,
    pub fingerprint: String,
    pub host_execution_required: bool,
}

pub fn build_agent_skill_registry_snapshot(
    profile: ProfileId,
    dirs: &[AgentSkillDirConfig],
    now_secs: u64,
) -> Result<AgentSkillRegistrySnapshot> {
    if dirs.is_empty() {
        return Ok(AgentSkillRegistrySnapshot::empty());
    }
    if agent_skill_dirs_forbidden_by_profile(profile) {
        return Err(Error::config(
            "agent_skill_directory",
            "agent_skill_dir_forbidden_by_profile",
        ));
    }

    let mut snapshot = AgentSkillRegistrySnapshot {
        scanned_at: now_secs,
        ..AgentSkillRegistrySnapshot::default()
    };
    for dir in dirs {
        scan_agent_skill_dir(dir, now_secs, &mut snapshot);
    }
    Ok(snapshot)
}

pub fn agent_skill_dirs_forbidden_by_profile(profile: ProfileId) -> bool {
    matches!(
        profile,
        ProfileId::EspStandaloneMemory | ProfileId::EspEmbeddedSdk
    )
}

pub fn retrieve_agent_skill_hits(
    snapshot: &AgentSkillRegistrySnapshot,
    query: &str,
    limit: usize,
) -> Vec<AgentSkillRecallHit> {
    let normalized_query = normalize_agent_skill_text(query);
    if normalized_query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let query_terms = normalized_query
        .split_whitespace()
        .filter(|term| term.len() > 1)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut hits = snapshot
        .packages
        .iter()
        .filter(|record| record.status == AgentSkillPackageStatus::Active)
        .filter_map(|record| score_agent_skill_record(record, &normalized_query, &query_terms))
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.namespace.cmp(&right.namespace))
            .then_with(|| left.name.cmp(&right.name))
    });
    hits.truncate(limit);
    hits
}

pub fn build_projected_agent_skill_hints(
    snapshot: &AgentSkillRegistrySnapshot,
    hits: &[AgentSkillRecallHit],
    max_chars: usize,
) -> (Vec<ProjectedAgentSkillHint>, AgentSkillProjectionAudit) {
    if hits.is_empty() || max_chars == 0 {
        return (Vec::new(), AgentSkillProjectionAudit::empty());
    }
    let mut used = 0usize;
    let mut selected = Vec::new();
    let mut rejected = Vec::new();
    let mut hints = Vec::new();
    let mut budget_limited = false;
    for hit in hits {
        let Some(record) = snapshot
            .packages
            .iter()
            .find(|record| record.id == hit.package_id)
        else {
            rejected.push(AgentSkillProjectionRejection {
                package_id: hit.package_id.clone(),
                namespace: hit.namespace.clone(),
                name: hit.name.clone(),
                reason: "package_not_in_registry_snapshot".to_string(),
            });
            continue;
        };
        let reason = hit
            .reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "agent_skill_description_match".to_string());
        let snippet = format!(
            "{}: {}. {}",
            record.title.trim(),
            record.description.trim(),
            record.body_summary.trim()
        );
        let prompt_snippet = truncate_content_to_max(snippet.trim(), MAX_PROMPT_SNIPPET_CHARS);
        let next_used = used.saturating_add(prompt_snippet.chars().count());
        if next_used > max_chars && !hints.is_empty() {
            budget_limited = true;
            rejected.push(AgentSkillProjectionRejection {
                package_id: hit.package_id.clone(),
                namespace: hit.namespace.clone(),
                name: hit.name.clone(),
                reason: "agent_skill_projection_budget_limited".to_string(),
            });
            continue;
        }
        used = next_used;
        selected.push(AgentSkillProjectionSource {
            package_id: hit.package_id.clone(),
            namespace: hit.namespace.clone(),
            name: hit.name.clone(),
            fingerprint: hit.fingerprint.clone(),
            reason: reason.clone(),
        });
        hints.push(ProjectedAgentSkillHint {
            package_id: hit.package_id.clone(),
            namespace: hit.namespace.clone(),
            name: hit.name.clone(),
            reason,
            prompt_snippet: prompt_snippet.to_string(),
            fingerprint: hit.fingerprint.clone(),
            host_execution_required: true,
        });
    }
    (
        hints,
        AgentSkillProjectionAudit {
            selected,
            rejected,
            budget_limited,
        },
    )
}

fn scan_agent_skill_dir(
    config: &AgentSkillDirConfig,
    now_secs: u64,
    snapshot: &mut AgentSkillRegistrySnapshot,
) {
    let namespace = config.namespace.trim().to_string();
    let root_redacted = redact_path(&config.root);
    let mut dir_warnings = Vec::new();
    if namespace.is_empty() || namespace.contains('/') || namespace.contains('\\') {
        dir_warnings.push(AgentSkillDirectoryWarning {
            namespace,
            root_redacted,
            code: "invalid_namespace".to_string(),
            detail: "namespace must be non-empty and must not contain path separators".to_string(),
        });
        snapshot.warnings.extend(dir_warnings.clone());
        snapshot.mounted_dirs.push(AgentSkillMountReport {
            namespace: config.namespace.clone(),
            scope: config.scope.clone(),
            trust: config.trust,
            profile_allowed: true,
            enabled: false,
            root_redacted: redact_path(&config.root),
            package_count: 0,
            invalid_count: 0,
            last_scan_at: Some(now_secs),
            last_scan_fingerprint: None,
            warnings: dir_warnings,
        });
        return;
    }

    let root = match config.root.canonicalize() {
        Ok(root) if root.is_dir() => root,
        _ => {
            dir_warnings.push(AgentSkillDirectoryWarning {
                namespace: namespace.clone(),
                root_redacted: redact_path(&config.root),
                code: "root_unavailable".to_string(),
                detail: "Agent Skill root does not exist or is not a directory".to_string(),
            });
            snapshot.warnings.extend(dir_warnings.clone());
            snapshot.mounted_dirs.push(AgentSkillMountReport {
                namespace,
                scope: config.scope.clone(),
                trust: config.trust,
                profile_allowed: true,
                enabled: false,
                root_redacted: redact_path(&config.root),
                package_count: 0,
                invalid_count: 0,
                last_scan_at: Some(now_secs),
                last_scan_fingerprint: None,
                warnings: dir_warnings,
            });
            return;
        }
    };

    let mut package_dirs = Vec::new();
    collect_agent_skill_package_dirs(
        &root,
        &root,
        0,
        config.effective_max_skill_count(),
        &namespace,
        &mut package_dirs,
        &mut dir_warnings,
    );
    let before = snapshot.packages.len();
    for package_dir in package_dirs
        .into_iter()
        .take(config.effective_max_skill_count())
    {
        let record = parse_agent_skill_package(
            &root,
            &package_dir,
            config,
            now_secs,
            config.effective_max_skill_doc_bytes(),
        );
        snapshot.packages.push(record);
    }
    let package_count = snapshot.packages.len().saturating_sub(before);
    let invalid_count = snapshot.packages[before..]
        .iter()
        .filter(|record| record.status == AgentSkillPackageStatus::Invalid)
        .count();
    let fingerprint = fingerprint_dir_records(&snapshot.packages[before..]);
    snapshot.warnings.extend(dir_warnings.clone());
    snapshot.mounted_dirs.push(AgentSkillMountReport {
        namespace,
        scope: config.scope.clone(),
        trust: config.trust,
        profile_allowed: true,
        enabled: true,
        root_redacted: redact_path(&root),
        package_count,
        invalid_count,
        last_scan_at: Some(now_secs),
        last_scan_fingerprint: Some(fingerprint),
        warnings: dir_warnings,
    });
}

fn collect_agent_skill_package_dirs(
    root: &Path,
    current: &Path,
    depth: usize,
    limit: usize,
    namespace: &str,
    out: &mut Vec<PathBuf>,
    warnings: &mut Vec<AgentSkillDirectoryWarning>,
) {
    if out.len() >= limit || depth > MAX_SCAN_DEPTH {
        return;
    }
    if current.join("SKILL.md").is_file() {
        out.push(current.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= limit {
            break;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            warnings.push(AgentSkillDirectoryWarning {
                namespace: namespace.to_string(),
                root_redacted: redact_path(root),
                code: "symlink_skipped".to_string(),
                detail: redact_path(&path),
            });
            continue;
        }
        if file_type.is_dir() {
            if let Ok(canon) = path.canonicalize() {
                if !canon.starts_with(root) {
                    warnings.push(AgentSkillDirectoryWarning {
                        namespace: namespace.to_string(),
                        root_redacted: redact_path(root),
                        code: "root_escape_skipped".to_string(),
                        detail: redact_path(&path),
                    });
                    continue;
                }
            }
            collect_agent_skill_package_dirs(
                root,
                &path,
                depth + 1,
                limit,
                namespace,
                out,
                warnings,
            );
        }
    }
}

fn parse_agent_skill_package(
    root: &Path,
    package_dir: &Path,
    config: &AgentSkillDirConfig,
    now_secs: u64,
    max_doc_bytes: usize,
) -> AgentSkillPackageRecord {
    let skill_file = package_dir.join("SKILL.md");
    let relative_dir = package_dir
        .strip_prefix(root)
        .unwrap_or(package_dir)
        .to_string_lossy()
        .trim_matches('/')
        .to_string();
    let resource_summary = summarize_agent_skill_resources(package_dir);
    let mut warnings = Vec::new();
    let raw = match fs::read(&skill_file) {
        Ok(bytes) => {
            if bytes.len() > max_doc_bytes {
                warnings.push(AgentSkillPackageWarning {
                    code: "skill_doc_truncated".to_string(),
                    detail: format!("{} > {}", bytes.len(), max_doc_bytes),
                });
            }
            String::from_utf8_lossy(&bytes[..bytes.len().min(max_doc_bytes)]).into_owned()
        }
        Err(error) => {
            warnings.push(AgentSkillPackageWarning {
                code: "skill_doc_unreadable".to_string(),
                detail: error.to_string(),
            });
            String::new()
        }
    };
    let parsed = parse_skill_markdown(&raw);
    let fallback_name = package_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("agent-skill")
        .to_string();
    let name = parsed.name.unwrap_or(fallback_name);
    let description = parsed.description.unwrap_or_default();
    if description.trim().is_empty() {
        warnings.push(AgentSkillPackageWarning {
            code: "missing_description".to_string(),
            detail: "SKILL.md frontmatter must include description".to_string(),
        });
    }
    if name.trim().is_empty() {
        warnings.push(AgentSkillPackageWarning {
            code: "missing_name".to_string(),
            detail: "SKILL.md frontmatter must include name".to_string(),
        });
    }
    let body_summary = summarize_agent_skill_body(&parsed.body);
    let status = if warnings
        .iter()
        .any(|warning| warning.code == "missing_description" || warning.code == "missing_name")
    {
        AgentSkillPackageStatus::Invalid
    } else {
        AgentSkillPackageStatus::Active
    };
    let title = name.replace(['_', '-'], " ");
    let fingerprint = fingerprint_package(
        &config.namespace,
        &name,
        &description,
        &body_summary,
        &relative_dir,
    );
    let id = stable_agent_skill_id(&config.namespace, &name, &relative_dir);
    AgentSkillPackageRecord {
        id,
        namespace: config.namespace.trim().to_string(),
        name,
        title,
        description,
        body_summary,
        root: root.to_path_buf(),
        skill_file,
        relative_dir,
        fingerprint,
        indexed_at: now_secs,
        stale: false,
        scope: config.scope.clone(),
        trust: config.trust,
        resource_summary,
        status,
        warnings,
    }
}

#[derive(Default)]
struct ParsedSkillMarkdown {
    name: Option<String>,
    description: Option<String>,
    body: String,
}

fn parse_skill_markdown(raw: &str) -> ParsedSkillMarkdown {
    let mut parsed = ParsedSkillMarkdown::default();
    let mut lines = raw.lines();
    if lines.next().map(str::trim) != Some("---") {
        parsed.body = raw.to_string();
        return parsed;
    }
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let value = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        match key.trim() {
            "name" => parsed.name = Some(value),
            "description" => parsed.description = Some(value),
            _ => {}
        }
    }
    parsed.body = lines.collect::<Vec<_>>().join("\n");
    parsed
}

fn summarize_agent_skill_body(body: &str) -> String {
    let summary = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    truncate_content_to_max(summary.trim(), MAX_BODY_SUMMARY_CHARS).to_string()
}

fn summarize_agent_skill_resources(package_dir: &Path) -> AgentSkillResourceSummary {
    let mut summary = AgentSkillResourceSummary {
        has_agents_metadata: package_dir.join("agents/openai.yaml").is_file(),
        ..AgentSkillResourceSummary::default()
    };
    let scripts = summarize_resource_dir(&package_dir.join("scripts"));
    summary.script_count = scripts.0;
    summary.total_bytes = summary.total_bytes.saturating_add(scripts.1);
    let references = summarize_resource_dir(&package_dir.join("references"));
    summary.reference_count = references.0;
    summary.total_bytes = summary.total_bytes.saturating_add(references.1);
    let assets = summarize_resource_dir(&package_dir.join("assets"));
    summary.asset_count = assets.0;
    summary.total_bytes = summary.total_bytes.saturating_add(assets.1);
    summary
}

fn summarize_resource_dir(path: &Path) -> (usize, u64) {
    let Ok(entries) = fs::read_dir(path) else {
        return (0, 0);
    };
    let mut count = 0usize;
    let mut bytes = 0u64;
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_file() {
            count = count.saturating_add(1);
            bytes = bytes.saturating_add(metadata.len());
        }
    }
    (count, bytes)
}

fn score_agent_skill_record(
    record: &AgentSkillPackageRecord,
    normalized_query: &str,
    query_terms: &[String],
) -> Option<AgentSkillRecallHit> {
    let haystack = normalize_agent_skill_text(&format!(
        "{} {} {} {}",
        record.name, record.title, record.description, record.body_summary
    ));
    let mut score = 0u16;
    let mut reasons = Vec::new();
    if haystack.contains(normalized_query) {
        score = score.saturating_add(600);
        reasons.push("agent_skill_exact_phrase_match".to_string());
    }
    for term in query_terms {
        if haystack.contains(term) {
            score = score.saturating_add(120);
        }
    }
    if record
        .description
        .to_ascii_lowercase()
        .contains(normalized_query)
    {
        score = score.saturating_add(240);
        reasons.push("agent_skill_description_match".to_string());
    }
    if score == 0 {
        return None;
    }
    if reasons.is_empty() {
        reasons.push("agent_skill_term_overlap".to_string());
    }
    Some(AgentSkillRecallHit {
        package_id: record.id.clone(),
        namespace: record.namespace.clone(),
        name: record.name.clone(),
        title: record.title.clone(),
        description: record.description.clone(),
        score,
        reasons,
        fingerprint: record.fingerprint.clone(),
        host_execution_required: true,
    })
}

fn normalize_agent_skill_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn stable_agent_skill_id(namespace: &str, name: &str, relative_dir: &str) -> String {
    format!("{namespace}:{name}:{}", fnv1a64(relative_dir.as_bytes()))
}

fn fingerprint_package(
    namespace: &str,
    name: &str,
    description: &str,
    body_summary: &str,
    relative_dir: &str,
) -> String {
    let mut buffer = String::new();
    push_hash_part(&mut buffer, namespace);
    push_hash_part(&mut buffer, name);
    push_hash_part(&mut buffer, description);
    push_hash_part(&mut buffer, body_summary);
    push_hash_part(&mut buffer, relative_dir);
    format!("{:016x}", fnv1a64(buffer.as_bytes()))
}

fn fingerprint_dir_records(records: &[AgentSkillPackageRecord]) -> String {
    let mut buffer = String::new();
    let mut records = records
        .iter()
        .map(|record| (record.id.as_str(), record.fingerprint.as_str()))
        .collect::<Vec<_>>();
    records.sort_unstable();
    for (id, fingerprint) in records {
        push_hash_part(&mut buffer, id);
        push_hash_part(&mut buffer, fingerprint);
    }
    format!("{:016x}", fnv1a64(buffer.as_bytes()))
}

fn push_hash_part(buffer: &mut String, value: &str) {
    buffer.push_str(&value.len().to_string());
    buffer.push(':');
    buffer.push_str(value);
    buffer.push('|');
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn redact_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!(".../{name}"))
        .unwrap_or_else(|| "...".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "bm-agent-skill-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn write_skill(root: &Path, dir: &str, body: &str) -> PathBuf {
        let skill_dir = root.join(dir);
        fs::create_dir_all(&skill_dir).expect("skill dir");
        fs::write(skill_dir.join("SKILL.md"), body).expect("skill");
        skill_dir
    }

    #[test]
    fn parser_indexes_valid_agent_skill_without_using_skill_storage() {
        let root = temp_root("valid");
        write_skill(
            &root,
            "release-helper",
            r#"---
name: release-helper
description: "Use when checking release gates and artifact integrity."
---
# Release helper
Run release gates, inspect artifacts, and report blockers.
"#,
        );

        let snapshot = build_agent_skill_registry_snapshot(
            ProfileId::ServerLinuxDevFull,
            &[AgentSkillDirConfig::read_only(&root, "host")],
            10,
        )
        .expect("snapshot");

        assert_eq!(snapshot.report().active_packages, 1);
        let hits = retrieve_agent_skill_hits(&snapshot, "release artifact gates", 4);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].namespace, "host");
        assert!(hits[0].host_execution_required);
    }

    #[test]
    fn invalid_skill_frontmatter_becomes_warning_not_panic() {
        let root = temp_root("invalid");
        write_skill(&root, "broken", "# Broken\nNo frontmatter.");

        let snapshot = build_agent_skill_registry_snapshot(
            ProfileId::ServerLinuxDevFull,
            &[AgentSkillDirConfig::read_only(&root, "host")],
            10,
        )
        .expect("snapshot");

        let report = snapshot.report();
        assert_eq!(report.scanned_packages, 1);
        assert_eq!(report.invalid_packages, 1);
    }

    #[test]
    fn esp_profiles_reject_agent_skill_directories() {
        let root = temp_root("esp");
        let err = build_agent_skill_registry_snapshot(
            ProfileId::EspStandaloneMemory,
            &[AgentSkillDirConfig::read_only(&root, "host")],
            10,
        )
        .expect_err("esp should reject dirs");

        assert_eq!(err.stage(), "agent_skill_directory");
        assert!(err
            .to_string()
            .contains("agent_skill_dir_forbidden_by_profile"));
    }

    #[test]
    fn projection_hints_are_budgeted_and_audited() {
        let root = temp_root("projection");
        write_skill(
            &root,
            "camera-check",
            r#"---
name: camera-check
description: "Use when diagnosing camera device capture failures."
---
Check camera permissions, enumerate devices, then verify capture format.
"#,
        );
        let snapshot = build_agent_skill_registry_snapshot(
            ProfileId::ServerLinuxDevFull,
            &[AgentSkillDirConfig::read_only(&root, "host")],
            10,
        )
        .expect("snapshot");
        let hits = retrieve_agent_skill_hits(&snapshot, "camera capture", 4);
        let (hints, audit) = build_projected_agent_skill_hints(&snapshot, &hits, 128);

        assert_eq!(hints.len(), 1);
        assert_eq!(audit.selected.len(), 1);
        assert!(hints[0].prompt_snippet.contains("camera"));
    }

    #[test]
    fn directory_fingerprint_is_stable_for_scan_order() {
        let root = temp_root("fingerprint-order");
        write_skill(
            &root,
            "camera-check",
            r#"---
name: camera-check
description: "Use when diagnosing camera capture failures."
---
# Camera check
Verify capture devices.
"#,
        );
        write_skill(
            &root,
            "release-helper",
            r#"---
name: release-helper
description: "Use when checking release gates."
---
# Release helper
Run release gates.
"#,
        );

        let snapshot = build_agent_skill_registry_snapshot(
            ProfileId::ServerLinuxDevFull,
            &[AgentSkillDirConfig::read_only(&root, "host")],
            10,
        )
        .expect("snapshot");
        let mut reversed = snapshot.packages.clone();
        reversed.reverse();

        assert_eq!(
            fingerprint_dir_records(&snapshot.packages),
            fingerprint_dir_records(&reversed)
        );
    }
}
