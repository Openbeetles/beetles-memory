use std::{
    fmt, fs, io,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
};

use sha2::{Digest, Sha256};
#[cfg(target_os = "linux")]
use std::sync::Mutex;

#[cfg(target_os = "linux")]
const P7_RETAINED_EXECUTABLE_FD_ENV: &str = "BM_P7_RETAINED_EXECUTABLE_FD";
#[cfg(target_os = "linux")]
const P7_RETAINED_EXECUTABLE_PATH_ENV: &str = "BM_P7_RETAINED_EXECUTABLE_PATH";
#[cfg(target_os = "linux")]
const P7_RETAINED_EXECUTABLE_SHA256_ENV: &str = "BM_P7_RETAINED_EXECUTABLE_SHA256";
#[cfg(target_os = "linux")]
const P7_RETAINED_EXECUTABLE_AUTHORITY_ENV: [&str; 3] = [
    P7_RETAINED_EXECUTABLE_FD_ENV,
    P7_RETAINED_EXECUTABLE_PATH_ENV,
    P7_RETAINED_EXECUTABLE_SHA256_ENV,
];
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum P7ExecutionAuthorityClaimState {
    Available,
    Consumed,
}

#[cfg(target_os = "linux")]
static P7_EXECUTION_AUTHORITY_CLAIM_STATE: Mutex<P7ExecutionAuthorityClaimState> =
    Mutex::new(P7ExecutionAuthorityClaimState::Available);

#[cfg(target_os = "linux")]
struct P7InheritedAuthorityClaim {
    inherited_fd: Option<i32>,
}

#[cfg(target_os = "linux")]
impl P7InheritedAuthorityClaim {
    fn new() -> Self {
        Self { inherited_fd: None }
    }

    fn retain_inherited_fd(&mut self, inherited_fd: i32) {
        self.inherited_fd = Some(inherited_fd);
    }

    fn revoke(mut self) -> io::Result<()> {
        let close_error = self.inherited_fd.take().and_then(|fd| {
            // SAFETY: fd is the launcher-issued inherited descriptor and has no Rust owner.
            (unsafe { libc::close(fd) } != 0).then(io::Error::last_os_error)
        });
        clear_p7_inherited_authority_environment();
        close_error.map_or(Ok(()), Err)
    }
}

#[cfg(target_os = "linux")]
impl Drop for P7InheritedAuthorityClaim {
    fn drop(&mut self) {
        if let Some(fd) = self.inherited_fd.take() {
            // SAFETY: fd is the launcher-issued inherited descriptor and has no Rust owner.
            unsafe {
                libc::close(fd);
            }
        }
        clear_p7_inherited_authority_environment();
    }
}

#[cfg(target_os = "linux")]
fn clear_p7_inherited_authority_environment() {
    for key in P7_RETAINED_EXECUTABLE_AUTHORITY_ENV {
        std::env::remove_var(key);
    }
}

#[cfg(target_os = "linux")]
fn is_p7_retained_executable_authority_env(name: &std::ffi::OsStr) -> bool {
    P7_RETAINED_EXECUTABLE_AUTHORITY_ENV
        .iter()
        .any(|key| name == std::ffi::OsStr::new(key))
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn validate_run_id(run_id: &str) -> io::Result<()> {
    if run_id.is_empty()
        || matches!(run_id, "." | "..")
        || !run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid_input("P7 run_id must match ASCII [A-Za-z0-9._-]+"));
    }
    Ok(())
}

fn validate_component(component: &str) -> io::Result<()> {
    if component.is_empty()
        || matches!(component, "." | "..")
        || component
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\' | b':' | 0))
    {
        return Err(invalid_input("P7 secure path component is invalid"));
    }
    Ok(())
}

fn validate_staged_component(component: &str) -> io::Result<()> {
    validate_component(component)?;
    if !component.starts_with('.') && !component.contains(".tmp") {
        return Err(invalid_input(
            "P7 staged artifact name must be hidden or contain .tmp",
        ));
    }
    Ok(())
}

fn validate_canonical_root(root: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid_input("P7 benchmark root must be a real directory"));
    }
    if fs::canonicalize(root)? != root {
        return Err(invalid_input("P7 benchmark root must be canonical"));
    }
    Ok(())
}

pub struct P7RetainedDirectoryOwner {
    path: PathBuf,
    directory: platform::DirectoryHandle,
}

impl P7RetainedDirectoryOwner {
    pub fn open_root(path: &Path) -> io::Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            directory: platform::DirectoryHandle::open_root(path)?,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn open_directory(&self, component: &str) -> io::Result<Self> {
        validate_component(component)?;
        Ok(Self {
            path: self.path.join(component),
            directory: self.directory.open_directory(component)?,
        })
    }

    pub fn open_existing_file(&self, file_name: &str) -> io::Result<fs::File> {
        validate_component(file_name)?;
        self.directory.open_existing_file(file_name)
    }

    pub fn open_existing_executable(&self, file_name: &str) -> io::Result<fs::File> {
        validate_component(file_name)?;
        self.directory.open_existing_executable(file_name)
    }

    pub fn try_open_existing_file(&self, file_name: &str) -> io::Result<Option<fs::File>> {
        match self.open_existing_file(file_name) {
            Ok(file) => Ok(Some(file)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn verify_unchanged(&self) -> io::Result<()> {
        self.directory.verify_path(&self.path)
    }

    pub fn verify_file_identity(&self, file_name: &str, file: &fs::File) -> io::Result<()> {
        validate_component(file_name)?;
        self.directory.verify_file(file_name, file)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P7ContentIdentity {
    pub byte_len: u64,
    pub sha256: String,
}

pub struct P7RetainedFile {
    path: PathBuf,
    file_name: String,
    file: fs::File,
    owner: P7RetainedDirectoryOwner,
    admitted_len: u64,
}

struct P7InheritedExecutionAuthority {
    file: fs::File,
    expected_sha256: String,
    locator: PathBuf,
}

pub struct P7ProcessExecutionAuthority {
    inherited_execution: P7InheritedExecutionAuthority,
    release_executable: P7RetainedFile,
    execution_identity: P7ContentIdentity,
}

pub(crate) trait P7ExternalWriteAuthority {
    fn verify_external_write_authority(&mut self) -> io::Result<()>;
    fn process_execution_authority(&mut self) -> &mut P7ProcessExecutionAuthority;
}

#[cfg(target_os = "linux")]
struct P7ExecPointerArray(Vec<*const libc::c_char>);

#[cfg(target_os = "linux")]
impl P7ExecPointerArray {
    fn as_ptr(&self) -> *const *const libc::c_char {
        self.0.as_ptr()
    }
}

// SAFETY: the pointers refer only to immutable CString buffers captured by the same pre_exec
// closure. Those buffers outlive the pointer arrays and never move or mutate before fexecve.
#[cfg(target_os = "linux")]
unsafe impl Send for P7ExecPointerArray {}
// SAFETY: see the Send implementation; the closure only reads the pointer values.
#[cfg(target_os = "linux")]
unsafe impl Sync for P7ExecPointerArray {}

impl P7InheritedExecutionAuthority {
    fn locator(&self) -> &Path {
        &self.locator
    }

    fn copy_and_verify(&mut self, destination: &mut dyn Write) -> io::Result<P7ContentIdentity> {
        let admitted_len = self.file.metadata()?.len();
        let limit = admitted_len
            .checked_add(1)
            .ok_or_else(|| invalid_data("P7 inherited executable read limit overflow"))?;
        self.file.seek(SeekFrom::Start(0))?;
        let mut reader = HashingReader::new((&mut self.file).take(limit));
        io::copy(&mut reader, destination)?;
        let identity = reader.finish();
        self.file.seek(SeekFrom::Start(0))?;
        if identity.byte_len != admitted_len || identity.sha256 != self.expected_sha256 {
            return Err(invalid_data(
                "P7 inherited executable differs from its sealed identity",
            ));
        }
        Ok(identity)
    }

    fn verify(&mut self) -> io::Result<P7ContentIdentity> {
        self.copy_and_verify(&mut io::sink())
    }
}

impl P7ProcessExecutionAuthority {
    pub fn claim() -> io::Result<Self> {
        let mut inherited_execution = P7RetainedFile::inherited_execution_authority()?;
        let locator = inherited_execution.locator().to_path_buf();
        let execution_identity = inherited_execution.verify()?;
        let mut release_executable = P7RetainedFile::open_executable(&locator)?;
        release_executable.verify_content(&execution_identity)?;
        Ok(Self {
            inherited_execution,
            release_executable,
            execution_identity,
        })
    }

    pub fn locator(&self) -> &Path {
        self.release_executable.path()
    }

    pub fn execution_identity(&self) -> &P7ContentIdentity {
        &self.execution_identity
    }

    pub fn verify_retained(&mut self) -> io::Result<()> {
        let current_execution = self.inherited_execution.verify()?;
        if current_execution != self.execution_identity {
            return Err(invalid_data(
                "P7 sealed execution identity changed after admission",
            ));
        }
        self.release_executable
            .verify_content(&self.execution_identity)
    }

    pub(crate) fn copy_execution_to(
        &mut self,
        destination: &mut dyn Write,
    ) -> io::Result<P7ContentIdentity> {
        let copied = self.inherited_execution.copy_and_verify(destination)?;
        if copied != self.execution_identity {
            return Err(invalid_data(
                "P7 sealed execution identity changed while being copied",
            ));
        }
        self.release_executable
            .verify_content(&self.execution_identity)?;
        Ok(copied)
    }

    pub fn initialize_cohort<'authority>(
        &'authority mut self,
        root: &Path,
        run_id: &str,
    ) -> io::Result<P7AuthorityBoundArtifactTransaction<'authority>> {
        let owner = initialize_p7_cohort_with_authority(self, root, run_id)?;
        Ok(P7AuthorityBoundArtifactTransaction::new(self, owner))
    }

    pub fn open_cohort<'authority>(
        &'authority mut self,
        root: &Path,
        run_id: &str,
    ) -> io::Result<P7AuthorityBoundArtifactTransaction<'authority>> {
        let owner = open_p7_cohort_artifact_owner_with_authority(self, root, run_id)?;
        Ok(P7AuthorityBoundArtifactTransaction::new(self, owner))
    }

    pub fn begin_runner_release<'authority>(
        &'authority mut self,
        root: &Path,
    ) -> io::Result<P7AuthorityBoundReleaseTransaction<'authority>> {
        let releases = open_or_create_p7_release_store_with_authority(self, root)?;
        P7AuthorityBoundReleaseTransaction::new(self, releases, ".staging-pending", None)
    }

    pub fn begin_runner_authority_probe<'authority>(
        &'authority mut self,
        root: &Path,
    ) -> io::Result<P7AuthorityBoundReleaseTransaction<'authority>> {
        let releases = open_or_create_p7_runner_authority_probe_store_with_authority(self, root)?;
        P7AuthorityBoundReleaseTransaction::new(self, releases, ".staging-pending", None)
    }

    pub fn begin_verifier_release<'authority>(
        &'authority mut self,
        root: &Path,
    ) -> io::Result<P7AuthorityBoundReleaseTransaction<'authority>> {
        let releases = open_or_create_p7_verifier_release_store_with_authority(self, root)?;
        P7AuthorityBoundReleaseTransaction::new(
            self,
            releases,
            ".staging-verifier",
            Some(".verifier-release-publish.lock"),
        )
    }
}

impl P7ExternalWriteAuthority for P7ProcessExecutionAuthority {
    fn verify_external_write_authority(&mut self) -> io::Result<()> {
        self.verify_retained()
    }

    fn process_execution_authority(&mut self) -> &mut P7ProcessExecutionAuthority {
        self
    }
}

