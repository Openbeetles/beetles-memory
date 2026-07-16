use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::path::Component;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{OllamaTransparentConfig, OllamaTransparentError, Result};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublishedExecutableKind {
    ManagedUpstream,
    TransparentFront,
}

impl PublishedExecutableKind {
    fn directory_name(self) -> &'static str {
        match self {
            Self::ManagedUpstream => "upstream",
            Self::TransparentFront => "front",
        }
    }
}

#[cfg(test)]
pub(crate) fn test_sequence() -> u64 {
    TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableFileIdentity {
    pub sha256: String,
    pub byte_len: u64,
    pub device: u64,
    pub inode: u64,
    pub unix_mode: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRunnerReport {
    pub source_path: PathBuf,
    pub managed_path: PathBuf,
    pub source_exists: bool,
    pub managed_exists: bool,
    pub installed: bool,
    pub source_digest: Option<String>,
    pub managed_digest: Option<String>,
    pub copy_digest: Option<String>,
    pub execution_identity: Option<ExecutableFileIdentity>,
    pub message: Option<String>,
}

impl ManagedRunnerReport {
    pub fn installed(
        source_path: PathBuf,
        managed_path: PathBuf,
        copy_digest: Option<String>,
    ) -> Self {
        Self {
            source_path,
            managed_path,
            source_exists: true,
            managed_exists: true,
            installed: true,
            source_digest: copy_digest.clone(),
            managed_digest: copy_digest.clone(),
            copy_digest,
            execution_identity: None,
            message: None,
        }
    }
}

pub(crate) trait RunnerInstaller {
    fn inspect(&self, config: &OllamaTransparentConfig) -> Result<ManagedRunnerReport>;

    fn ensure_installed(&self, config: &OllamaTransparentConfig) -> Result<ManagedRunnerReport>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FileSystemRunnerInstaller;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PublishedExecutable {
    path: PathBuf,
    identity: ExecutableFileIdentity,
}

impl PublishedExecutable {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn identity(&self) -> &ExecutableFileIdentity {
        &self.identity
    }
}

impl RunnerInstaller for FileSystemRunnerInstaller {
    fn inspect(&self, config: &OllamaTransparentConfig) -> Result<ManagedRunnerReport> {
        config.validate()?;
        let mut source = match secure_open_optional(&config.official_ollama_binary)? {
            Some(source) => source,
            None => return report_from_open_files(config, None, None),
        };
        let source_identity = identity_for_file(&mut source)?;
        let managed_path = published_executable_path(
            config,
            PublishedExecutableKind::ManagedUpstream,
            &source_identity,
        )?;
        let managed = secure_open_optional(&managed_path)?;
        report_from_open_files_at(config, managed_path, Some(source), managed)
    }

    fn ensure_installed(&self, config: &OllamaTransparentConfig) -> Result<ManagedRunnerReport> {
        config.validate()?;
        let mut source =
            secure_open_optional(&config.official_ollama_binary)?.ok_or_else(|| {
                OllamaTransparentError::runner_install_failed(format!(
                    "official Ollama binary does not exist: {}",
                    config.official_ollama_binary.display()
                ))
            })?;
        let source_identity = identity_for_file(&mut source)?;

        let published = publish_open_file(
            config,
            PublishedExecutableKind::ManagedUpstream,
            &mut source,
            &source_identity,
        )?;
        report_from_identities(config, published.path, source_identity, published.identity)
    }
}

pub(crate) fn published_managed_runner(
    config: &OllamaTransparentConfig,
    report: &ManagedRunnerReport,
) -> Result<PublishedExecutable> {
    let expected = report.execution_identity.as_ref().ok_or_else(|| {
        OllamaTransparentError::runner_install_failed(
            "managed runner report has no retained execution identity",
        )
    })?;
    validate_published_executable(
        config,
        PublishedExecutableKind::ManagedUpstream,
        &report.managed_path,
        expected,
    )
}

pub(crate) fn publish_gateway_executable(
    config: &OllamaTransparentConfig,
    expected: &ExecutableFileIdentity,
) -> Result<PublishedExecutable> {
    publish_executable(
        config,
        PublishedExecutableKind::TransparentFront,
        &config.gateway_binary_path,
        expected,
    )
}

pub fn inspect_executable_identity(path: &Path) -> Result<ExecutableFileIdentity> {
    let mut file = secure_open_optional(path)?.ok_or_else(|| {
        OllamaTransparentError::runner_install_failed(format!(
            "executable does not exist: {}",
            path.display()
        ))
    })?;
    let identity = identity_for_file(&mut file)?;
    if identity.unix_mode & 0o111 == 0 {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "executable is not marked executable: {}",
            path.display()
        )));
    }
    Ok(identity)
}

