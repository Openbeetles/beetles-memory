use std::fs::File;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use bm_sdk::MemoryProjectionReport;

use crate::{GatewayAuditConfig, GatewayError, GatewayScopeResolution, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayAuditStage {
    Projection,
    Upstream,
    Maintenance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayAuditOutcome {
    Succeeded,
    Failed,
    Skipped,
    NotExecuted,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GatewayAuditStageReport {
    pub stage: GatewayAuditStage,
    pub outcome: GatewayAuditOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayProjectionAuditStatus {
    NotRecorded,
    Recorded,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GatewayProjectionAuditRecord {
    pub status: GatewayProjectionAuditStatus,
    pub reason: String,
    pub projection_chars: usize,
    pub redacted: bool,
    pub redacted_source_ids: Vec<String>,
    pub local_diagnostic_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
}

impl GatewayProjectionAuditRecord {
    pub fn not_recorded(reason: impl Into<String>, projection_chars: usize) -> Self {
        Self {
            status: GatewayProjectionAuditStatus::NotRecorded,
            reason: reason.into(),
            projection_chars,
            redacted: true,
            redacted_source_ids: Vec::new(),
            local_diagnostic_path: None,
            block: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GatewayAuditReport {
    pub audit_id: String,
    pub endpoint: String,
    pub client_profile: String,
    pub model_alias: String,
    pub scope: GatewayScopeResolution,
    pub projection_record: GatewayProjectionAuditRecord,
    pub stages: Vec<GatewayAuditStageReport>,
    pub notes: Vec<String>,
}

impl GatewayAuditReport {
    pub fn new(
        audit_id: impl Into<String>,
        endpoint: impl Into<String>,
        client_profile: impl Into<String>,
        model_alias: impl Into<String>,
        scope: GatewayScopeResolution,
    ) -> Self {
        Self {
            audit_id: audit_id.into(),
            endpoint: endpoint.into(),
            client_profile: client_profile.into(),
            model_alias: model_alias.into(),
            scope,
            projection_record: GatewayProjectionAuditRecord::not_recorded(
                "projection_not_attempted",
                0,
            ),
            stages: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn record_stage(&mut self, stage: GatewayAuditStage, outcome: GatewayAuditOutcome) {
        self.stages.push(GatewayAuditStageReport { stage, outcome });
    }

    pub fn record_note(&mut self, note: impl Into<String>) {
        let note = note.into();
        if !note.trim().is_empty() {
            self.notes.push(note);
        }
    }

    pub fn record_projection(
        &mut self,
        config: &GatewayAuditConfig,
        projection: &MemoryProjectionReport,
    ) -> Result<()> {
        let projection_chars = projection.system_memory_block.chars().count();
        if !config.enabled {
            self.projection_record = GatewayProjectionAuditRecord::not_recorded(
                "gateway_audit_disabled",
                projection_chars,
            );
            return Ok(());
        }
        if !config.record_raw_projection {
            self.projection_record = GatewayProjectionAuditRecord::not_recorded(
                "raw_projection_recording_disabled",
                projection_chars,
            );
            return Ok(());
        }

        let redaction = redacted_projection_block(projection);
        let mut record = GatewayProjectionAuditRecord {
            status: GatewayProjectionAuditStatus::Recorded,
            reason: if redaction.redacted {
                "raw_projection_recorded_redacted".to_string()
            } else {
                "raw_projection_recorded".to_string()
            },
            projection_chars,
            redacted: redaction.redacted,
            redacted_source_ids: redaction.redacted_source_ids,
            local_diagnostic_path: None,
            block: Some(redaction.block),
        };

        if let Some(dir) = &config.raw_projection_diagnostic_path {
            let owner = ProjectionDiagnosticDirectory::open(dir)?;
            let transaction = owner.begin_transaction()?;
            let diagnostic_path = projection_diagnostic_path(
                owner.path(),
                &self.audit_id,
                &projection.runtime_projection.projection_id,
            );
            record.local_diagnostic_path = Some(diagnostic_path.display().to_string());
            write_projection_diagnostic(&transaction, &diagnostic_path, &record)?;
            enforce_projection_diagnostic_retention(
                &transaction,
                config.raw_projection_retention_limit,
            )?;
        }
        self.projection_record = record;
        Ok(())
    }
}

struct ProjectionRedaction {
    block: String,
    redacted: bool,
    redacted_source_ids: Vec<String>,
}

fn redacted_projection_block(projection: &MemoryProjectionReport) -> ProjectionRedaction {
    let mut redacted_source_ids = projection
        .private_disclosure_integrity
        .redacted_source_ids
        .clone();
    redacted_source_ids.sort();
    redacted_source_ids.dedup();
    let block = projection.projection_surfaces.gateway_raw_audit.clone();
    ProjectionRedaction {
        redacted: block != projection.system_memory_block || !redacted_source_ids.is_empty(),
        block,
        redacted_source_ids,
    }
}

fn projection_diagnostic_path(dir: &Path, audit_id: &str, projection_id: &str) -> PathBuf {
    let seed = format!("{audit_id}-{projection_id}");
    let sanitized = sanitize_diagnostic_name(&seed);
    dir.join(format!("gateway-projection-{sanitized}.json"))
}

fn sanitize_diagnostic_name(seed: &str) -> String {
    let mut out = seed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "projection".to_string()
    } else {
        trimmed.chars().take(160).collect()
    }
}

fn write_projection_diagnostic(
    owner: &ProjectionDiagnosticTransaction<'_>,
    diagnostic_path: &Path,
    record: &GatewayProjectionAuditRecord,
) -> Result<()> {
    if diagnostic_path.parent() != Some(owner.path()) {
        return Err(GatewayError::runtime_unavailable(
            "projection diagnostic path escaped its owner directory",
        ));
    }
    let payload = serde_json::to_vec_pretty(record).map_err(|error| {
        GatewayError::runtime_unavailable(format!(
            "projection diagnostic serialize failed: {error}"
        ))
    })?;
    let file_name = diagnostic_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| GatewayError::runtime_unavailable("invalid projection diagnostic name"))?;
    let staging_name = format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let mut staging = owner.create_new_file(&staging_name)?;
    let result = (|| {
        staging
            .write_all(&payload)
            .and_then(|_| staging.sync_all())
            .map_err(|error| projection_io_error("write_staging", error))?;
        owner.publish(&staging, &staging_name, file_name)
    })();
    if result.is_err() {
        let _ = owner.discard_if_same(&staging, &staging_name);
    }
    result
}

fn enforce_projection_diagnostic_retention(
    owner: &ProjectionDiagnosticTransaction<'_>,
    limit: usize,
) -> Result<()> {
    if limit == 0 {
        return Err(GatewayError::invalid_config(
            "audit.raw_projection_retention_limit must be greater than zero",
        ));
    }
    let mut entries = Vec::new();
    for file_name in owner.entry_names()? {
        if !file_name.starts_with("gateway-projection-") || !file_name.ends_with(".json") {
            continue;
        }
        let metadata = owner.regular_file_metadata(&file_name)?;
        entries.push((metadata.modified, file_name));
    }
    if entries.len() <= limit {
        return Ok(());
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let remove_count = entries.len() - limit;
    for (_, file_name) in entries.into_iter().take(remove_count) {
        owner.remove_regular_file(&file_name)?;
    }
    Ok(())
}

#[derive(Debug)]
struct ProjectionDiagnosticDirectory {
    path: PathBuf,
    directory: File,
}

#[derive(Debug)]
struct ProjectionDiagnosticTransaction<'a> {
    owner: &'a ProjectionDiagnosticDirectory,
}

impl std::ops::Deref for ProjectionDiagnosticTransaction<'_> {
    type Target = ProjectionDiagnosticDirectory;

    fn deref(&self) -> &Self::Target {
        self.owner
    }
}

#[cfg(unix)]
impl Drop for ProjectionDiagnosticTransaction<'_> {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe {
            libc::flock(self.owner.directory.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DiagnosticFileMetadata {
    modified: i128,
}

impl ProjectionDiagnosticDirectory {
    fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(unix)]
    fn begin_transaction(&self) -> Result<ProjectionDiagnosticTransaction<'_>> {
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(self.directory.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(projection_io_error(
                "transaction_lock",
                io::Error::last_os_error(),
            ));
        }
        Ok(ProjectionDiagnosticTransaction { owner: self })
    }

    #[cfg(not(unix))]
    fn begin_transaction(&self) -> Result<ProjectionDiagnosticTransaction<'_>> {
        Err(GatewayError::runtime_unavailable(
            "raw projection diagnostics require cross-process directory locks",
        ))
    }

    #[cfg(unix)]
    fn open(path: &Path) -> Result<Self> {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};

        let normalized_path = normalize_diagnostic_creation_path(path)?;
        let mut current = open_diagnostic_directory(if normalized_path.is_absolute() {
            Path::new("/")
        } else {
            Path::new(".")
        })?;
        for component in normalized_path.components() {
            let Component::Normal(name) = component else {
                if matches!(component, Component::RootDir | Component::CurDir) {
                    continue;
                }
                return Err(GatewayError::runtime_unavailable(
                    "projection diagnostic path contains a parent or platform prefix",
                ));
            };
            let name = CString::new(name.as_encoded_bytes()).map_err(|_| {
                GatewayError::runtime_unavailable(
                    "projection diagnostic path contains an interior NUL",
                )
            })?;
            let mut fd = unsafe {
                libc::openat(
                    current.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
                let status = unsafe { libc::mkdirat(current.as_raw_fd(), name.as_ptr(), 0o700) };
                if status != 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                    return Err(projection_io_error(
                        "create_dir",
                        io::Error::last_os_error(),
                    ));
                }
                fd = unsafe {
                    libc::openat(
                        current.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
            }
            if fd < 0 {
                return Err(projection_io_error("open_dir", io::Error::last_os_error()));
            }
            current = unsafe { File::from_raw_fd(fd) };
        }
        let metadata = current
            .metadata()
            .map_err(|error| projection_io_error("diagnostic_dir_metadata", error))?;
        use std::os::unix::fs::MetadataExt;
        if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(GatewayError::runtime_unavailable(
                "projection diagnostic directory must be a current-user-owned real directory",
            ));
        }
        if unsafe { libc::fchmod(current.as_raw_fd(), 0o700) } != 0 {
            return Err(projection_io_error(
                "diagnostic_dir_permissions",
                io::Error::last_os_error(),
            ));
        }
        Ok(Self {
            path: normalized_path,
            directory: current,
        })
    }

    #[cfg(not(unix))]
    fn open(_path: &Path) -> Result<Self> {
        Err(GatewayError::runtime_unavailable(
            "raw projection diagnostics require retained secure directory handles",
        ))
    }

    #[cfg(unix)]
    fn create_new_file(&self, name: &str) -> Result<File> {
        use std::os::fd::{AsRawFd, FromRawFd};
        let name = diagnostic_component(name)?;
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(projection_io_error(
                "create_staging",
                io::Error::last_os_error(),
            ));
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    #[cfg(not(unix))]
    fn create_new_file(&self, _name: &str) -> Result<File> {
        unreachable!("non-Unix diagnostic directories cannot be opened")
    }

    #[cfg(unix)]
    fn publish(&self, staged: &File, staged_name: &str, final_name: &str) -> Result<()> {
        let staged_name = diagnostic_component(staged_name)?;
        let final_name = diagnostic_component(final_name)?;
        rename_noreplace(&self.directory, &staged_name, &final_name, "publish")?;
        self.require_named_identity(&final_name, staged)?;
        self.sync()
    }

    #[cfg(not(unix))]
    fn publish(&self, _staged: &File, _staged_name: &str, _final_name: &str) -> Result<()> {
        unreachable!("non-Unix diagnostic directories cannot be opened")
    }

    #[cfg(unix)]
    fn discard_if_same(&self, staged: &File, staged_name: &str) -> Result<()> {
        use std::os::fd::AsRawFd;
        let staged_name = diagnostic_component(staged_name)?;
        self.require_named_identity(&staged_name, staged)?;
        if unsafe { libc::unlinkat(self.directory.as_raw_fd(), staged_name.as_ptr(), 0) } != 0 {
            return Err(projection_io_error(
                "discard_staging",
                io::Error::last_os_error(),
            ));
        }
        self.sync()
    }

    #[cfg(not(unix))]
    fn discard_if_same(&self, _staged: &File, _staged_name: &str) -> Result<()> {
        unreachable!("non-Unix diagnostic directories cannot be opened")
    }

    #[cfg(unix)]
    fn entry_names(&self) -> Result<Vec<String>> {
        use std::ffi::CStr;
        use std::os::fd::AsRawFd;
        let current = std::ffi::CString::new(".").expect("static directory component");
        let iterator_fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                current.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if iterator_fd < 0 {
            return Err(projection_io_error(
                "open_dir_iterator",
                io::Error::last_os_error(),
            ));
        }
        let stream = unsafe { libc::fdopendir(iterator_fd) };
        if stream.is_null() {
            unsafe { libc::close(iterator_fd) };
            return Err(projection_io_error("read_dir", io::Error::last_os_error()));
        }
        let mut names = Vec::new();
        loop {
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                break;
            }
            let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            let name = std::str::from_utf8(bytes).map_err(|_| {
                GatewayError::runtime_unavailable(
                    "projection diagnostic directory contains a non-UTF-8 entry",
                )
            })?;
            names.push(name.to_string());
        }
        if unsafe { libc::closedir(stream) } != 0 {
            return Err(projection_io_error("close_dir", io::Error::last_os_error()));
        }
        Ok(names)
    }

    #[cfg(not(unix))]
    fn entry_names(&self) -> Result<Vec<String>> {
        unreachable!("non-Unix diagnostic directories cannot be opened")
    }

    #[cfg(unix)]
    fn regular_file_metadata(&self, name: &str) -> Result<DiagnosticFileMetadata> {
        use std::os::fd::AsRawFd;
        let name = diagnostic_component(name)?;
        let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
        if unsafe {
            libc::fstatat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                &mut stat,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(projection_io_error("metadata", io::Error::last_os_error()));
        }
        if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(GatewayError::runtime_unavailable(
                "projection diagnostic retention encountered a non-regular owner",
            ));
        }
        Ok(DiagnosticFileMetadata {
            modified: i128::from(stat.st_mtime),
        })
    }

    #[cfg(not(unix))]
    fn regular_file_metadata(&self, _name: &str) -> Result<DiagnosticFileMetadata> {
        unreachable!("non-Unix diagnostic directories cannot be opened")
    }

    #[cfg(unix)]
    fn remove_regular_file(&self, name: &str) -> Result<()> {
        use std::os::fd::{AsRawFd, FromRawFd};
        let source_name = diagnostic_component(name)?;
        let source_fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                source_name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if source_fd < 0 {
            return Err(projection_io_error(
                "retention_open",
                io::Error::last_os_error(),
            ));
        }
        let source = unsafe { File::from_raw_fd(source_fd) };
        if !source
            .metadata()
            .map_err(|error| projection_io_error("retention_metadata", error))?
            .is_file()
        {
            return Err(GatewayError::runtime_unavailable(
                "projection diagnostic retention encountered a non-regular owner",
            ));
        }
        let quarantine_name = format!(
            ".quarantine.{}.{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let quarantine = diagnostic_component(&quarantine_name)?;
        rename_noreplace(
            &self.directory,
            &source_name,
            &quarantine,
            "retention_quarantine",
        )?;
        self.require_named_identity(&quarantine, &source)?;
        if unsafe { libc::unlinkat(self.directory.as_raw_fd(), quarantine.as_ptr(), 0) } != 0 {
            return Err(projection_io_error(
                "remove_old",
                io::Error::last_os_error(),
            ));
        }
        self.sync()
    }

    #[cfg(not(unix))]
    fn remove_regular_file(&self, _name: &str) -> Result<()> {
        unreachable!("non-Unix diagnostic directories cannot be opened")
    }

    #[cfg(unix)]
    fn require_named_identity(&self, name: &std::ffi::CString, expected: &File) -> Result<()> {
        use std::os::fd::{AsRawFd, FromRawFd};
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(projection_io_error(
                "identity_open",
                io::Error::last_os_error(),
            ));
        }
        let actual = unsafe { File::from_raw_fd(fd) };
        let expected = expected
            .metadata()
            .map_err(|error| projection_io_error("identity_expected", error))?;
        let actual = actual
            .metadata()
            .map_err(|error| projection_io_error("identity_actual", error))?;
        use std::os::unix::fs::MetadataExt;
        if !actual.is_file() || expected.dev() != actual.dev() || expected.ino() != actual.ino() {
            return Err(GatewayError::runtime_unavailable(
                "projection diagnostic staged file identity changed",
            ));
        }
        Ok(())
    }

    #[cfg(unix)]
    fn sync(&self) -> Result<()> {
        self.directory
            .sync_all()
            .map_err(|error| projection_io_error("publish_dir_sync", error))
    }
}

#[cfg(target_os = "linux")]
fn rename_noreplace(
    directory: &File,
    source: &std::ffi::CString,
    destination: &std::ffi::CString,
    action: &str,
) -> Result<()> {
    use std::os::fd::AsRawFd;
    if unsafe {
        libc::renameat2(
            directory.as_raw_fd(),
            source.as_ptr(),
            directory.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(projection_io_error(action, io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn rename_noreplace(
    directory: &File,
    source: &std::ffi::CString,
    destination: &std::ffi::CString,
    action: &str,
) -> Result<()> {
    use std::os::fd::AsRawFd;
    if unsafe {
        libc::renameatx_np(
            directory.as_raw_fd(),
            source.as_ptr(),
            directory.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_EXCL,
        )
    } != 0
    {
        return Err(projection_io_error(action, io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(all(
    unix,
    not(any(target_os = "linux", target_os = "macos", target_os = "ios"))
))]
fn rename_noreplace(
    _directory: &File,
    _source: &std::ffi::CString,
    _destination: &std::ffi::CString,
    _action: &str,
) -> Result<()> {
    Err(GatewayError::runtime_unavailable(
        "raw projection diagnostics require atomic no-replace rename support",
    ))
}

#[cfg(unix)]
fn open_diagnostic_directory(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| projection_io_error("open_root", error))
}

#[cfg(unix)]
fn normalize_diagnostic_creation_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(GatewayError::runtime_unavailable(
            "projection diagnostic path must not be empty",
        ));
    }
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(GatewayError::runtime_unavailable(
                "projection diagnostic directory must be a real directory",
            ));
        }
    }

    let mut existing = path;
    let mut missing = Vec::new();
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            GatewayError::runtime_unavailable(
                "projection diagnostic path has no existing filesystem ancestor",
            )
        })?;
        missing.push(name.to_os_string());
        existing = existing.parent().ok_or_else(|| {
            GatewayError::runtime_unavailable(
                "projection diagnostic path has no existing filesystem ancestor",
            )
        })?;
    }
    let mut normalized = std::fs::canonicalize(existing)
        .map_err(|error| projection_io_error("canonicalize_ancestor", error))?;
    for component in missing.into_iter().rev() {
        normalized.push(component);
    }
    Ok(normalized)
}

#[cfg(unix)]
fn diagnostic_component(name: &str) -> Result<std::ffi::CString> {
    if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\']) {
        return Err(GatewayError::runtime_unavailable(
            "projection diagnostic name must be one path component",
        ));
    }
    std::ffi::CString::new(name).map_err(|_| {
        GatewayError::runtime_unavailable("projection diagnostic name contains an interior NUL")
    })
}

fn projection_io_error(action: &str, error: io::Error) -> GatewayError {
    GatewayError::runtime_unavailable(format!("projection diagnostic {action} failed: {error}"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{Read, Write};
    use std::os::unix::fs::symlink;

    #[test]
    fn diagnostic_owner_directory_rejects_symlink_leaf() {
        let root = std::env::temp_dir().join(format!(
            "bm-gateway-audit-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let owner = root.join("owner");
        let alias = root.join("alias");
        fs::create_dir_all(&owner).expect("owner directory");
        symlink(&owner, &alias).expect("diagnostic symlink");

        let error = ProjectionDiagnosticDirectory::open(&alias)
            .expect_err("diagnostic directory symlink must fail closed");
        assert!(error.to_string().contains("must be a real directory"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn diagnostic_publish_never_replaces_existing_final_owner() {
        let root = unique_test_directory("noreplace");
        let owner = ProjectionDiagnosticDirectory::open(&root).expect("diagnostic owner");
        let transaction = owner.begin_transaction().expect("directory transaction");
        let mut first = transaction
            .create_new_file(".first.tmp")
            .expect("first staging");
        first.write_all(b"first").expect("first payload");
        first.sync_all().expect("first sync");
        transaction
            .publish(&first, ".first.tmp", "gateway-projection-final.json")
            .expect("first publish");

        let mut second = transaction
            .create_new_file(".second.tmp")
            .expect("second staging");
        second.write_all(b"second").expect("second payload");
        second.sync_all().expect("second sync");
        let error = transaction
            .publish(&second, ".second.tmp", "gateway-projection-final.json")
            .expect_err("second publish must not replace final owner");
        assert!(error.to_string().contains("publish"));
        let mut payload = String::new();
        fs::File::open(root.join("gateway-projection-final.json"))
            .expect("published final")
            .read_to_string(&mut payload)
            .expect("read final");
        assert_eq!(payload, "first");
        drop(transaction);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn diagnostic_directory_transaction_serializes_independent_owner_handles() {
        let root = unique_test_directory("lock");
        let first_owner = ProjectionDiagnosticDirectory::open(&root).expect("first owner");
        let first_transaction = first_owner.begin_transaction().expect("first transaction");
        let second_owner = ProjectionDiagnosticDirectory::open(&root).expect("second owner");
        let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let _second_transaction = second_owner
                .begin_transaction()
                .expect("second transaction");
            acquired_tx.send(()).expect("signal lock acquisition");
        });
        assert!(matches!(
            acquired_rx.recv_timeout(std::time::Duration::from_millis(100)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        drop(first_transaction);
        acquired_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("second transaction acquires after release");
        waiter.join().expect("lock waiter");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn diagnostic_retention_quarantines_same_identity_before_delete() {
        let root = unique_test_directory("retention");
        let owner = ProjectionDiagnosticDirectory::open(&root).expect("diagnostic owner");
        let transaction = owner.begin_transaction().expect("directory transaction");
        for (name, payload) in [
            ("gateway-projection-a.json", b"a".as_slice()),
            ("gateway-projection-b.json", b"b".as_slice()),
        ] {
            let staging_name = format!(".{name}.tmp");
            let mut staging = transaction
                .create_new_file(&staging_name)
                .expect("retention staging");
            staging.write_all(payload).expect("retention payload");
            staging.sync_all().expect("retention sync");
            transaction
                .publish(&staging, &staging_name, name)
                .expect("retention publish");
        }
        enforce_projection_diagnostic_retention(&transaction, 1).expect("retention");
        let names = transaction.entry_names().expect("entry names");
        assert_eq!(
            names
                .iter()
                .filter(|name| name.starts_with("gateway-projection-"))
                .count(),
            1
        );
        assert!(!names.iter().any(|name| name.starts_with(".quarantine.")));
        drop(transaction);
        fs::remove_dir_all(root).expect("cleanup");
    }

    fn unique_test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "bm-gateway-audit-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ))
    }
}