#[cfg(target_os = "linux")]
fn require_p7_execution_seals(actual_seals: libc::c_int) -> io::Result<()> {
    let required_seals = p7_required_execution_seals();
    if actual_seals & required_seals != required_seals {
        return Err(invalid_data(
            "P7 retained executable descriptor is missing required seals",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn p7_required_execution_seals() -> libc::c_int {
    libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL
}

impl P7RetainedFile {
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::open_with(path, false)
    }

    pub fn open_executable(path: &Path) -> io::Result<Self> {
        Self::open_with(path, true)
    }

    fn open_with(path: &Path, executable: bool) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(invalid_input("P7 retained file path must be absolute"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| invalid_input("P7 retained file has no parent directory"))?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid_input("P7 retained file name must be valid UTF-8"))?
            .to_string();
        validate_component(&file_name)?;
        let owner = P7RetainedDirectoryOwner::open_root(parent)?;
        let file = owner.open_existing_file(&file_name)?;
        if executable {
            let executable_file = owner.open_existing_executable(&file_name)?;
            owner.verify_file_identity(&file_name, &executable_file)?;
        }
        let admitted_len = file.metadata()?.len();
        owner.verify_file_identity(&file_name, &file)?;
        Ok(Self {
            path: path.to_path_buf(),
            file_name,
            file,
            owner,
            admitted_len,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn admitted_len(&self) -> u64 {
        self.admitted_len
    }

    pub fn metadata(&self) -> io::Result<fs::Metadata> {
        self.file.metadata()
    }

    pub fn verify_unchanged(&self) -> io::Result<()> {
        self.owner
            .verify_file_identity(&self.file_name, &self.file)?;
        if self.file.metadata()?.len() != self.admitted_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "P7 retained file changed length",
            ));
        }
        self.owner.verify_unchanged()
    }

    pub fn verify_content(&mut self, expected: &P7ContentIdentity) -> io::Result<()> {
        let actual = self.hash_for_launch()?;
        if &actual != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "P7 retained file content changed after admission",
            ));
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn executable_command(
        &mut self,
        args: &[String],
    ) -> io::Result<(Command, P7ExecutableLaunchGuard, P7ContentIdentity)> {
        self.linux_executable_command_with_seals(args, p7_required_execution_seals())
    }

    #[cfg(all(test, target_os = "linux"))]
    fn executable_command_with_test_seals(
        &mut self,
        args: &[String],
        seals: libc::c_int,
    ) -> io::Result<(Command, P7ExecutableLaunchGuard, P7ContentIdentity)> {
        self.linux_executable_command_with_seals(args, seals)
    }

    #[cfg(target_os = "linux")]
    fn linux_executable_command_with_seals(
        &mut self,
        args: &[String],
        seals: libc::c_int,
    ) -> io::Result<(Command, P7ExecutableLaunchGuard, P7ContentIdentity)> {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::process::CommandExt;

        let name = CString::new("bm-p7-sealed-executable").expect("static memfd name");
        // SAFETY: name is a valid C string and the returned descriptor is uniquely owned.
        let raw = unsafe {
            libc::syscall(
                libc::SYS_memfd_create,
                name.as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
            ) as libc::c_int
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful memfd_create returned a new owned descriptor.
        let memfd = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut target = fs::File::from(memfd);
        let mut source = self.file.try_clone()?;
        source.seek(SeekFrom::Start(0))?;
        let mut reader = HashingReader::new(source.take(self.admitted_len.saturating_add(1)));
        io::copy(&mut reader, &mut target)?;
        let launch_identity = reader.finish();
        if launch_identity.byte_len != self.admitted_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "P7 executable changed while sealing Linux memfd",
            ));
        }
        target.sync_all()?;
        // SAFETY: target is a live anonymous regular file descriptor owned by this process.
        if unsafe { libc::fchmod(target.as_raw_fd(), 0o500) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: F_ADD_SEALS applies the requested kernel-enforced restrictions to the live memfd.
        if unsafe { libc::fcntl(target.as_raw_fd(), libc::F_ADD_SEALS, seals) } != 0 {
            return Err(io::Error::last_os_error());
        }
        self.verify_content(&launch_identity)?;

        let exec_fd = target.as_raw_fd();
        let mut argv_storage = Vec::with_capacity(args.len() + 1);
        argv_storage.push(CString::new("beetle-memory-p7-sealed").expect("static argv0"));
        for arg in args {
            argv_storage.push(
                CString::new(arg.as_bytes())
                    .map_err(|_| invalid_input("P7 executable argument contains NUL"))?,
            );
        }
        let mut environment = std::env::vars_os()
            .filter(|(name, _)| !is_p7_retained_executable_authority_env(name))
            .map(|(name, value)| {
                let mut field = name.as_os_str().as_bytes().to_vec();
                field.push(b'=');
                field.extend_from_slice(value.as_os_str().as_bytes());
                CString::new(field)
                    .map_err(|_| invalid_input("P7 executable environment contains NUL"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        environment.push(
            CString::new(format!(
                "{P7_RETAINED_EXECUTABLE_SHA256_ENV}={}",
                launch_identity.sha256
            ))
            .expect("SHA256 environment has no NUL"),
        );
        environment.push(
            CString::new(format!(
                "{P7_RETAINED_EXECUTABLE_PATH_ENV}={}",
                self.path.display()
            ))
            .map_err(|_| invalid_input("P7 executable path contains NUL"))?,
        );
        let inherited = duplicate_inheritable_file(&target)?;
        environment.push(
            CString::new(format!(
                "{P7_RETAINED_EXECUTABLE_FD_ENV}={}",
                inherited.as_raw_fd()
            ))
            .expect("inherited descriptor environment has no NUL"),
        );
        let mut argv = argv_storage
            .iter()
            .map(|value| value.as_ptr())
            .collect::<Vec<_>>();
        argv.push(std::ptr::null());
        let mut envp = environment
            .iter()
            .map(|value| value.as_ptr())
            .collect::<Vec<_>>();
        envp.push(std::ptr::null());
        let argv = P7ExecPointerArray(argv);
        let envp = P7ExecPointerArray(envp);
        let mut command = Command::new("/proc/self/exe");
        // SAFETY: all C strings and pointer arrays are built before fork and retained by the closure.
        unsafe {
            command.pre_exec(move || {
                let _retained_c_strings = (&argv_storage, &environment);
                libc::fexecve(exec_fd, argv.as_ptr(), envp.as_ptr());
                Err(io::Error::last_os_error())
            });
        }
        Ok((
            command,
            P7ExecutableLaunchGuard {
                files: vec![target, inherited],
            },
            launch_identity,
        ))
    }

    #[cfg(target_os = "linux")]
    fn inherited_execution_authority() -> io::Result<P7InheritedExecutionAuthority> {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        use std::os::unix::fs::MetadataExt;

        let mut claim_state = P7_EXECUTION_AUTHORITY_CLAIM_STATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *claim_state == P7ExecutionAuthorityClaimState::Consumed {
            clear_p7_inherited_authority_environment();
            return Err(invalid_data(
                "P7 inherited execution authority was already consumed",
            ));
        }
        *claim_state = P7ExecutionAuthorityClaimState::Consumed;
        let mut claim = P7InheritedAuthorityClaim::new();
        let raw = std::env::var(P7_RETAINED_EXECUTABLE_FD_ENV)
            .map_err(|_| invalid_data("P7 retained executable descriptor is missing"))?
            .parse::<i32>()
            .map_err(|_| invalid_data("P7 retained executable descriptor is invalid"))?;
        if raw < 3 {
            return Err(invalid_data(
                "P7 retained executable descriptor is reserved or invalid",
            ));
        }
        claim.retain_inherited_fd(raw);
        let locator = std::env::var_os(P7_RETAINED_EXECUTABLE_PATH_ENV)
            .map(PathBuf::from)
            .ok_or_else(|| invalid_data("P7 retained executable locator is missing"))?;
        if !locator.is_absolute() {
            return Err(invalid_data(
                "P7 retained executable locator must be absolute",
            ));
        }
        let expected_sha256 = std::env::var(P7_RETAINED_EXECUTABLE_SHA256_ENV)
            .map_err(|_| invalid_data("P7 retained executable SHA256 is missing"))?;
        if expected_sha256.len() != 64
            || !expected_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(invalid_data("P7 retained executable SHA256 is invalid"));
        }
        // SAFETY: F_DUPFD_CLOEXEC validates raw and returns a new owned descriptor on success.
        let duplicate = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 3) };
        if duplicate < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fcntl returned a new descriptor uniquely owned by this value.
        let owned = unsafe { OwnedFd::from_raw_fd(duplicate) };
        let file = fs::File::from(owned);
        claim.revoke()?;
        let inherited_metadata = file.metadata()?;
        if !inherited_metadata.file_type().is_file() || inherited_metadata.mode() & 0o111 == 0 {
            return Err(invalid_data(
                "P7 retained executable descriptor is not an executable regular file",
            ));
        }
        // SAFETY: F_GET_SEALS only queries the live descriptor.
        let actual_seals = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) };
        if actual_seals < 0 {
            return Err(invalid_data(
                "P7 retained executable descriptor is not a sealed memfd",
            ));
        }
        require_p7_execution_seals(actual_seals)?;

        let current = fs::File::open("/proc/self/exe")?;
        let current_metadata = current.metadata()?;
        if inherited_metadata.dev() != current_metadata.dev()
            || inherited_metadata.ino() != current_metadata.ino()
        {
            return Err(invalid_data(
                "P7 retained executable descriptor is not the current execution object",
            ));
        }
        Ok(P7InheritedExecutionAuthority {
            file,
            expected_sha256,
            locator,
        })
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn executable_command(
        &mut self,
        _args: &[String],
    ) -> io::Result<(Command, P7ExecutableLaunchGuard, P7ContentIdentity)> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "P7 sealed execution on macOS requires an independently owned execution broker; Darwin pathname exec and /dev/fd exec are not an immutable byte boundary",
        ))
    }

    #[cfg(windows)]
    pub(crate) fn executable_command(
        &mut self,
        _args: &[String],
    ) -> io::Result<(Command, P7ExecutableLaunchGuard, P7ContentIdentity)> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "P7 sealed execution on Windows requires an independently owned execution broker; pathname spawn is not an immutable byte boundary",
        ))
    }

    #[cfg(not(target_os = "linux"))]
    fn inherited_execution_authority() -> io::Result<P7InheritedExecutionAuthority> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "P7 inherited sealed execution authority is available only on Linux",
        ))
    }

    fn hash_for_launch(&mut self) -> io::Result<P7ContentIdentity> {
        self.file.seek(SeekFrom::Start(0))?;
        let limit = self
            .admitted_len
            .checked_add(1)
            .ok_or_else(|| invalid_input("P7 retained executable read limit overflow"))?;
        let mut reader = HashingReader::new((&mut self.file).take(limit));
        io::copy(&mut reader, &mut io::sink())?;
        let identity = reader.finish();
        self.file.seek(SeekFrom::Start(0))?;
        if identity.byte_len != self.admitted_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "P7 retained executable changed length while being hashed",
            ));
        }
        self.verify_unchanged()?;
        Ok(identity)
    }

    pub fn consume_once<T>(
        self,
        consume: impl FnOnce(&mut dyn Read) -> io::Result<T>,
    ) -> io::Result<(T, P7ContentIdentity)> {
        self.consume_once_boxed(|reader| {
            consume(reader).map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
        })
        .map_err(|error| io::Error::other(error.to_string()))
    }

    pub fn consume_once_boxed<T>(
        mut self,
        consume: impl FnOnce(&mut dyn Read) -> Result<T, Box<dyn std::error::Error>>,
    ) -> Result<(T, P7ContentIdentity), Box<dyn std::error::Error>> {
        let limit = self
            .admitted_len
            .checked_add(1)
            .ok_or_else(|| invalid_input("P7 retained file read limit overflow"))?;
        let mut reader = HashingReader::new((&mut self.file).take(limit));
        let value = consume(&mut reader)?;
        io::copy(&mut reader, &mut io::sink())?;
        let identity = reader.finish();
        if identity.byte_len != self.admitted_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "P7 retained file changed length while being consumed",
            )
            .into());
        }
        self.verify_unchanged()?;
        Ok((value, identity))
    }

    pub fn hash_once(self) -> io::Result<P7ContentIdentity> {
        self.consume_once(|reader| io::copy(reader, &mut io::sink()))
            .map(|(_, identity)| identity)
    }

    pub fn copy_and_hash_once(self, writer: &mut dyn Write) -> io::Result<P7ContentIdentity> {
        self.consume_once(|reader| io::copy(reader, writer))
            .map(|(_, identity)| identity)
    }
}

#[cfg(target_os = "linux")]
fn duplicate_inheritable_file(file: &fs::File) -> io::Result<fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};

    // SAFETY: F_DUPFD validates the live source descriptor and returns a new descriptor without
    // FD_CLOEXEC, allowing the sealed child to attest and republish the exact execution object.
    let duplicate = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 3) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fcntl returned a new descriptor uniquely owned by this File.
    Ok(unsafe { fs::File::from_raw_fd(duplicate) })
}

pub(crate) struct P7ExecutableLaunchGuard {
    files: Vec<fs::File>,
}

impl Drop for P7ExecutableLaunchGuard {
    fn drop(&mut self) {
        self.files.clear();
    }
}

struct HashingReader<R> {
    inner: R,
    hasher: Sha256,
    byte_len: u64,
}