pub(crate) fn publish_executable(
    config: &OllamaTransparentConfig,
    kind: PublishedExecutableKind,
    source_path: &Path,
    expected: &ExecutableFileIdentity,
) -> Result<PublishedExecutable> {
    let mut source = secure_open_optional(source_path)?.ok_or_else(|| {
        OllamaTransparentError::runner_install_failed(format!(
            "executable disappeared before publication: {}",
            source_path.display()
        ))
    })?;
    let actual = identity_for_file(&mut source)?;
    if &actual != expected {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "executable identity changed before immutable publication: {}",
            source_path.display()
        )));
    }
    publish_open_file(config, kind, &mut source, expected)
}

fn publish_open_file(
    config: &OllamaTransparentConfig,
    kind: PublishedExecutableKind,
    source: &mut File,
    expected: &ExecutableFileIdentity,
) -> Result<PublishedExecutable> {
    let path = published_executable_path(config, kind, expected)?;
    if let Some(existing) = secure_open_optional(&path)? {
        drop(existing);
        return validate_published_content(config, kind, &path, expected);
    }

    let parent_path = path.parent().ok_or_else(|| {
        OllamaTransparentError::runner_install_failed(
            "published executable path must have a digest directory",
        )
    })?;
    let parent = secure_open_directory(parent_path, true)?;
    set_directory_owner_only_mode(&parent, 0o700)?;
    let name = normal_file_name(&path)?;
    let temp_name = format!(
        ".program.{}.{}.tmp",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let mut temp = createat_new(&parent, &temp_name)?;
    let mut cleanup = TempEntry::new(&parent, temp_name.clone());
    source.seek(SeekFrom::Start(0)).map_err(|error| {
        OllamaTransparentError::runner_install_failed(format!(
            "failed to rewind executable for publication: {error}"
        ))
    })?;
    let copied_digest = copy_and_hash(source, &mut temp)?;
    if copied_digest != expected.sha256 {
        return Err(OllamaTransparentError::runner_install_failed(
            "executable changed while publishing its immutable content object",
        ));
    }
    set_executable_mode(&temp)?;
    temp.sync_all().map_err(|error| {
        OllamaTransparentError::runner_install_failed(format!(
            "failed to fsync published executable: {error}"
        ))
    })?;
    match linkat_no_clobber(&parent, &temp_name, &name) {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {}
        Err(error) => {
            return Err(OllamaTransparentError::runner_install_failed(format!(
                "failed to publish content-addressed executable: {error}"
            )));
        }
    }
    unlinkat_name(&parent, &temp_name).map_err(|error| {
        OllamaTransparentError::runner_install_failed(format!(
            "failed to remove executable publication staging file: {error}"
        ))
    })?;
    cleanup.disarm();
    parent.sync_all().map_err(|error| {
        OllamaTransparentError::runner_install_failed(format!(
            "failed to fsync executable digest directory: {error}"
        ))
    })?;
    set_directory_owner_only_mode(&parent, 0o500)?;
    validate_published_content(config, kind, &path, expected)
}

fn validate_published_content(
    config: &OllamaTransparentConfig,
    kind: PublishedExecutableKind,
    path: &Path,
    source_identity: &ExecutableFileIdentity,
) -> Result<PublishedExecutable> {
    let canonical = published_executable_path(config, kind, source_identity)?;
    if path != canonical {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "published executable path is not bound to its digest: {}",
            path.display()
        )));
    }
    let mut file = secure_open_optional(path)?.ok_or_else(|| {
        OllamaTransparentError::runner_install_failed(format!(
            "published executable disappeared: {}",
            path.display()
        ))
    })?;
    let actual = identity_for_file(&mut file)?;
    if actual.sha256 != source_identity.sha256 || actual.byte_len != source_identity.byte_len {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "published executable content differs from its digest owner: {}",
            path.display()
        )));
    }
    validate_immutable_mode(&file, path)?;
    validate_digest_owner(path)?;
    Ok(PublishedExecutable {
        path: path.to_path_buf(),
        identity: actual,
    })
}