impl<R> HashingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            byte_len: 0,
        }
    }

    fn finish(self) -> P7ContentIdentity {
        P7ContentIdentity {
            byte_len: self.byte_len,
            sha256: format!("{:x}", self.hasher.finalize()),
        }
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        self.byte_len = self
            .byte_len
            .checked_add(u64::try_from(read).map_err(io::Error::other)?)
            .ok_or_else(|| io::Error::other("P7 retained file byte count overflow"))?;
        Ok(read)
    }
}

pub(crate) struct P7CohortArtifactOwner {
    retained: P7RetainedDirectoryOwner,
}

pub struct P7AuthorityBoundArtifactTransaction<'authority> {
    authority: &'authority mut dyn P7ExternalWriteAuthority,
    owner: P7CohortArtifactOwner,
}

pub struct P7AuthorityBoundReleaseTransaction<'authority> {
    authority: &'authority mut dyn P7ExternalWriteAuthority,
    releases: P7CohortArtifactOwner,
    staging: P7CohortArtifactOwner,
    staging_name: String,
    tracked_files: Vec<String>,
    committed: bool,
    cleaned: bool,
    _lock: Option<P7BundleWriteGuard>,
}

pub struct P7BundleWriteGuard {
    directory: platform::DirectoryHandle,
    lock_name: String,
    _lock: fs::File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P7ArtifactPublishOutcome {
    Published,
    ReusedIdentical,
}

#[derive(Debug)]
pub struct P7DirectoryInstallError {
    source: io::Error,
    committed: bool,
}

impl P7DirectoryInstallError {
    fn before_commit(source: io::Error) -> Self {
        Self {
            source,
            committed: false,
        }
    }

    #[cfg(unix)]
    fn after_commit(source: io::Error) -> Self {
        Self {
            source,
            committed: true,
        }
    }

    pub fn committed(&self) -> bool {
        self.committed
    }

    pub fn cleanup_permitted(&self) -> bool {
        !self.committed
    }

    pub fn kind(&self) -> io::ErrorKind {
        self.source.kind()
    }

    pub fn into_inner(self) -> io::Error {
        self.source
    }
}

impl std::fmt::Display for P7DirectoryInstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for P7DirectoryInstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[cfg(all(test, unix))]
thread_local! {
    static P7_FAIL_NEXT_DIRECTORY_INSTALL_AFTER_COMMIT: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(all(test, unix))]
fn inject_p7_directory_install_post_commit_failure() {
    P7_FAIL_NEXT_DIRECTORY_INSTALL_AFTER_COMMIT.with(|fault| fault.set(true));
}

#[cfg(all(test, unix))]
fn take_p7_directory_install_post_commit_failure() -> bool {
    P7_FAIL_NEXT_DIRECTORY_INSTALL_AFTER_COMMIT.with(|fault| fault.replace(false))
}

#[cfg(all(test, unix))]
mod directory_install_error_tests {
    use super::*;

    struct TestWriteAuthority {
        verification_attempts: usize,
        reject_on_attempt: Option<usize>,
    }

    impl TestWriteAuthority {
        fn valid() -> Self {
            Self {
                verification_attempts: 0,
                reject_on_attempt: None,
            }
        }

        fn rejecting(reject_on_attempt: usize) -> Self {
            Self {
                verification_attempts: 0,
                reject_on_attempt: Some(reject_on_attempt),
            }
        }
    }

    impl P7ExternalWriteAuthority for TestWriteAuthority {
        fn verify_external_write_authority(&mut self) -> io::Result<()> {
            self.verification_attempts += 1;
            if self.reject_on_attempt == Some(self.verification_attempts) {
                return Err(invalid_data("test execution authority changed"));
            }
            Ok(())
        }

        fn process_execution_authority(&mut self) -> &mut P7ProcessExecutionAuthority {
            panic!("artifact transaction tests never copy execution bytes")
        }
    }

    fn fixture_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("bm-p7-{label}-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).expect("create P7 transaction fixture root");
        fs::canonicalize(root).expect("canonical P7 transaction fixture root")
    }

    #[test]
    fn directory_install_error_preserves_the_atomic_commit_boundary() {
        let before = P7DirectoryInstallError::before_commit(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "before",
        ));
        assert!(!before.committed());
        assert!(before.cleanup_permitted());
        assert_eq!(before.kind(), io::ErrorKind::AlreadyExists);

        let after = P7DirectoryInstallError::after_commit(io::Error::other("after"));
        assert!(after.committed());
        assert!(!after.cleanup_permitted());
        assert_eq!(after.kind(), io::ErrorKind::Other);
    }

    #[test]
    fn post_commit_install_failure_preserves_the_final_release_directory() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "bm-p7-post-commit-install-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create benchmark root");
        let root = fs::canonicalize(root).expect("canonical benchmark root");
        let releases =
            open_or_create_p7_verifier_release_store(&root).expect("open verifier releases");
        let staging = releases
            .create_directory(".staging-post-commit")
            .expect("create staging directory");
        staging
            .publish_immutable_bytes(b"release-evidence", ".payload.tmp", "payload")
            .expect("publish staged payload");

        inject_p7_directory_install_post_commit_failure();
        let error = releases
            .install_staged_directory(".staging-post-commit", "content-address")
            .expect_err("injected post-commit verification must fail closed");
        assert!(error.committed());
        assert!(!error.cleanup_permitted());
        assert_eq!(
            fs::read(root.join("verifier/releases/content-address/payload"))
                .expect("read preserved final payload"),
            b"release-evidence"
        );
        assert!(!root.join("verifier/releases/.staging-post-commit").exists());

        fs::remove_dir_all(root).expect("remove owned test root");
    }

    #[test]
    fn authority_bound_artifact_transaction_revalidates_before_final_link() {
        let root = fixture_root("authority-final-link");
        initialize_p7_cohort(&root, "run-1").expect("initialize cohort fixture");
        let mut authority = TestWriteAuthority::rejecting(3);
        let mut transaction = open_authority_bound_p7_cohort(&mut authority, &root, "run-1")
            .expect("open authority-bound cohort");
        let mut staged = transaction
            .create_staged_file("artifact.tmp")
            .expect("create staged artifact");
        staged
            .write_all(b"uncommitted")
            .expect("write staged bytes");
        staged.sync_all().expect("sync staged bytes");

        let error = transaction
            .publish_staged_file(staged, "artifact.tmp", "artifact.json")
            .expect_err("changed authority must block the final link");
        assert!(error
            .to_string()
            .contains("test execution authority changed"));
        assert!(!transaction.path().join("artifact.json").exists());
        assert_eq!(
            fs::read(transaction.path().join("artifact.tmp")).expect("preserved staged evidence"),
            b"uncommitted"
        );
        fs::remove_dir_all(root).expect("remove final-link fixture");
    }

    #[test]
    fn authority_bound_artifact_transaction_only_creates_staged_names() {
        let root = fixture_root("staged-name");
        initialize_p7_cohort(&root, "run-1").expect("initialize cohort fixture");
        let mut authority = TestWriteAuthority::valid();
        let mut transaction = open_authority_bound_p7_cohort(&mut authority, &root, "run-1")
            .expect("open authority-bound cohort");

        let error = transaction
            .create_staged_file("artifact.json")
            .expect_err("final artifact name must not be created through the staged API");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(!transaction.path().join("artifact.json").exists());
        fs::remove_dir_all(root).expect("remove staged-name fixture");
    }

    #[test]
    fn uncommitted_bundle_cleanup_requires_the_matching_bundle_lock() {
        let root = fixture_root("bundle-cleanup-lock");
        initialize_p7_cohort(&root, "run-1").expect("initialize cohort fixture");
        let mut authority = TestWriteAuthority::valid();
        let mut transaction = open_authority_bound_p7_cohort(&mut authority, &root, "run-1")
            .expect("open authority-bound cohort");
        transaction
            .publish_immutable_bytes(b"uncommitted", ".bundle.summary.tmp", "bundle.summary.json")
            .expect("publish uncommitted bundle artifact");
        let wrong_guard = transaction
            .lock_bundle("other.lock")
            .expect("lock unrelated bundle");

        let error = transaction
            .discard_uncommitted_bundle_artifacts(
                &wrong_guard,
                "bundle.commit.json",
                &["bundle.summary.json"],
            )
            .expect_err("unrelated bundle lock must not authorize cleanup");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            fs::read(transaction.path().join("bundle.summary.json"))
                .expect("preserve uncommitted artifact"),
            b"uncommitted"
        );
        fs::remove_dir_all(root).expect("remove bundle-cleanup-lock fixture");
    }

    #[test]
    fn uncommitted_bundle_cleanup_rejects_same_named_lock_from_another_cohort() {
        let root = fixture_root("bundle-cleanup-cohort");
        initialize_p7_cohort(&root, "run-1").expect("initialize first cohort fixture");
        initialize_p7_cohort(&root, "run-2").expect("initialize second cohort fixture");
        let mut first_authority = TestWriteAuthority::valid();
        let mut first = open_authority_bound_p7_cohort(&mut first_authority, &root, "run-1")
            .expect("open first authority-bound cohort");
        let foreign_guard = first
            .lock_bundle("bundle.lock")
            .expect("lock first cohort bundle");
        let mut second_authority = TestWriteAuthority::valid();
        let mut second = open_authority_bound_p7_cohort(&mut second_authority, &root, "run-2")
            .expect("open second authority-bound cohort");
        second
            .publish_immutable_bytes(b"uncommitted", ".bundle.summary.tmp", "bundle.summary.json")
            .expect("publish second cohort artifact");

        let error = second
            .discard_uncommitted_bundle_artifacts(
                &foreign_guard,
                "bundle.commit.json",
                &["bundle.summary.json"],
            )
            .expect_err("foreign cohort lock must not authorize cleanup");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            fs::read(second.path().join("bundle.summary.json"))
                .expect("preserve second cohort artifact"),
            b"uncommitted"
        );
        drop(second);
        drop(foreign_guard);
        drop(first);
        fs::remove_dir_all(root).expect("remove bundle-cleanup-cohort fixture");
    }

    #[test]
    fn uncommitted_bundle_cleanup_rejects_replaced_cohort_at_the_same_path() {
        let root = fixture_root("bundle-cleanup-replaced-cohort");
        initialize_p7_cohort(&root, "run-1").expect("initialize original cohort fixture");
        let mut original_authority = TestWriteAuthority::valid();
        let mut original = open_authority_bound_p7_cohort(&mut original_authority, &root, "run-1")
            .expect("open original authority-bound cohort");
        let stale_guard = original
            .lock_bundle("bundle.lock")
            .expect("lock original cohort bundle");
        drop(original);

        let cohort_path = root.join("results/runs/run-1");
        let displaced_path = root.join("results/runs/run-1-displaced");
        fs::rename(&cohort_path, &displaced_path).expect("displace original cohort directory");
        initialize_p7_cohort(&root, "run-1").expect("initialize replacement cohort fixture");
        let mut replacement_authority = TestWriteAuthority::valid();
        let mut replacement =
            open_authority_bound_p7_cohort(&mut replacement_authority, &root, "run-1")
                .expect("open replacement authority-bound cohort");
        replacement
            .publish_immutable_bytes(b"replacement", ".bundle.summary.tmp", "bundle.summary.json")
            .expect("publish replacement cohort artifact");

        let error = replacement
            .discard_uncommitted_bundle_artifacts(
                &stale_guard,
                "bundle.commit.json",
                &["bundle.summary.json"],
            )
            .expect_err("stale same-path cohort lock must not authorize cleanup");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            fs::read(replacement.path().join("bundle.summary.json"))
                .expect("preserve replacement cohort artifact"),
            b"replacement"
        );
        drop(replacement);
        drop(stale_guard);
        fs::remove_dir_all(root).expect("remove replaced-cohort fixture");
    }

    #[test]
    fn authority_bound_release_precommit_cleanup_preserves_existing_final() {
        let root = fixture_root("release-precommit");
        let mut authority = TestWriteAuthority::valid();
        let releases =
            open_or_create_p7_verifier_release_store_with_authority(&mut authority, &root)
                .expect("open verifier releases");
        let mut first = P7AuthorityBoundReleaseTransaction::new(
            &mut authority,
            releases,
            ".staging-test",
            None,
        )
        .expect("begin first release");
        first
            .publish_immutable_bytes(b"first", ".payload.tmp", "payload")
            .expect("publish first staged payload");
        first
            .install("content-address")
            .expect("install first release");
        drop(first);

        let releases =
            open_or_create_p7_verifier_release_store_with_authority(&mut authority, &root)
                .expect("reopen verifier releases");
        let mut second = P7AuthorityBoundReleaseTransaction::new(
            &mut authority,
            releases,
            ".staging-test",
            None,
        )
        .expect("begin second release");
        second
            .publish_immutable_bytes(b"second", ".payload.tmp", "payload")
            .expect("publish second staged payload");
        let second_staging = second.staging_path().to_path_buf();
        let error = second
            .install("content-address")
            .expect_err("existing content address must fail before commit");
        assert!(!error.committed());
        assert!(error.cleanup_permitted());
        second
            .cleanup_uncommitted()
            .expect("clean only second staging directory");
        assert!(!second_staging.exists());
        assert_eq!(
            fs::read(root.join("verifier/releases/content-address/payload"))
                .expect("read preserved first release"),
            b"first"
        );
        fs::remove_dir_all(root).expect("remove precommit fixture");
    }

    #[test]
    fn authority_bound_release_postcommit_failure_forbids_cleanup() {
        let root = fixture_root("release-postcommit");
        let mut authority = TestWriteAuthority::valid();
        let releases =
            open_or_create_p7_verifier_release_store_with_authority(&mut authority, &root)
                .expect("open verifier releases");
        let mut transaction = P7AuthorityBoundReleaseTransaction::new(
            &mut authority,
            releases,
            ".staging-test",
            None,
        )
        .expect("begin postcommit release");
        transaction
            .publish_immutable_bytes(b"release", ".payload.tmp", "payload")
            .expect("publish staged release payload");

        inject_p7_directory_install_post_commit_failure();
        let error = transaction
            .install("content-address")
            .expect_err("injected postcommit verification must fail");
        assert!(error.committed());
        assert!(transaction.committed());
        assert!(transaction.cleanup_uncommitted().is_err());
        assert_eq!(
            fs::read(root.join("verifier/releases/content-address/payload"))
                .expect("read preserved committed release"),
            b"release"
        );
        fs::remove_dir_all(root).expect("remove postcommit fixture");
    }
}

impl P7CohortArtifactOwner {
    fn new(path: PathBuf, directory: platform::DirectoryHandle) -> Self {
        Self {
            retained: P7RetainedDirectoryOwner { path, directory },
        }
    }

    pub(crate) fn path(&self) -> &Path {
        self.retained.path()
    }

    pub(crate) fn open_or_create_directory(&self, component: &str) -> io::Result<Self> {
        validate_component(component)?;
        let directory = self
            .retained
            .directory
            .open_or_create_directory(component)?;
        Ok(Self::new(self.path().join(component), directory))
    }

    pub(crate) fn create_directory(&self, component: &str) -> io::Result<Self> {
        validate_component(component)?;
        let directory = self.retained.directory.create_directory(component)?;
        Ok(Self::new(self.path().join(component), directory))
    }

    pub(crate) fn create_new_file(&self, file_name: &str) -> io::Result<fs::File> {
        validate_component(file_name)?;
        self.retained.directory.create_new_file(file_name)
    }

    pub(crate) fn lock_bundle(&self, lock_name: &str) -> io::Result<P7BundleWriteGuard> {
        validate_component(lock_name)?;
        let directory = self.retained.directory.try_clone()?;
        let lock = self.retained.directory.open_and_lock_file(lock_name)?;
        Ok(P7BundleWriteGuard {
            directory,
            lock_name: lock_name.to_string(),
            _lock: lock,
        })
    }

    pub(crate) fn open_existing_file(&self, file_name: &str) -> io::Result<fs::File> {
        validate_component(file_name)?;
        self.retained.open_existing_file(file_name)
    }

    pub(crate) fn try_open_existing_file(&self, file_name: &str) -> io::Result<Option<fs::File>> {
        self.retained.try_open_existing_file(file_name)
    }

    fn try_open_existing_deletable_file(&self, file_name: &str) -> io::Result<Option<fs::File>> {
        validate_component(file_name)?;
        match self
            .retained
            .directory
            .open_existing_deletable_file(file_name)
        {
            Ok(file) => Ok(Some(file)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn verify_existing_file(&self, file_name: &str, file: &fs::File) -> io::Result<()> {
        self.retained.verify_file_identity(file_name, file)
    }

    #[cfg(all(test, unix))]
    pub(crate) fn publish_staged_file(
        &self,
        mut staged_file: fs::File,
        staged_name: &str,
        final_name: &str,
    ) -> io::Result<P7ArtifactPublishOutcome> {
        validate_component(staged_name)?;
        validate_component(final_name)?;
        if staged_name == final_name {
            return Err(invalid_input(
                "P7 staged and final artifact names must differ",
            ));
        }
        staged_file.sync_all()?;
        match self.retained.directory.publish_staged_file(
            &staged_file,
            staged_name,
            final_name,
            || Ok(()),
        ) {
            Ok(()) => Ok(P7ArtifactPublishOutcome::Published),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let comparison = self
                    .open_existing_file(final_name)
                    .and_then(|mut existing| files_equal(&mut staged_file, &mut existing));
                self.retained
                    .directory
                    .discard_staged_file(&staged_file, staged_name)?;
                if !comparison? {
                    Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "P7 immutable artifact already exists with different content",
                    ))
                } else {
                    Ok(P7ArtifactPublishOutcome::ReusedIdentical)
                }
            }
            Err(error) => {
                let _ = self
                    .retained
                    .directory
                    .discard_staged_file(&staged_file, staged_name);
                Err(error)
            }
        }
    }

    #[cfg(all(test, unix))]
    pub(crate) fn publish_immutable_bytes(
        &self,
        body: &[u8],
        staged_name: &str,
        final_name: &str,
    ) -> io::Result<P7ArtifactPublishOutcome> {
        validate_component(staged_name)?;
        validate_component(final_name)?;
        if staged_name == final_name {
            return Err(invalid_input(
                "P7 staged and final artifact names must differ",
            ));
        }
        let mut staged_file = self.create_new_file(staged_name)?;
        if let Err(error) = staged_file
            .write_all(body)
            .and_then(|()| staged_file.sync_all())
        {
            let _ = self
                .retained
                .directory
                .discard_staged_file(&staged_file, staged_name);
            return Err(error);
        }
        self.publish_staged_file(staged_file, staged_name, final_name)
    }

    pub(crate) fn install_staged_directory(
        &self,
        staged_name: &str,
        final_name: &str,
    ) -> Result<(), P7DirectoryInstallError> {
        validate_component(staged_name).map_err(P7DirectoryInstallError::before_commit)?;
        validate_component(final_name).map_err(P7DirectoryInstallError::before_commit)?;
        if staged_name == final_name {
            return Err(P7DirectoryInstallError::before_commit(invalid_input(
                "P7 staged and final directory names must differ",
            )));
        }
        self.retained
            .directory
            .install_directory_no_replace(staged_name, final_name)
    }

    pub(crate) fn discard_empty_directory(
        &self,
        directory_name: &str,
        expected: &P7CohortArtifactOwner,
    ) -> io::Result<()> {
        validate_component(directory_name)?;
        self.retained
            .directory
            .discard_empty_directory(directory_name, &expected.retained.directory)
    }
}

impl<'authority> P7AuthorityBoundArtifactTransaction<'authority> {
    fn new(
        authority: &'authority mut dyn P7ExternalWriteAuthority,
        owner: P7CohortArtifactOwner,
    ) -> Self {
        Self { authority, owner }
    }

    pub fn path(&self) -> &Path {
        self.owner.path()
    }

    pub fn display(&self) -> impl fmt::Display + '_ {
        self.path().display()
    }

    pub fn lock_bundle(&mut self, lock_name: &str) -> io::Result<P7BundleWriteGuard> {
        self.authority.verify_external_write_authority()?;
        self.owner.lock_bundle(lock_name)
    }

    pub fn create_staged_file(&mut self, file_name: &str) -> io::Result<fs::File> {
        validate_staged_component(file_name)?;
        self.authority.verify_external_write_authority()?;
        self.owner.create_new_file(file_name)
    }

    pub fn publish_staged_file(
        &mut self,
        staged_file: fs::File,
        staged_name: &str,
        final_name: &str,
    ) -> io::Result<P7ArtifactPublishOutcome> {
        publish_staged_file_with_authority(
            self.authority,
            &self.owner,
            staged_file,
            staged_name,
            final_name,
        )
    }

    pub fn publish_immutable_bytes(
        &mut self,
        body: &[u8],
        staged_name: &str,
        final_name: &str,
    ) -> io::Result<P7ArtifactPublishOutcome> {
        validate_staged_component(staged_name)?;
        validate_component(final_name)?;
        if staged_name == final_name {
            return Err(invalid_input(
                "P7 staged and final artifact names must differ",
            ));
        }
        let mut staged_file = self.create_staged_file(staged_name)?;
        if let Err(error) = staged_file
            .write_all(body)
            .and_then(|()| staged_file.sync_all())
        {
            self.authority.verify_external_write_authority()?;
            self.owner
                .retained
                .directory
                .discard_staged_file(&staged_file, staged_name)?;
            return Err(error);
        }
        self.publish_staged_file(staged_file, staged_name, final_name)
    }

    pub fn discard_uncommitted_bundle_artifacts(
        &mut self,
        bundle_guard: &P7BundleWriteGuard,
        commit_name: &str,
        artifact_names: &[&str],
    ) -> io::Result<()> {
        validate_component(commit_name)?;
        let stem = commit_name
            .strip_suffix(".commit.json")
            .filter(|stem| !stem.is_empty())
            .ok_or_else(|| invalid_input("P7 bundle commit name must end in .commit.json"))?;
        let expected_lock_name = format!("{stem}.lock");
        if bundle_guard.lock_name != expected_lock_name
            || self
                .owner
                .retained
                .directory
                .verify_same_directory(&bundle_guard.directory)
                .is_err()
        {
            return Err(invalid_input(
                "P7 bundle cleanup requires the matching cohort bundle lock",
            ));
        }
        for artifact_name in artifact_names {
            validate_component(artifact_name)?;
            if !artifact_name.starts_with(stem)
                || !matches!(
                    artifact_name.strip_prefix(stem),
                    Some(".jsonl" | ".summary.json")
                )
            {
                return Err(invalid_input(
                    "P7 uncommitted artifact does not belong to its bundle commit",
                ));
            }
        }
        if self.owner.try_open_existing_file(commit_name)?.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "P7 committed bundle must never be cleaned or overwritten",
            ));
        }
        for artifact_name in artifact_names {
            let Some(file) = self.owner.try_open_existing_deletable_file(artifact_name)? else {
                continue;
            };
            self.authority.verify_external_write_authority()?;
            self.owner
                .retained
                .directory
                .discard_staged_file(&file, artifact_name)?;
        }
        if self.owner.try_open_existing_file(commit_name)?.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "P7 bundle commit appeared while uncommitted artifacts were cleaned",
            ));
        }
        Ok(())
    }

    pub fn open_existing_file(&self, file_name: &str) -> io::Result<fs::File> {
        self.owner.open_existing_file(file_name)
    }

    pub fn try_open_existing_file(&self, file_name: &str) -> io::Result<Option<fs::File>> {
        self.owner.try_open_existing_file(file_name)
    }

    pub fn verify_existing_file(&self, file_name: &str, file: &fs::File) -> io::Result<()> {
        self.owner.verify_existing_file(file_name, file)
    }

    pub fn into_open_or_create_directory(self, component: &str) -> io::Result<Self> {
        self.authority.verify_external_write_authority()?;
        let owner = self.owner.open_or_create_directory(component)?;
        Ok(Self::new(self.authority, owner))
    }

    pub fn into_create_directory(self, component: &str) -> io::Result<Self> {
        self.authority.verify_external_write_authority()?;
        let owner = self.owner.create_directory(component)?;
        Ok(Self::new(self.authority, owner))
    }
}

impl<'authority> P7AuthorityBoundReleaseTransaction<'authority> {
    fn new(
        authority: &'authority mut dyn P7ExternalWriteAuthority,
        releases: P7CohortArtifactOwner,
        staging_prefix: &str,
        lock_name: Option<&str>,
    ) -> io::Result<Self> {
        let _lock = if let Some(lock_name) = lock_name {
            authority.verify_external_write_authority()?;
            Some(releases.lock_bundle(lock_name)?)
        } else {
            None
        };
        let staging_name = format!(
            "{staging_prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos()
        );
        authority.verify_external_write_authority()?;
        let staging = releases.create_directory(&staging_name)?;
        Ok(Self {
            authority,
            releases,
            staging,
            staging_name,
            tracked_files: Vec::new(),
            committed: false,
            cleaned: false,
            _lock,
        })
    }