fn published_executable_path(
    config: &OllamaTransparentConfig,
    kind: PublishedExecutableKind,
    identity: &ExecutableFileIdentity,
) -> Result<PathBuf> {
    let digest = identity.sha256.strip_prefix("sha256:").ok_or_else(|| {
        OllamaTransparentError::runner_install_failed(
            "executable identity must use a sha256 digest",
        )
    })?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(OllamaTransparentError::runner_install_failed(
            "executable identity has a non-canonical sha256 digest",
        ));
    }
    let owner = config.managed_runner_path.parent().ok_or_else(|| {
        OllamaTransparentError::runner_install_failed(
            "managed runner path must have a parent directory",
        )
    })?;
    Ok(owner
        .join("objects")
        .join(kind.directory_name())
        .join(digest)
        .join("program"))
}

pub(crate) fn validate_published_executable(
    config: &OllamaTransparentConfig,
    kind: PublishedExecutableKind,
    path: &Path,
    expected: &ExecutableFileIdentity,
) -> Result<PublishedExecutable> {
    let canonical = published_executable_path(config, kind, expected)?;
    if path != canonical {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "published executable path is not bound to its digest: {}",
            path.display()
        )));
    }
    let mut file = secure_open_optional(path)?.ok_or_else(|| {
        OllamaTransparentError::runner_install_failed(format!(
            "executable disappeared before launch: {}",
            path.display()
        ))
    })?;
    let actual = identity_for_file(&mut file)?;
    if &actual != expected {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "executable identity changed before launch: {}",
            path.display()
        )));
    }
    if actual.unix_mode & 0o111 == 0 {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "executable is not executable: {}",
            path.display()
        )));
    }
    validate_immutable_mode(&file, path)?;
    validate_digest_owner(path)?;
    Ok(PublishedExecutable {
        path: path.to_path_buf(),
        identity: actual,
    })
}

pub(crate) fn open_secure_lock_file(path: &Path) -> Result<File> {
    let parent_path = path.parent().ok_or_else(|| {
        OllamaTransparentError::runner_install_failed("lease path must have a parent directory")
    })?;
    let name = normal_file_name(path)?;
    let parent = secure_open_directory(parent_path, true)?;
    openat_lock_file(&parent, &name)
}

fn report_from_open_files(
    config: &OllamaTransparentConfig,
    source: Option<File>,
    managed: Option<File>,
) -> Result<ManagedRunnerReport> {
    report_from_open_files_at(config, config.managed_runner_path.clone(), source, managed)
}

fn report_from_open_files_at(
    config: &OllamaTransparentConfig,
    managed_path: PathBuf,
    mut source: Option<File>,
    mut managed: Option<File>,
) -> Result<ManagedRunnerReport> {
    let source_identity = source.as_mut().map(identity_for_file).transpose()?;
    let managed_identity = managed.as_mut().map(identity_for_file).transpose()?;
    let installed = source_identity
        .as_ref()
        .zip(managed_identity.as_ref())
        .is_some_and(|(source, managed)| {
            source.sha256 == managed.sha256
                && managed.unix_mode & 0o100 != 0
                && managed.unix_mode & 0o222 == 0
        });
    if installed {
        validate_immutable_mode(
            managed.as_ref().expect("installed managed executable"),
            &managed_path,
        )?;
        validate_digest_owner(&managed_path)?;
    }
    Ok(ManagedRunnerReport {
        source_path: config.official_ollama_binary.clone(),
        managed_path,
        source_exists: source_identity.is_some(),
        managed_exists: managed_identity.is_some(),
        installed,
        source_digest: source_identity
            .as_ref()
            .map(|identity| identity.sha256.clone()),
        managed_digest: managed_identity
            .as_ref()
            .map(|identity| identity.sha256.clone()),
        copy_digest: managed_identity
            .as_ref()
            .map(|identity| identity.sha256.clone()),
        execution_identity: installed.then_some(managed_identity).flatten(),
        message: (!installed).then(|| {
            "managed runner is missing or differs from official Ollama binary".to_string()
        }),
    })
}

fn report_from_identities(
    config: &OllamaTransparentConfig,
    managed_path: PathBuf,
    source: ExecutableFileIdentity,
    managed: ExecutableFileIdentity,
) -> Result<ManagedRunnerReport> {
    if source.sha256 != managed.sha256 {
        return Err(OllamaTransparentError::runner_install_failed(
            "managed runner content differs from official Ollama binary",
        ));
    }
    Ok(ManagedRunnerReport {
        source_path: config.official_ollama_binary.clone(),
        managed_path,
        source_exists: true,
        managed_exists: true,
        installed: true,
        source_digest: Some(source.sha256.clone()),
        managed_digest: Some(managed.sha256.clone()),
        copy_digest: Some(managed.sha256.clone()),
        execution_identity: Some(managed),
        message: None,
    })
}

fn identity_for_file(file: &mut File) -> Result<ExecutableFileIdentity> {
    let metadata = file.metadata().map_err(|error| {
        OllamaTransparentError::runner_install_failed(format!(
            "failed to inspect runner file identity: {error}"
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(OllamaTransparentError::runner_install_failed(
            "runner path must identify a regular file",
        ));
    }
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        OllamaTransparentError::runner_install_failed(format!("failed to rewind runner: {error}"))
    })?;
    let sha256 = hash_reader(file)?;
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        OllamaTransparentError::runner_install_failed(format!("failed to rewind runner: {error}"))
    })?;
    let (device, inode, unix_mode) = platform_file_identity(&metadata);
    Ok(ExecutableFileIdentity {
        sha256,
        byte_len: metadata.len(),
        device,
        inode,
        unix_mode,
    })
}

fn hash_reader(reader: &mut impl Read) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            OllamaTransparentError::runner_install_failed(format!(
                "failed to read runner for SHA-256 identity: {error}"
            ))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn copy_and_hash(source: &mut File, destination: &mut File) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source.read(&mut buffer).map_err(|error| {
            OllamaTransparentError::runner_install_failed(format!(
                "failed to read official Ollama binary: {error}"
            ))
        })?;
        if read == 0 {
            break;
        }
        destination.write_all(&buffer[..read]).map_err(|error| {
            OllamaTransparentError::runner_install_failed(format!(
                "failed to write managed runner temporary file: {error}"
            ))
        })?;
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[cfg(unix)]
fn platform_file_identity(metadata: &std::fs::Metadata) -> (u64, u64, u32) {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino(), metadata.mode())
}

#[cfg(not(unix))]
fn platform_file_identity(_metadata: &std::fs::Metadata) -> (u64, u64, u32) {
    (0, 0, 0)
}

pub(crate) fn normal_file_name(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            OllamaTransparentError::runner_install_failed("runner path must have a UTF-8 file name")
        })?;
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(OllamaTransparentError::runner_install_failed(
            "runner file name is not a normal path component",
        ));
    }
    Ok(name.to_string())
}