    pub fn releases_path(&self) -> &Path {
        self.releases.path()
    }

    pub fn staging_path(&self) -> &Path {
        self.staging.path()
    }

    pub fn copy_execution(
        &mut self,
        file_name: &str,
        #[cfg_attr(not(unix), allow(unused_variables))] unix_mode: u32,
    ) -> io::Result<P7ContentIdentity> {
        self.authority.verify_external_write_authority()?;
        let mut destination = self.staging.create_new_file(file_name)?;
        self.tracked_files.push(file_name.to_string());
        let copied = self
            .authority
            .process_execution_authority()
            .copy_execution_to(&mut destination)?;
        destination.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            destination.set_permissions(fs::Permissions::from_mode(unix_mode))?;
            destination.sync_all()?;
        }
        Ok(copied)
    }

    pub fn publish_immutable_bytes(
        &mut self,
        body: &[u8],
        staged_name: &str,
        final_name: &str,
    ) -> io::Result<P7ArtifactPublishOutcome> {
        self.authority.verify_external_write_authority()?;
        let mut staged_file = self.staging.create_new_file(staged_name)?;
        if let Err(error) = staged_file
            .write_all(body)
            .and_then(|()| staged_file.sync_all())
        {
            self.authority.verify_external_write_authority()?;
            self.staging
                .retained
                .directory
                .discard_staged_file(&staged_file, staged_name)?;
            return Err(error);
        }
        let outcome = publish_staged_file_with_authority(
            self.authority,
            &self.staging,
            staged_file,
            staged_name,
            final_name,
        )?;
        self.tracked_files.push(final_name.to_string());
        Ok(outcome)
    }

    pub fn install(&mut self, final_name: &str) -> Result<(), P7DirectoryInstallError> {
        if let Err(source) = self.authority.verify_external_write_authority() {
            return Err(P7DirectoryInstallError::before_commit(source));
        }
        let result = self
            .releases
            .install_staged_directory(&self.staging_name, final_name);
        if result.is_ok() || result.as_ref().is_err_and(|error| error.committed()) {
            self.committed = true;
        }
        result
    }

    pub fn committed(&self) -> bool {
        self.committed
    }

    pub fn cleanup_uncommitted(&mut self) -> io::Result<()> {
        if self.committed {
            return Err(invalid_data(
                "committed P7 release transaction cannot clean the final directory",
            ));
        }
        if self.cleaned {
            return Ok(());
        }
        for file_name in self.tracked_files.iter().rev() {
            let Some(file) = self.staging.try_open_existing_deletable_file(file_name)? else {
                continue;
            };
            self.authority.verify_external_write_authority()?;
            self.staging
                .retained
                .directory
                .discard_staged_file(&file, file_name)?;
        }
        self.authority.verify_external_write_authority()?;
        self.releases
            .discard_empty_directory(&self.staging_name, &self.staging)?;
        self.cleaned = true;
        Ok(())
    }
}

fn publish_staged_file_with_authority(
    authority: &mut dyn P7ExternalWriteAuthority,
    owner: &P7CohortArtifactOwner,
    mut staged_file: fs::File,
    staged_name: &str,
    final_name: &str,
) -> io::Result<P7ArtifactPublishOutcome> {
    validate_staged_component(staged_name)?;
    validate_component(final_name)?;
    if staged_name == final_name {
        return Err(invalid_input(
            "P7 staged and final artifact names must differ",
        ));
    }
    staged_file.sync_all()?;
    authority.verify_external_write_authority()?;
    match owner.retained.directory.publish_staged_file(
        &staged_file,
        staged_name,
        final_name,
        || authority.verify_external_write_authority(),
    ) {
        Ok(()) => Ok(P7ArtifactPublishOutcome::Published),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let comparison = owner
                .open_existing_file(final_name)
                .and_then(|mut existing| files_equal(&mut staged_file, &mut existing));
            authority.verify_external_write_authority()?;
            owner
                .retained
                .directory
                .discard_staged_file(&staged_file, staged_name)?;
            if !comparison? {
                Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "P7 immutable artifact already exists with different content",
                ))
            } else {
                Ok(P7ArtifactPublishOutcome::ReusedIdentical)
            }
        }
        Err(error) => {
            authority.verify_external_write_authority()?;
            owner
                .retained
                .directory
                .discard_staged_file(&staged_file, staged_name)?;
            Err(error)
        }
    }
}

fn files_equal(left: &mut fs::File, right: &mut fs::File) -> io::Result<bool> {
    if left.metadata()?.len() != right.metadata()?.len() {
        return Ok(false);
    }
    left.seek(SeekFrom::Start(0))?;
    right.seek(SeekFrom::Start(0))?;
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_read = left.read(&mut left_buffer)?;
        let right_read = right.read(&mut right_buffer)?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

#[cfg(all(test, unix))]
pub(crate) fn initialize_p7_cohort(root: &Path, run_id: &str) -> io::Result<P7CohortArtifactOwner> {
    validate_run_id(run_id)?;
    let root_owner = platform::DirectoryHandle::open_root(root)?;
    let results = root_owner.open_or_create_directory("results")?;
    let runs = results.open_or_create_directory("runs")?;
    let cohort = runs.create_directory(run_id)?;
    Ok(P7CohortArtifactOwner::new(
        root.join("results/runs").join(run_id),
        cohort,
    ))
}

pub(crate) fn open_p7_cohort_artifact_owner(
    root: &Path,
    run_id: &str,
) -> io::Result<P7CohortArtifactOwner> {
    validate_run_id(run_id)?;
    let root_owner = platform::DirectoryHandle::open_root(root)?;
    let results = root_owner.open_directory("results")?;
    let runs = results.open_directory("runs")?;
    let cohort = runs.open_directory(run_id)?;
    Ok(P7CohortArtifactOwner::new(
        root.join("results/runs").join(run_id),
        cohort,
    ))
}

#[cfg(all(test, unix))]
pub(crate) fn open_or_create_p7_verifier_release_store(
    root: &Path,
) -> io::Result<P7CohortArtifactOwner> {
    let root_owner = platform::DirectoryHandle::open_root(root)?;
    let verifier = root_owner.open_or_create_directory("verifier")?;
    let releases = verifier.open_or_create_directory("releases")?;
    Ok(P7CohortArtifactOwner::new(
        root.join("verifier/releases"),
        releases,
    ))
}

fn initialize_p7_cohort_with_authority(
    authority: &mut dyn P7ExternalWriteAuthority,
    root: &Path,
    run_id: &str,
) -> io::Result<P7CohortArtifactOwner> {
    validate_run_id(run_id)?;
    authority.verify_external_write_authority()?;
    let root_owner = platform::DirectoryHandle::open_root(root)?;
    authority.verify_external_write_authority()?;
    let results = root_owner.open_or_create_directory("results")?;
    authority.verify_external_write_authority()?;
    let runs = results.open_or_create_directory("runs")?;
    authority.verify_external_write_authority()?;
    let cohort = runs.create_directory(run_id)?;
    Ok(P7CohortArtifactOwner::new(
        root.join("results/runs").join(run_id),
        cohort,
    ))
}

pub(crate) fn initialize_authority_bound_p7_cohort<'authority>(
    authority: &'authority mut dyn P7ExternalWriteAuthority,
    root: &Path,
    run_id: &str,
) -> io::Result<P7AuthorityBoundArtifactTransaction<'authority>> {
    let owner = initialize_p7_cohort_with_authority(authority, root, run_id)?;
    Ok(P7AuthorityBoundArtifactTransaction::new(authority, owner))
}

fn open_p7_cohort_artifact_owner_with_authority(
    authority: &mut dyn P7ExternalWriteAuthority,
    root: &Path,
    run_id: &str,
) -> io::Result<P7CohortArtifactOwner> {
    validate_run_id(run_id)?;
    authority.verify_external_write_authority()?;
    open_p7_cohort_artifact_owner(root, run_id)
}

pub(crate) fn open_authority_bound_p7_cohort<'authority>(
    authority: &'authority mut dyn P7ExternalWriteAuthority,
    root: &Path,
    run_id: &str,
) -> io::Result<P7AuthorityBoundArtifactTransaction<'authority>> {
    let owner = open_p7_cohort_artifact_owner_with_authority(authority, root, run_id)?;
    Ok(P7AuthorityBoundArtifactTransaction::new(authority, owner))
}

fn open_or_create_p7_release_store_with_authority(
    authority: &mut dyn P7ExternalWriteAuthority,
    root: &Path,
) -> io::Result<P7CohortArtifactOwner> {
    authority.verify_external_write_authority()?;
    let root_owner = platform::DirectoryHandle::open_root(root)?;
    authority.verify_external_write_authority()?;
    let runner = root_owner.open_or_create_directory("runner")?;
    authority.verify_external_write_authority()?;
    let releases = runner.open_or_create_directory("releases")?;
    Ok(P7CohortArtifactOwner::new(
        root.join("runner/releases"),
        releases,
    ))
}

fn open_or_create_p7_runner_authority_probe_store_with_authority(
    authority: &mut dyn P7ExternalWriteAuthority,
    root: &Path,
) -> io::Result<P7CohortArtifactOwner> {
    authority.verify_external_write_authority()?;
    let root_owner = platform::DirectoryHandle::open_root(root)?;
    authority.verify_external_write_authority()?;
    let runner = root_owner.open_or_create_directory("runner")?;
    authority.verify_external_write_authority()?;
    let probes = runner.open_or_create_directory("authority-probes")?;
    Ok(P7CohortArtifactOwner::new(
        root.join("runner/authority-probes"),
        probes,
    ))
}

fn open_or_create_p7_verifier_release_store_with_authority(
    authority: &mut dyn P7ExternalWriteAuthority,
    root: &Path,
) -> io::Result<P7CohortArtifactOwner> {
    authority.verify_external_write_authority()?;
    let root_owner = platform::DirectoryHandle::open_root(root)?;
    authority.verify_external_write_authority()?;
    let verifier = root_owner.open_or_create_directory("verifier")?;
    authority.verify_external_write_authority()?;
    let releases = verifier.open_or_create_directory("releases")?;
    Ok(P7CohortArtifactOwner::new(
        root.join("verifier/releases"),
        releases,
    ))
}

#[cfg(unix)]
mod platform {
    use super::{invalid_input, validate_canonical_root, P7DirectoryInstallError};
    use std::{
        ffi::CString,
        fs::File,
        io,
        os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        path::{Component, Path},
    };

    pub(super) struct DirectoryHandle(OwnedFd);

    impl DirectoryHandle {
        pub(super) fn try_clone(&self) -> io::Result<Self> {
            Ok(Self(self.0.try_clone()?))
        }

        pub(super) fn verify_same_directory(&self, expected: &Self) -> io::Result<()> {
            require_same_node(
                self.0.as_raw_fd(),
                expected.0.as_raw_fd(),
                "bundle lock directory",
            )
        }

        pub(super) fn open_root(root: &Path) -> io::Result<Self> {
            validate_canonical_root(root)?;
            // Anchor traversal at the filesystem root, then retain-open every component.
            let slash = CString::new("/").expect("static root path has no NUL");
            let fd = unsafe {
                libc::open(
                    slash.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            let mut current = owned_fd(fd)?;
            for component in root.components() {
                match component {
                    Component::RootDir => {}
                    Component::Normal(component) => {
                        let component = component.to_str().ok_or_else(|| {
                            invalid_input("P7 secure root component must be valid UTF-8")
                        })?;
                        current = open_directory_at(current.as_raw_fd(), component)?;
                    }
                    _ => {
                        return Err(invalid_input(
                            "P7 secure root must be an absolute normal path",
                        ))
                    }
                }
            }
            Ok(Self(current))
        }

        pub(super) fn open_directory(&self, component: &str) -> io::Result<Self> {
            open_directory_at(self.0.as_raw_fd(), component).map(Self)
        }

        pub(super) fn open_or_create_directory(&self, component: &str) -> io::Result<Self> {
            create_directory_at(self.0.as_raw_fd(), component, true).map(Self)
        }

        pub(super) fn create_directory(&self, component: &str) -> io::Result<Self> {
            create_directory_at(self.0.as_raw_fd(), component, false).map(Self)
        }

        pub(super) fn create_new_file(&self, file_name: &str) -> io::Result<File> {
            let file_name = c_component(file_name)?;
            // SAFETY: the parent descriptor is live and file_name is one valid component.
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    file_name.as_ptr(),
                    libc::O_RDWR
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            let fd = owned_fd(fd)?;
            require_regular_fd(fd.as_raw_fd())?;
            sync_fd(self.0.as_raw_fd())?;
            Ok(File::from(fd))
        }

        pub(super) fn open_and_lock_file(&self, file_name: &str) -> io::Result<File> {
            let file_name = c_component(file_name)?;
            // SAFETY: the retained directory and single component remain live for openat.
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    file_name.as_ptr(),
                    libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    0o600,
                )
            };
            let fd = owned_fd(fd)?;
            require_regular_fd(fd.as_raw_fd())?;
            // SAFETY: flock applies to this retained lock descriptor and releases on close.
            if unsafe { libc::flock(fd.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::WouldBlock {
                    return Err(io::Error::new(io::ErrorKind::WouldBlock, error));
                }
                return Err(error);
            }
            Ok(File::from(fd))
        }

        pub(super) fn open_existing_file(&self, file_name: &str) -> io::Result<File> {
            let file_name = c_component(file_name)?;
            // SAFETY: the parent descriptor is live and file_name is one valid component.
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    file_name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            let fd = owned_fd(fd)?;
            require_regular_fd(fd.as_raw_fd())?;
            Ok(File::from(fd))
        }

        pub(super) fn open_existing_deletable_file(&self, file_name: &str) -> io::Result<File> {
            self.open_existing_file(file_name)
        }

        pub(super) fn open_existing_executable(&self, file_name: &str) -> io::Result<File> {
            self.open_existing_file(file_name)
        }

        pub(super) fn verify_path(&self, path: &Path) -> io::Result<()> {
            let current = Self::open_root(path)?;
            require_same_node(self.0.as_raw_fd(), current.0.as_raw_fd(), "directory")
        }

        pub(super) fn verify_file(&self, file_name: &str, file: &File) -> io::Result<()> {
            let file_name = c_component(file_name)?;
            let current = open_regular_at(self.0.as_raw_fd(), &file_name)?;
            require_same_node(file.as_raw_fd(), current.as_raw_fd(), "artifact")
        }

        pub(super) fn publish_staged_file(
            &self,
            staged_file: &File,
            staged_name: &str,
            final_name: &str,
            mut verify_before_staged_unlink: impl FnMut() -> io::Result<()>,
        ) -> io::Result<()> {
            let staged_name = c_component(staged_name)?;
            let final_name = c_component(final_name)?;
            let named_staged = open_regular_at(self.0.as_raw_fd(), &staged_name)?;
            require_same_node(
                staged_file.as_raw_fd(),
                named_staged.as_raw_fd(),
                "staged artifact",
            )?;
            // SAFETY: both names are single components owned by the retained directory handle.
            if unsafe {
                libc::linkat(
                    self.0.as_raw_fd(),
                    staged_name.as_ptr(),
                    self.0.as_raw_fd(),
                    final_name.as_ptr(),
                    0,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            let named_final = open_regular_at(self.0.as_raw_fd(), &final_name)?;
            require_same_node(
                staged_file.as_raw_fd(),
                named_final.as_raw_fd(),
                "published artifact",
            )?;
            verify_before_staged_unlink()?;
            unlink_same_file(self.0.as_raw_fd(), &staged_name, staged_file.as_raw_fd())?;
            sync_fd(self.0.as_raw_fd())
        }

        pub(super) fn discard_staged_file(
            &self,
            staged_file: &File,
            staged_name: &str,
        ) -> io::Result<()> {
            let staged_name = c_component(staged_name)?;
            unlink_same_file(self.0.as_raw_fd(), &staged_name, staged_file.as_raw_fd())?;
            sync_fd(self.0.as_raw_fd())
        }

        pub(super) fn install_directory_no_replace(
            &self,
            staged_name: &str,
            final_name: &str,
        ) -> Result<(), P7DirectoryInstallError> {
            let staged_name =
                c_component(staged_name).map_err(P7DirectoryInstallError::before_commit)?;
            let final_name =
                c_component(final_name).map_err(P7DirectoryInstallError::before_commit)?;
            let staged = open_directory_at(
                self.0.as_raw_fd(),
                staged_name
                    .to_str()
                    .map_err(|_| invalid_input("invalid staged directory"))
                    .map_err(P7DirectoryInstallError::before_commit)?,
            )
            .map_err(P7DirectoryInstallError::before_commit)?;
            sync_fd(staged.as_raw_fd()).map_err(P7DirectoryInstallError::before_commit)?;
            #[cfg(target_os = "linux")]
            let status = unsafe {
                libc::syscall(
                    libc::SYS_renameat2,
                    self.0.as_raw_fd(),
                    staged_name.as_ptr(),
                    self.0.as_raw_fd(),
                    final_name.as_ptr(),
                    libc::RENAME_NOREPLACE,
                ) as libc::c_int
            };
            #[cfg(target_os = "macos")]
            let status = unsafe {
                libc::renameatx_np(
                    self.0.as_raw_fd(),
                    staged_name.as_ptr(),
                    self.0.as_raw_fd(),
                    final_name.as_ptr(),
                    libc::RENAME_EXCL,
                )
            };
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            let status = {
                return Err(P7DirectoryInstallError::before_commit(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "P7 atomic directory no-replace is unsupported on this Unix platform",
                )));
            };
            if status != 0 {
                return Err(P7DirectoryInstallError::before_commit(
                    io::Error::last_os_error(),
                ));
            }
            #[cfg(test)]
            if super::take_p7_directory_install_post_commit_failure() {
                return Err(P7DirectoryInstallError::after_commit(io::Error::other(
                    "injected P7 post-commit directory verification failure",
                )));
            }
            let installed = open_directory_at(
                self.0.as_raw_fd(),
                final_name
                    .to_str()
                    .map_err(|_| invalid_input("invalid final directory"))
                    .map_err(P7DirectoryInstallError::after_commit)?,
            )
            .map_err(P7DirectoryInstallError::after_commit)?;
            require_same_node(
                staged.as_raw_fd(),
                installed.as_raw_fd(),
                "published release directory",
            )
            .map_err(P7DirectoryInstallError::after_commit)?;
            sync_fd(installed.as_raw_fd()).map_err(P7DirectoryInstallError::after_commit)?;
            sync_fd(self.0.as_raw_fd()).map_err(P7DirectoryInstallError::after_commit)
        }

        pub(super) fn discard_empty_directory(
            &self,
            directory_name: &str,
            expected: &DirectoryHandle,
        ) -> io::Result<()> {
            let directory_name = c_component(directory_name)?;
            let named = open_directory_at(
                self.0.as_raw_fd(),
                directory_name
                    .to_str()
                    .map_err(|_| invalid_input("invalid cleanup directory"))?,
            )?;
            require_same_node(
                named.as_raw_fd(),
                expected.0.as_raw_fd(),
                "cleanup directory",
            )?;
            if unsafe {
                libc::unlinkat(
                    self.0.as_raw_fd(),
                    directory_name.as_ptr(),
                    libc::AT_REMOVEDIR,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            sync_fd(self.0.as_raw_fd())
        }
    }

    fn c_component(component: &str) -> io::Result<CString> {
        CString::new(component).map_err(|_| invalid_input("P7 component contains a NUL byte"))
    }

    fn owned_fd(fd: RawFd) -> io::Result<OwnedFd> {
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful open/openat returned a new descriptor owned by this function.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    fn open_directory_at(parent: RawFd, component: &str) -> io::Result<OwnedFd> {
        let component = c_component(component)?;
        // SAFETY: parent is live and component is one valid C path component.
        let fd = unsafe {
            libc::openat(
                parent,
                component.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        owned_fd(fd)
    }

    fn open_regular_at(parent: RawFd, component: &CString) -> io::Result<OwnedFd> {
        // SAFETY: parent is live and component is one valid C path component.
        let fd = unsafe {
            libc::openat(
                parent,
                component.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        let fd = owned_fd(fd)?;
        require_regular_fd(fd.as_raw_fd())?;
        Ok(fd)
    }

    fn unlink_same_file(parent: RawFd, component: &CString, expected: RawFd) -> io::Result<()> {
        let named = open_regular_at(parent, component)?;
        require_same_node(expected, named.as_raw_fd(), "staged artifact")?;
        // SAFETY: identity was checked through the retained parent immediately before unlinkat.
        if unsafe { libc::unlinkat(parent, component.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn create_directory_at(
        parent: RawFd,
        component: &str,
        allow_existing: bool,
    ) -> io::Result<OwnedFd> {
        let component_c = c_component(component)?;
        // SAFETY: parent is live and component is one valid C path component.
        let status = unsafe { libc::mkdirat(parent, component_c.as_ptr(), 0o700) };
        let created = status == 0;
        if !created {
            let error = io::Error::last_os_error();
            if !(allow_existing && error.kind() == io::ErrorKind::AlreadyExists) {
                return Err(error);
            }
        }
        let directory = open_directory_at(parent, component)?;
        if created {
            sync_fd(directory.as_raw_fd())?;
            sync_fd(parent)?;
        }
        Ok(directory)
    }

    fn sync_fd(fd: RawFd) -> io::Result<()> {
        // SAFETY: fd remains live for the duration of fsync.
        if unsafe { libc::fsync(fd) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn require_regular_fd(fd: RawFd) -> io::Result<()> {
        // SAFETY: fstat initializes the zeroed stat before it is inspected.
        let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
        // SAFETY: fd is live and stat points to writable storage.
        if unsafe { libc::fstat(fd, &mut stat) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(invalid_input("P7 cohort artifact must be a regular file"));
        }
        Ok(())
    }

    fn require_same_node(left: RawFd, right: RawFd, label: &str) -> io::Result<()> {
        // SAFETY: fstat initializes both stat values before they are inspected.
        let mut left_stat = unsafe { std::mem::zeroed::<libc::stat>() };
        let mut right_stat = unsafe { std::mem::zeroed::<libc::stat>() };
        if unsafe { libc::fstat(left, &mut left_stat) } != 0
            || unsafe { libc::fstat(right, &mut right_stat) } != 0
        {
            return Err(io::Error::last_os_error());
        }
        if left_stat.st_dev != right_stat.st_dev || left_stat.st_ino != right_stat.st_ino {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("P7 retained {label} handle differs from its directory entry"),
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use super::{invalid_input, validate_canonical_root, P7DirectoryInstallError};
    use std::{
        ffi::c_void,
        fs::File,
        io,
        mem::{offset_of, size_of, zeroed, MaybeUninit},
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        path::{Component, Path, PathBuf},
        ptr::{null, null_mut},
    };
    use windows_sys::{
        Wdk::{
            Foundation::OBJECT_ATTRIBUTES,
            Storage::FileSystem::{
                NtCreateFile, FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
                FILE_OPEN_IF, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
            },
        },
        Win32::{
            Foundation::{
                RtlNtStatusToDosError, HANDLE, INVALID_HANDLE_VALUE, OBJ_CASE_INSENSITIVE,
                OBJ_DONT_REPARSE, UNICODE_STRING,
            },
            Storage::FileSystem::{
                CreateFileW, FileBasicInfo, FileDispositionInfo, FileRenameInfo,
                GetFileInformationByHandleEx, SetFileInformationByHandle, DELETE,
                FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
                FILE_BASIC_INFO, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
                FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
                FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_SHARE_DELETE,
                FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, OPEN_EXISTING, SYNCHRONIZE,
            },
            System::IO::IO_STATUS_BLOCK,
        },
    };

    pub(super) struct DirectoryHandle(OwnedHandle);

    impl DirectoryHandle {
        pub(super) fn try_clone(&self) -> io::Result<Self> {
            Ok(Self(self.0.try_clone()?))
        }

        pub(super) fn verify_same_directory(&self, expected: &Self) -> io::Result<()> {
            require_same_node(
                self.0.as_raw_handle(),
                expected.0.as_raw_handle(),
                "bundle lock directory",
            )
        }

        pub(super) fn open_root(root: &Path) -> io::Result<Self> {
            validate_canonical_root(root)?;
            let mut anchor = PathBuf::new();
            let mut relative_components = Vec::new();
            for component in root.components() {
                match component {
                    Component::Prefix(prefix) => anchor.push(prefix.as_os_str()),
                    Component::RootDir => anchor.push(Path::new("\\")),
                    Component::Normal(component) => {
                        relative_components.push(component.to_str().ok_or_else(|| {
                            invalid_input("P7 secure root component must be valid UTF-8")
                        })?);
                    }
                    _ => {
                        return Err(invalid_input(
                            "P7 secure root must be an absolute normal path",
                        ))
                    }
                }
            }
            if !anchor.has_root() {
                return Err(invalid_input("P7 secure root has no fixed volume root"));
            }
            let mut path = anchor.as_os_str().encode_wide().collect::<Vec<_>>();
            path.push(0);
            // SAFETY: path is NUL-terminated and all pointer arguments remain live for the call.
            let handle = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: CreateFileW returned a new owned handle.
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            require_directory(handle.as_raw_handle())?;
            let mut current = handle;
            for component in relative_components {
                current = nt_open_directory(&current, component, FILE_OPEN)?;
            }
            Ok(Self(current))
        }

        pub(super) fn open_directory(&self, component: &str) -> io::Result<Self> {
            nt_open_directory(&self.0, component, FILE_OPEN).map(Self)
        }

        pub(super) fn open_or_create_directory(&self, component: &str) -> io::Result<Self> {
            nt_open_directory(&self.0, component, FILE_OPEN_IF).map(Self)
        }

        pub(super) fn create_directory(&self, component: &str) -> io::Result<Self> {
            nt_open_directory(&self.0, component, FILE_CREATE).map(Self)
        }

        pub(super) fn create_new_file(&self, file_name: &str) -> io::Result<File> {
            let handle = nt_create_relative(
                &self.0,
                file_name,
                FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | SYNCHRONIZE,
                FILE_CREATE,
                FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            )?;
            require_regular_file(handle.as_raw_handle())?;
            Ok(File::from(handle))
        }

        pub(super) fn open_and_lock_file(&self, file_name: &str) -> io::Result<File> {
            use windows_sys::Win32::{
                Foundation::ERROR_LOCK_VIOLATION,
                Storage::FileSystem::{
                    LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
                },
            };

            let handle = nt_create_relative(
                &self.0,
                file_name,
                FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | SYNCHRONIZE,
                FILE_OPEN_IF,
                FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            )?;
            require_regular_file(handle.as_raw_handle())?;
            let mut overlapped = unsafe { zeroed() };
            // SAFETY: the retained handle and OVERLAPPED buffer remain live for the lock call.
            if unsafe {
                LockFileEx(
                    handle.as_raw_handle(),
                    LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                    0,
                    u32::MAX,
                    u32::MAX,
                    &mut overlapped,
                )
            } == 0
            {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) {
                    return Err(io::Error::new(io::ErrorKind::WouldBlock, error));
                }
                return Err(error);
            }
            Ok(File::from(handle))
        }

        pub(super) fn publish_staged_file(
            &self,
            staged_file: &File,
            _staged_name: &str,
            final_name: &str,
            _verify_before_staged_unlink: impl FnMut() -> io::Result<()>,
        ) -> io::Result<()> {
            let final_name = final_name.encode_utf16().collect::<Vec<_>>();
            let name_bytes = final_name
                .len()
                .checked_mul(size_of::<u16>())
                .ok_or_else(|| invalid_input("P7 secure path component is too long"))?;
            let total_bytes = offset_of!(FILE_RENAME_INFO, FileName)
                .checked_add(name_bytes)
                .ok_or_else(|| invalid_input("P7 secure path component is too long"))?;
            let word_count = total_bytes.div_ceil(size_of::<usize>());
            let mut storage = vec![0_usize; word_count];
            let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            // SAFETY: storage is aligned for FILE_RENAME_INFO and sized for the complete name.
            unsafe {
                (*info).Anonymous.ReplaceIfExists = false;
                (*info).RootDirectory = self.0.as_raw_handle();
                (*info).FileNameLength = u32::try_from(name_bytes)
                    .map_err(|_| invalid_input("P7 secure path component is too long"))?;
                std::ptr::copy_nonoverlapping(
                    final_name.as_ptr(),
                    (*info).FileName.as_mut_ptr(),
                    final_name.len(),
                );
            }
            // SAFETY: staged_file and directory handles are live and info owns the full buffer.
            let status = unsafe {
                SetFileInformationByHandle(
                    staged_file.as_raw_handle(),
                    FileRenameInfo,
                    info.cast(),
                    u32::try_from(total_bytes)
                        .map_err(|_| invalid_input("P7 rename buffer is too long"))?,
                )
            };
            if status == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub(super) fn discard_staged_file(
            &self,
            staged_file: &File,
            _staged_name: &str,
        ) -> io::Result<()> {
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: staged_file is live and disposition has the declared layout and size.
            let status = unsafe {
                SetFileInformationByHandle(
                    staged_file.as_raw_handle(),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            };
            if status == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub(super) fn install_directory_no_replace(
            &self,
            _staged_name: &str,
            _final_name: &str,
        ) -> Result<(), P7DirectoryInstallError> {
            Err(P7DirectoryInstallError::before_commit(io::Error::new(
                io::ErrorKind::Unsupported,
                "P7 atomic retained directory publication is unsupported on Windows",
            )))
        }

        pub(super) fn discard_empty_directory(
            &self,
            _directory_name: &str,
            _expected: &DirectoryHandle,
        ) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "P7 retained directory cleanup is unsupported on Windows",
            ))
        }

        pub(super) fn open_existing_file(&self, file_name: &str) -> io::Result<File> {
            let handle = nt_create_relative(
                &self.0,
                file_name,
                FILE_GENERIC_READ | SYNCHRONIZE,
                FILE_OPEN,
                FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            )?;
            require_regular_file(handle.as_raw_handle())?;
            Ok(File::from(handle))
        }

        pub(super) fn open_existing_deletable_file(&self, file_name: &str) -> io::Result<File> {
            let handle = nt_create_relative(
                &self.0,
                file_name,
                FILE_GENERIC_READ | DELETE | SYNCHRONIZE,
                FILE_OPEN,
                FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            )?;
            require_regular_file(handle.as_raw_handle())?;
            Ok(File::from(handle))
        }

        pub(super) fn open_existing_executable(&self, file_name: &str) -> io::Result<File> {
            let handle = nt_create_relative_with_share(
                &self.0,
                file_name,
                FILE_GENERIC_READ | SYNCHRONIZE,
                FILE_OPEN,
                FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
                FILE_SHARE_READ,
            )?;
            require_regular_file(handle.as_raw_handle())?;
            Ok(File::from(handle))
        }

        pub(super) fn verify_path(&self, path: &Path) -> io::Result<()> {
            let current = Self::open_root(path)?;
            require_same_node(
                self.0.as_raw_handle(),
                current.0.as_raw_handle(),
                "directory",
            )
        }

        pub(super) fn verify_file(&self, file_name: &str, file: &File) -> io::Result<()> {
            let current = nt_create_relative(
                &self.0,
                file_name,
                FILE_GENERIC_READ | SYNCHRONIZE,
                FILE_OPEN,
                FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            )?;
            require_regular_file(current.as_raw_handle())?;
            require_same_node(file.as_raw_handle(), current.as_raw_handle(), "artifact")
        }
    }

    fn nt_open_directory(
        parent: &OwnedHandle,
        component: &str,
        disposition: u32,
    ) -> io::Result<OwnedHandle> {
        let handle = nt_create_relative(
            parent,
            component,
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            disposition,
            FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        )?;
        require_directory(handle.as_raw_handle())?;
        Ok(handle)
    }

    fn nt_create_relative(
        parent: &OwnedHandle,
        component: &str,
        desired_access: u32,
        disposition: u32,
        create_options: u32,
    ) -> io::Result<OwnedHandle> {
        nt_create_relative_with_share(
            parent,
            component,
            desired_access,
            disposition,
            create_options,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
    }

    fn nt_create_relative_with_share(
        parent: &OwnedHandle,
        component: &str,
        desired_access: u32,
        disposition: u32,
        create_options: u32,
        share_access: u32,
    ) -> io::Result<OwnedHandle> {
        let mut name = component.encode_utf16().collect::<Vec<_>>();
        let byte_len = name
            .len()
            .checked_mul(size_of::<u16>())
            .and_then(|length| u16::try_from(length).ok())
            .ok_or_else(|| invalid_input("P7 secure path component is too long"))?;
        let mut unicode_name = UNICODE_STRING {
            Length: byte_len,
            MaximumLength: byte_len,
            Buffer: name.as_mut_ptr(),
        };
        let object_attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: parent.as_raw_handle(),
            ObjectName: &mut unicode_name,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            SecurityDescriptor: null_mut(),
            SecurityQualityOfService: null_mut(),
        };
        let mut status_block = MaybeUninit::<IO_STATUS_BLOCK>::uninit();
        let mut handle = MaybeUninit::<HANDLE>::uninit();
        // SAFETY: RootDirectory and all pointed-to structures remain live through NtCreateFile.
        let status = unsafe {
            NtCreateFile(
                handle.as_mut_ptr(),
                desired_access,
                &object_attributes,
                status_block.as_mut_ptr(),
                null(),
                FILE_ATTRIBUTE_NORMAL,
                share_access,
                disposition,
                create_options,
                null(),
                0,
            )
        };
        if status < 0 {
            // SAFETY: RtlNtStatusToDosError is a pure status-code conversion.
            let error = unsafe { RtlNtStatusToDosError(status) };
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        // SAFETY: successful NtCreateFile initialized a new owned handle.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle.assume_init()) })
    }

    fn basic_info(handle: *mut c_void) -> io::Result<FILE_BASIC_INFO> {
        // SAFETY: GetFileInformationByHandleEx initializes info on success.
        let mut info = unsafe { zeroed::<FILE_BASIC_INFO>() };
        // SAFETY: handle is live and info is valid writable storage of the declared size.
        let status = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileBasicInfo,
                (&mut info as *mut FILE_BASIC_INFO).cast(),
                size_of::<FILE_BASIC_INFO>() as u32,
            )
        };
        if status == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info)
    }

    fn require_directory(handle: *mut c_void) -> io::Result<()> {
        let attributes = basic_info(handle)?.FileAttributes;
        if attributes & FILE_ATTRIBUTE_DIRECTORY == 0
            || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(invalid_input(
                "P7 directory owner must not be a reparse point",
            ));
        }
        Ok(())
    }

    fn require_regular_file(handle: *mut c_void) -> io::Result<()> {
        let attributes = basic_info(handle)?.FileAttributes;
        if attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
            return Err(invalid_input("P7 cohort artifact must be a regular file"));
        }
        Ok(())
    }

    fn require_same_node(left: *mut c_void, right: *mut c_void, label: &str) -> io::Result<()> {
        use windows_sys::Win32::Storage::FileSystem::{FileIdInfo, FILE_ID_INFO};

        fn identity(handle: *mut c_void) -> io::Result<FILE_ID_INFO> {
            let mut info = MaybeUninit::<FILE_ID_INFO>::uninit();
            // SAFETY: handle is live and info has the exact layout requested by FileIdInfo.
            let status = unsafe {
                GetFileInformationByHandleEx(
                    handle,
                    FileIdInfo,
                    info.as_mut_ptr().cast(),
                    size_of::<FILE_ID_INFO>() as u32,
                )
            };
            if status == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: successful GetFileInformationByHandleEx initialized info.
            Ok(unsafe { info.assume_init() })
        }

        let left = identity(left)?;
        let right = identity(right)?;
        if left.VolumeSerialNumber != right.VolumeSerialNumber
            || left.FileId.Identifier != right.FileId.Identifier
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("P7 retained {label} handle differs from its directory entry"),
            ));
        }
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
compile_error!("P7 secure cohort ownership requires Unix or Windows handle-relative APIs");

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn p7_retained_launcher_replaces_reserved_execution_authority_environment() {
        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical test executable");
        let output = Command::new(&executable)
            .args([
                "p7_secure_fs::tests::p7_reserved_environment_replacement_worker",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .env(P7_RETAINED_EXECUTABLE_FD_ENV, "999999")
            .env(P7_RETAINED_EXECUTABLE_PATH_ENV, "/forged/publisher")
            .env(P7_RETAINED_EXECUTABLE_SHA256_ENV, "0".repeat(64))
            .output()
            .expect("run retained environment replacement worker");
        assert!(
            output.status.success(),
            "status={:?} stdout={} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn p7_concurrent_execution_authority_claim_is_serialized_and_one_time() {
        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical test executable");
        let output = Command::new(&executable)
            .args([
                "p7_secure_fs::tests::p7_concurrent_execution_authority_worker",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .output()
            .expect("run concurrent authority worker");
        assert!(
            output.status.success(),
            "status={:?} stdout={} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "subprocess worker"]
    fn p7_concurrent_execution_authority_worker() {
        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical test executable");
        let mut retained =
            P7RetainedFile::open_executable(&executable).expect("retain concurrent worker");
        let args = vec![
            "p7_secure_fs::tests::p7_concurrent_execution_authority_child".to_string(),
            "--exact".to_string(),
            "--ignored".to_string(),
            "--nocapture".to_string(),
        ];
        let (mut command, guard, _) = retained
            .executable_command(&args)
            .expect("build concurrent sealed child command");
        let output = command.output().expect("run concurrent sealed child");
        drop(guard);
        assert!(
            output.status.success(),
            "status={:?} stdout={} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "subprocess child"]
    fn p7_concurrent_execution_authority_child() {
        use std::sync::{Arc, Barrier};

        let inherited_fd = std::env::var(P7_RETAINED_EXECUTABLE_FD_ENV)
            .expect("concurrent inherited FD")
            .parse::<i32>()
            .expect("numeric concurrent inherited FD");
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                P7ProcessExecutionAuthority::claim()
            }));
        }
        barrier.wait();

        let mut admitted = None;
        let mut rejected = Vec::new();
        for worker in workers {
            match worker.join().expect("authority claim thread") {
                Ok(authority) => {
                    assert!(
                        admitted.replace(authority).is_none(),
                        "two claims succeeded"
                    )
                }
                Err(error) => rejected.push(error),
            }
        }
        let mut admitted = admitted.expect("exactly one concurrent claim must succeed");
        assert_eq!(rejected.len(), 1, "exactly one claim must fail");
        assert!(rejected[0].to_string().contains("already consumed"));
        admitted
            .verify_retained()
            .expect("winning authority remains valid");
        assert_inherited_authority_revoked(inherited_fd);
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "subprocess worker"]
    fn p7_reserved_environment_replacement_worker() {
        assert_eq!(
            std::env::var(P7_RETAINED_EXECUTABLE_FD_ENV).as_deref(),
            Ok("999999")
        );
        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical test executable");
        let mut retained = P7RetainedFile::open_executable(&executable)
            .expect("retain environment replacement worker");
        let args = vec![
            "p7_secure_fs::tests::p7_reserved_environment_replacement_child".to_string(),
            "--exact".to_string(),
            "--ignored".to_string(),
            "--nocapture".to_string(),
        ];
        let (mut command, guard, _) = retained
            .executable_command(&args)
            .expect("build sealed child command");
        let output = command.output().expect("run sealed child command");
        drop(guard);
        assert!(
            output.status.success(),
            "status={:?} stdout={} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "subprocess child"]
    fn p7_reserved_environment_replacement_child() {
        let raw_environment = fs::read("/proc/self/environ").expect("read child environment");
        for key in P7_RETAINED_EXECUTABLE_AUTHORITY_ENV {
            let prefix = format!("{key}=").into_bytes();
            let count = raw_environment
                .split(|byte| *byte == 0)
                .filter(|field| field.starts_with(&prefix))
                .count();
            assert_eq!(count, 1, "reserved authority key {key} must be unique");
        }
        assert_ne!(
            std::env::var(P7_RETAINED_EXECUTABLE_FD_ENV).as_deref(),
            Ok("999999")
        );
        assert_ne!(
            std::env::var(P7_RETAINED_EXECUTABLE_PATH_ENV).as_deref(),
            Ok("/forged/publisher")
        );
        assert_ne!(
            std::env::var(P7_RETAINED_EXECUTABLE_SHA256_ENV).as_deref(),
            Ok("0000000000000000000000000000000000000000000000000000000000000000")
        );
        let expected_sha256 =
            std::env::var(P7_RETAINED_EXECUTABLE_SHA256_ENV).expect("sealed child SHA256");
        let inherited_fd = std::env::var(P7_RETAINED_EXECUTABLE_FD_ENV)
            .expect("sealed child inherited FD")
            .parse::<i32>()
            .expect("numeric sealed child inherited FD");
        let mut authority = P7RetainedFile::inherited_execution_authority()
            .expect("sealed child authority must validate");
        assert_inherited_authority_revoked(inherited_fd);
        let identity = authority
            .verify()
            .expect("sealed child bytes must match authority");
        assert_eq!(identity.sha256, expected_sha256);
        std::env::set_var(P7_RETAINED_EXECUTABLE_FD_ENV, "999999");
        std::env::set_var(P7_RETAINED_EXECUTABLE_PATH_ENV, "/forged/repeated");
        std::env::set_var(P7_RETAINED_EXECUTABLE_SHA256_ENV, "f".repeat(64));
        let repeated = P7RetainedFile::inherited_execution_authority()
            .err()
            .expect("execution authority must be one-time");
        assert!(repeated.to_string().contains("already consumed"));
        for key in P7_RETAINED_EXECUTABLE_AUTHORITY_ENV {
            assert!(
                std::env::var_os(key).is_none(),
                "repeated claim must clear reserved environment key {key}"
            );
        }
    }

    #[test]
    fn p7_inherited_execution_authority_rejects_partial_seals_and_wrong_sha() {
        let required = p7_required_execution_seals();
        for missing in [
            libc::F_SEAL_WRITE,
            libc::F_SEAL_GROW,
            libc::F_SEAL_SHRINK,
            libc::F_SEAL_SEAL,
        ] {
            assert!(require_p7_execution_seals(required & !missing).is_err());
        }

        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical test executable");
        for worker in [
            "p7_secure_fs::tests::p7_partial_seal_execution_authority_worker",
            "p7_secure_fs::tests::p7_wrong_sha_execution_authority_worker",
        ] {
            let output = Command::new(&executable)
                .args([worker, "--exact", "--ignored", "--nocapture"])
                .output()
                .expect("run malformed authority worker");
            assert!(
                output.status.success(),
                "worker={worker} status={:?} stdout={} stderr={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    #[ignore = "subprocess worker"]
    fn p7_partial_seal_execution_authority_worker() {
        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical test executable");
        let mut retained =
            P7RetainedFile::open_executable(&executable).expect("retain partial-seal worker");
        let args = vec![
            "p7_secure_fs::tests::p7_partial_seal_execution_authority_child".to_string(),
            "--exact".to_string(),
            "--ignored".to_string(),
            "--nocapture".to_string(),
        ];
        let partial_seals = p7_required_execution_seals() & !libc::F_SEAL_SEAL;
        let (mut command, guard, _) = retained
            .executable_command_with_test_seals(&args, partial_seals)
            .expect("build partial-seal child command");
        let output = command.output().expect("run partial-seal child");
        drop(guard);
        assert!(
            output.status.success(),
            "status={:?} stdout={} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "subprocess child"]
    fn p7_partial_seal_execution_authority_child() {
        let inherited_fd = std::env::var(P7_RETAINED_EXECUTABLE_FD_ENV)
            .expect("partial-seal inherited FD")
            .parse::<i32>()
            .expect("numeric partial-seal inherited FD");
        let error = P7RetainedFile::inherited_execution_authority()
            .err()
            .expect("partial-sealed current execution object must fail closed");
        assert!(
            error.to_string().contains("missing required seals"),
            "{error:?}"
        );
        assert_inherited_authority_revoked(inherited_fd);
    }

    #[test]
    #[ignore = "subprocess worker"]
    fn p7_wrong_sha_execution_authority_worker() {
        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical test executable");
        let mut retained =
            P7RetainedFile::open_executable(&executable).expect("retain wrong-SHA worker");
        let args = vec![
            "p7_secure_fs::tests::p7_wrong_sha_execution_authority_child".to_string(),
            "--exact".to_string(),
            "--ignored".to_string(),
            "--nocapture".to_string(),
        ];
        let (mut command, guard, _) = retained
            .executable_command(&args)
            .expect("build wrong-SHA sealed child command");
        let output = command.output().expect("run wrong-SHA sealed child");
        drop(guard);
        assert!(
            output.status.success(),
            "status={:?} stdout={} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "subprocess child"]
    fn p7_wrong_sha_execution_authority_child() {
        let inherited_fd = std::env::var(P7_RETAINED_EXECUTABLE_FD_ENV)
            .expect("wrong-SHA inherited FD")
            .parse::<i32>()
            .expect("numeric wrong-SHA inherited FD");
        std::env::set_var(P7_RETAINED_EXECUTABLE_SHA256_ENV, "0".repeat(64));
        let mut authority = P7RetainedFile::inherited_execution_authority()
            .expect("current sealed execution authority");
        let error = authority
            .verify()
            .expect_err("wrong launcher SHA must fail closed");
        assert!(error.to_string().contains("sealed identity"), "{error:?}");
        assert_inherited_authority_revoked(inherited_fd);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn p7_inherited_execution_authority_rejects_direct_path_and_foreign_fd() {
        use std::os::fd::AsRawFd;

        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical test executable");
        let direct_file = fs::File::open(&executable).expect("open direct executable");
        let direct_fd =
            duplicate_inheritable_file(&direct_file).expect("inherit direct executable");
        assert_authority_rejected_by_direct_child(
            &executable,
            direct_fd.as_raw_fd(),
            &"0".repeat(64),
            "not a sealed memfd",
        );
        assert_authority_rejected_by_direct_child(
            &executable,
            direct_fd.as_raw_fd(),
            "invalid",
            "SHA256 is invalid",
        );

        let foreign_memfd = sealed_test_memfd(&executable);
        let foreign_fd =
            duplicate_inheritable_file(&foreign_memfd).expect("inherit foreign sealed memfd");
        assert_authority_rejected_by_direct_child(
            &executable,
            foreign_fd.as_raw_fd(),
            &"0".repeat(64),
            "not the current execution object",
        );
    }

    #[cfg(target_os = "linux")]
    fn assert_authority_rejected_by_direct_child(
        executable: &Path,
        inherited_fd: i32,
        sha256: &str,
        expected_error: &str,
    ) {
        let output = Command::new(executable)
            .args([
                "p7_secure_fs::tests::p7_direct_execution_authority_rejection_child",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .env(P7_RETAINED_EXECUTABLE_FD_ENV, inherited_fd.to_string())
            .env(P7_RETAINED_EXECUTABLE_PATH_ENV, executable)
            .env(P7_RETAINED_EXECUTABLE_SHA256_ENV, sha256)
            .env("BM_P7_EXPECTED_AUTHORITY_ERROR", expected_error)
            .output()
            .expect("run direct authority rejection child");
        assert!(
            output.status.success(),
            "status={:?} stdout={} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "subprocess child"]
    fn p7_direct_execution_authority_rejection_child() {
        let inherited_fd = std::env::var(P7_RETAINED_EXECUTABLE_FD_ENV)
            .expect("direct inherited FD")
            .parse::<i32>()
            .expect("numeric direct inherited FD");
        let expected = std::env::var("BM_P7_EXPECTED_AUTHORITY_ERROR")
            .expect("expected authority error fragment");
        let error = P7RetainedFile::inherited_execution_authority()
            .err()
            .expect("direct execution authority must fail closed");
        assert!(
            error.to_string().contains(&expected),
            "error={error:?} expected={expected}"
        );
        assert_inherited_authority_revoked(inherited_fd);
    }

    fn assert_inherited_authority_revoked(inherited_fd: i32) {
        for key in P7_RETAINED_EXECUTABLE_AUTHORITY_ENV {
            assert!(
                std::env::var_os(key).is_none(),
                "failed authority must clear reserved environment key {key}"
            );
        }
        // SAFETY: F_GETFD only probes whether the inherited descriptor remains open.
        assert_eq!(unsafe { libc::fcntl(inherited_fd, libc::F_GETFD) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    }

    #[cfg(target_os = "linux")]
    fn sealed_test_memfd(source_path: &Path) -> fs::File {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

        let name = CString::new("bm-p7-foreign-test").expect("static memfd name");
        // SAFETY: name is a valid C string and the returned descriptor is uniquely owned.
        let raw = unsafe {
            libc::syscall(
                libc::SYS_memfd_create,
                name.as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
            ) as libc::c_int
        };
        assert!(
            raw >= 0,
            "create foreign memfd: {}",
            io::Error::last_os_error()
        );
        // SAFETY: successful memfd_create returned a new owned descriptor.
        let owned = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut target = fs::File::from(owned);
        let mut source = fs::File::open(source_path).expect("open foreign memfd source");
        io::copy(&mut source, &mut target).expect("copy foreign memfd source");
        target.sync_all().expect("sync foreign memfd");
        // SAFETY: target is a live anonymous regular file descriptor owned by this test.
        assert_eq!(unsafe { libc::fchmod(target.as_raw_fd(), 0o500) }, 0);
        let seals =
            libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
        // SAFETY: F_ADD_SEALS applies to the live test memfd.
        assert_eq!(
            unsafe { libc::fcntl(target.as_raw_fd(), libc::F_ADD_SEALS, seals) },
            0
        );
        target
    }
}