#[cfg(unix)]
fn secure_open_optional(path: &Path) -> Result<Option<File>> {
    let parent = path.parent().ok_or_else(|| {
        OllamaTransparentError::runner_install_failed("runner path must have a parent directory")
    })?;
    let name = normal_file_name(path)?;
    match secure_open_directory(parent, false) {
        Ok(directory) => openat_optional(&directory, &name),
        Err(error) if error.message().contains("does not exist") => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
fn secure_open_optional(_path: &Path) -> Result<Option<File>> {
    Err(OllamaTransparentError::unsupported(
        "secure managed runner installation is currently available only on Unix hosts",
    ))
}

#[cfg(unix)]
pub(crate) fn secure_open_directory(path: &Path, create: bool) -> Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};

    let mut current = if path.is_absolute() {
        open_directory_path(Path::new("/"))?
    } else {
        open_directory_path(Path::new("."))?
    };
    for component in path.components() {
        let Component::Normal(name) = component else {
            if matches!(component, Component::RootDir | Component::CurDir) {
                continue;
            }
            return Err(OllamaTransparentError::runner_install_failed(
                "runner directory must not contain parent or platform-prefix components",
            ));
        };
        let name = CString::new(name.as_encoded_bytes()).map_err(|_| {
            OllamaTransparentError::runner_install_failed(
                "runner directory component contains an interior NUL",
            )
        })?;
        let fd = unsafe {
            libc::openat(
                current.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        let fd = if fd >= 0 {
            fd
        } else {
            let open_error = std::io::Error::last_os_error();
            if create && open_error.raw_os_error() == Some(libc::ENOENT) {
                let mkdir = unsafe { libc::mkdirat(current.as_raw_fd(), name.as_ptr(), 0o700) };
                if mkdir != 0
                    && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
                {
                    return Err(OllamaTransparentError::runner_install_failed(format!(
                        "failed to create retained runner directory: {}",
                        std::io::Error::last_os_error()
                    )));
                }
                let reopened = unsafe {
                    libc::openat(
                        current.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if reopened < 0 {
                    return Err(OllamaTransparentError::runner_install_failed(format!(
                        "failed to retain newly created runner directory: {}",
                        std::io::Error::last_os_error()
                    )));
                }
                reopened
            } else if open_error.raw_os_error() == Some(libc::ENOENT) {
                return Err(OllamaTransparentError::runner_install_failed(format!(
                    "runner directory does not exist: {}",
                    path.display()
                )));
            } else {
                return Err(OllamaTransparentError::runner_install_failed(format!(
                    "failed to open runner directory without following symlinks: {open_error}"
                )));
            }
        };
        current = unsafe { File::from_raw_fd(fd) };
    }
    Ok(current)
}

#[cfg(not(unix))]
pub(crate) fn secure_open_directory(_path: &Path, _create: bool) -> Result<File> {
    Err(OllamaTransparentError::unsupported(
        "secure managed runner directories are currently available only on Unix hosts",
    ))
}

#[cfg(unix)]
fn open_directory_path(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| {
            OllamaTransparentError::runner_install_failed(format!(
                "failed to open runner directory {}: {error}",
                path.display()
            ))
        })
}

#[cfg(unix)]
fn openat_optional(parent: &File, name: &str) -> Result<Option<File>> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = CString::new(name).expect("validated file name");
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd >= 0 {
        Ok(Some(unsafe { File::from_raw_fd(fd) }))
    } else {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOENT) {
            Ok(None)
        } else {
            Err(OllamaTransparentError::runner_install_failed(format!(
                "failed to open runner without following symlinks: {error}"
            )))
        }
    }
}

#[cfg(not(unix))]
fn openat_optional(_parent: &File, _name: &str) -> Result<Option<File>> {
    Err(OllamaTransparentError::unsupported(
        "secure openat is currently available only on Unix hosts",
    ))
}

#[cfg(unix)]
fn createat_new(parent: &File, name: &str) -> Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = CString::new(name).expect("generated temporary name");
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o700,
        )
    };
    if fd < 0 {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "failed to create managed runner temporary file: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn openat_lock_file(parent: &File, name: &str) -> Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = CString::new(name).expect("validated lease file name");
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "failed to open transition lease without following symlinks: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(not(unix))]
fn openat_lock_file(_parent: &File, _name: &str) -> Result<File> {
    Err(OllamaTransparentError::unsupported(
        "OS transition leases are currently available only on Unix hosts",
    ))
}

#[cfg(not(unix))]
fn createat_new(_parent: &File, _name: &str) -> Result<File> {
    Err(OllamaTransparentError::unsupported(
        "secure openat is currently available only on Unix hosts",
    ))
}

#[cfg(unix)]
fn set_executable_mode(file: &File) -> Result<()> {
    use std::os::fd::AsRawFd;
    if unsafe { libc::fchmod(file.as_raw_fd(), 0o500) } != 0 {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "failed to set managed runner executable mode: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn set_directory_owner_only_mode(directory: &File, mode: libc::mode_t) -> Result<()> {
    use std::os::fd::AsRawFd;
    if unsafe { libc::fchmod(directory.as_raw_fd(), mode) } != 0 {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "failed to seal executable digest directory: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_directory_owner_only_mode(_directory: &File, _mode: libc::mode_t) -> Result<()> {
    Err(OllamaTransparentError::unsupported(
        "content-addressed executable owners require Unix directory permissions",
    ))
}

#[cfg(unix)]
fn validate_immutable_mode(file: &File, path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().map_err(|error| {
        OllamaTransparentError::runner_install_failed(format!(
            "failed to inspect published executable permissions: {error}"
        ))
    })?;
    if metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o222 != 0
        || metadata.mode() & 0o100 == 0
    {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "published executable is not an owner-controlled immutable executable: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn validate_digest_owner(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let directory_path = path.parent().ok_or_else(|| {
        OllamaTransparentError::runner_install_failed(
            "published executable must have a digest directory",
        )
    })?;
    let directory = secure_open_directory(directory_path, false)?;
    let metadata = directory.metadata().map_err(|error| {
        OllamaTransparentError::runner_install_failed(format!(
            "failed to inspect executable digest owner: {error}"
        ))
    })?;
    if metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o222 != 0
        || metadata.mode() & 0o100 == 0
    {
        return Err(OllamaTransparentError::runner_install_failed(format!(
            "published executable digest directory is not immutable and owner-controlled: {}",
            directory_path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_digest_owner(_path: &Path) -> Result<()> {
    Err(OllamaTransparentError::unsupported(
        "content-addressed executable owners require Unix directory permissions",
    ))
}

#[cfg(not(unix))]
fn validate_immutable_mode(_file: &File, _path: &Path) -> Result<()> {
    Err(OllamaTransparentError::unsupported(
        "content-addressed executable owners require Unix file permissions",
    ))
}

#[cfg(not(unix))]
fn set_executable_mode(_file: &File) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn linkat_no_clobber(parent: &File, source: &str, destination: &str) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    let source = CString::new(source).expect("generated temporary name");
    let destination = CString::new(destination).expect("validated destination name");
    let result = unsafe {
        libc::linkat(
            parent.as_raw_fd(),
            source.as_ptr(),
            parent.as_raw_fd(),
            destination.as_ptr(),
            0,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn linkat_no_clobber(_parent: &File, _source: &str, _destination: &str) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "linkat is unavailable",
    ))
}

#[cfg(unix)]
fn unlinkat_name(parent: &File, name: &str) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    let name = CString::new(name).expect("validated file name");
    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn unlinkat_name(_parent: &File, _name: &str) -> std::io::Result<()> {
    Ok(())
}

struct TempEntry<'a> {
    parent: &'a File,
    name: String,
    armed: bool,
}

impl<'a> TempEntry<'a> {
    fn new(parent: &'a File, name: String) -> Self {
        Self {
            parent,
            name,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempEntry<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = unlinkat_name(self.parent, &self.name);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn install_uses_sha256_and_rejects_different_existing_content() {
        let root = TestRoot::new("different-content");
        let config = test_config(&root.path);
        fs::create_dir_all(
            config
                .official_ollama_binary
                .parent()
                .expect("source parent"),
        )
        .expect("source parent");
        fs::write(&config.official_ollama_binary, b"official-v1").expect("source");
        let source_identity = identity_for_file(
            &mut File::open(&config.official_ollama_binary).expect("source file"),
        )
        .expect("source identity");
        let published_path = published_executable_path(
            &config,
            PublishedExecutableKind::ManagedUpstream,
            &source_identity,
        )
        .expect("published path");
        fs::create_dir_all(published_path.parent().expect("digest parent")).expect("digest parent");
        fs::write(&published_path, b"untrusted-existing").expect("managed");

        let error = FileSystemRunnerInstaller
            .ensure_installed(&config)
            .expect_err("different content must not be overwritten");

        assert!(error.message().contains("differs from its digest owner"));
        assert_eq!(
            fs::read(&published_path).expect("managed content"),
            b"untrusted-existing"
        );
    }

    #[test]
    fn install_rejects_destination_symlink_without_touching_target() {
        let root = TestRoot::new("destination-symlink");
        let config = test_config(&root.path);
        fs::create_dir_all(
            config
                .official_ollama_binary
                .parent()
                .expect("source parent"),
        )
        .expect("source parent");
        fs::write(&config.official_ollama_binary, b"official-v1").expect("source");
        let source_identity = identity_for_file(
            &mut File::open(&config.official_ollama_binary).expect("source file"),
        )
        .expect("source identity");
        let published_path = published_executable_path(
            &config,
            PublishedExecutableKind::ManagedUpstream,
            &source_identity,
        )
        .expect("published path");
        fs::create_dir_all(published_path.parent().expect("digest parent")).expect("digest parent");
        let victim = root.path.join("victim");
        fs::write(&victim, b"victim").expect("victim");
        symlink(&victim, &published_path).expect("destination symlink");

        let error = FileSystemRunnerInstaller
            .ensure_installed(&config)
            .expect_err("destination symlink must fail closed");

        assert!(error.message().contains("without following symlinks"));
        assert_eq!(fs::read(victim).expect("victim content"), b"victim");
    }

    #[test]
    fn install_rejects_symlink_in_managed_parent_chain() {
        let root = TestRoot::new("parent-symlink");
        let mut config = test_config(&root.path);
        fs::create_dir_all(
            config
                .official_ollama_binary
                .parent()
                .expect("source parent"),
        )
        .expect("source parent");
        fs::write(&config.official_ollama_binary, b"official-v1").expect("source");
        let actual = root.path.join("actual-parent");
        fs::create_dir_all(&actual).expect("actual parent");
        let linked = root.path.join("linked-parent");
        symlink(&actual, &linked).expect("parent symlink");
        config.managed_runner_path = linked.join("bm-real-ollama");

        let error = FileSystemRunnerInstaller
            .ensure_installed(&config)
            .expect_err("parent symlink must fail closed");

        assert!(error.message().contains("without following symlinks"));
        assert!(!actual.join("bm-real-ollama").exists());
    }

    #[test]
    fn retained_parent_publication_is_not_redirected_by_parent_swap() {
        let root = TestRoot::new("parent-swap");
        let original = root.path.join("managed");
        let retained = root.path.join("retained");
        let attacker = root.path.join("attacker");
        fs::create_dir_all(&original).expect("original parent");
        fs::create_dir_all(&attacker).expect("attacker parent");
        let parent = secure_open_directory(&original, false).expect("retained parent");
        fs::rename(&original, &retained).expect("move original parent");
        symlink(&attacker, &original).expect("replace parent with symlink");

        let mut temp = createat_new(&parent, ".temp").expect("retained temp");
        temp.write_all(b"runner").expect("temp content");
        temp.sync_all().expect("temp fsync");
        linkat_no_clobber(&parent, ".temp", "bm-real-ollama").expect("retained publish");
        unlinkat_name(&parent, ".temp").expect("temp cleanup");

        assert_eq!(
            fs::read(retained.join("bm-real-ollama")).expect("retained publication"),
            b"runner"
        );
        assert!(!attacker.join("bm-real-ollama").exists());
    }

    #[test]
    fn published_runner_rejects_content_object_replacement() {
        let root = TestRoot::new("execution-revalidation");
        let config = test_config(&root.path);
        fs::create_dir_all(
            config
                .official_ollama_binary
                .parent()
                .expect("source parent"),
        )
        .expect("source parent");
        fs::write(&config.official_ollama_binary, b"official-v1").expect("source");
        let report = FileSystemRunnerInstaller
            .ensure_installed(&config)
            .expect("install report");
        let digest_dir = report.managed_path.parent().expect("digest directory");
        let mut directory_permissions = fs::metadata(digest_dir)
            .expect("digest metadata")
            .permissions();
        use std::os::unix::fs::PermissionsExt;
        directory_permissions.set_mode(0o700);
        fs::set_permissions(digest_dir, directory_permissions).expect("unseal fixture directory");
        fs::remove_file(&report.managed_path).expect("remove published runner");
        fs::write(&report.managed_path, b"replaced").expect("replacement");
        let mut permissions = fs::metadata(&report.managed_path)
            .expect("replacement metadata")
            .permissions();
        permissions.set_mode(0o500);
        fs::set_permissions(&report.managed_path, permissions).expect("replacement mode");

        let error = published_managed_runner(&config, &report)
            .expect_err("replacement must fail execution revalidation");

        assert!(error.message().contains("identity changed before launch"));
    }

    #[test]
    fn gateway_execution_rejects_symbolic_link_path() {
        let root = TestRoot::new("gateway-symlink");
        let gateway = root.path.join("bm-llm-gateway");
        symlink(std::env::current_exe().expect("test executable"), &gateway)
            .expect("gateway symlink");

        let error =
            inspect_executable_identity(&gateway).expect_err("gateway symlink must fail closed");

        assert!(error.message().contains("without following symlinks"));
    }

    #[test]
    fn gateway_execution_rejects_path_replacement_after_identity_capture() {
        let root = TestRoot::new("gateway-replacement");
        let gateway = root.path.join("bm-llm-gateway");
        fs::copy(std::env::current_exe().expect("test executable"), &gateway)
            .expect("gateway fixture");
        let expected = inspect_executable_identity(&gateway).expect("gateway identity");
        fs::remove_file(&gateway).expect("remove verified gateway");
        fs::write(&gateway, b"replacement executable").expect("replacement gateway");
        let mut permissions = fs::metadata(&gateway)
            .expect("replacement metadata")
            .permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(&gateway, permissions).expect("replacement mode");

        let mut config = test_config(&root.path);
        config.gateway_binary_path = gateway.clone();
        let error = publish_gateway_executable(&config, &expected)
            .expect_err("gateway replacement must fail closed");

        assert!(error
            .message()
            .contains("identity changed before immutable publication"));
    }

    fn test_config(root: &Path) -> OllamaTransparentConfig {
        let data_dir = root.join("data");
        let authority = crate::OllamaTransparentMemoryAuthority::new(
            "test-owner",
            "test-agent",
            "test-channel",
            data_dir.join("store"),
        )
        .expect("test memory authority");
        let mut config = OllamaTransparentConfig::new(
            &data_dir,
            std::env::current_exe().expect("test executable"),
            authority,
        )
        .expect("test config");
        config.official_ollama_binary = root.join("source").join("ollama");
        config.managed_runner_path = root.join("managed").join("bm-real-ollama");
        config
    }

    struct TestRoot {
        path: PathBuf,
    }

    impl TestRoot {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "bm-ollama-transparent-{label}-{}-{}",
                std::process::id(),
                TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("test root");
            Self {
                path: fs::canonicalize(path).expect("canonical test root"),
            }
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
