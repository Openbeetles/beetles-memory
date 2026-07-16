use std::{
    fmt, fs, io,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
};

use sha2::{Digest, Sha256};

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
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

    pub fn open_or_create_directory(&self, component: &str) -> io::Result<Self> {
        validate_component(component)?;
        Ok(Self {
            path: self.path.join(component),
            directory: self.directory.open_or_create_directory(component)?,
        })
    }

    pub fn create_directory(&self, component: &str) -> io::Result<Self> {
        validate_component(component)?;
        Ok(Self {
            path: self.path.join(component),
            directory: self.directory.create_directory(component)?,
        })
    }

    pub fn create_new_file(&self, file_name: &str) -> io::Result<fs::File> {
        validate_component(file_name)?;
        self.directory.create_new_file(file_name)
    }

    pub fn lock_bundle(&self, lock_name: &str) -> io::Result<P7BundleWriteGuard> {
        validate_component(lock_name)?;
        let lock = self.directory.open_and_lock_file(lock_name)?;
        Ok(P7BundleWriteGuard { _lock: lock })
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

    pub(crate) fn clone_file(&self) -> io::Result<fs::File> {
        self.file.try_clone()
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
        let seals =
            libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
        // SAFETY: F_ADD_SEALS applies to the live memfd and permanently removes mutation rights.
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
            .filter(|(name, _)| {
                name != "BM_P7_RETAINED_EXECUTABLE_SHA256"
                    && name != "BM_P7_RETAINED_EXECUTABLE_PATH"
            })
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
                "BM_P7_RETAINED_EXECUTABLE_SHA256={}",
                launch_identity.sha256
            ))
            .expect("SHA256 environment has no NUL"),
        );
        environment.push(
            CString::new(format!(
                "BM_P7_RETAINED_EXECUTABLE_PATH={}",
                self.path.display()
            ))
            .map_err(|_| invalid_input("P7 executable path contains NUL"))?,
        );
        let inherited = duplicate_inheritable_file(&target)?;
        environment.push(
            CString::new(format!(
                "BM_P7_RETAINED_EXECUTABLE_FD={}",
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
                capsule_directory: None,
            },
            launch_identity,
        ))
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
        args: &[String],
    ) -> io::Result<(Command, P7ExecutableLaunchGuard, P7ContentIdentity)> {
        let launch_identity = self.hash_for_launch()?;
        let mut command = Command::new(&self.path);
        command.args(args);
        command.env("BM_P7_RETAINED_EXECUTABLE_HELD", "1");
        command.env("BM_P7_RETAINED_EXECUTABLE_SHA256", &launch_identity.sha256);
        command.env("BM_P7_RETAINED_EXECUTABLE_PATH", &self.path);
        Ok((
            command,
            P7ExecutableLaunchGuard {
                files: vec![self.file.try_clone()?],
            },
            launch_identity,
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

pub struct P7CohortArtifactOwner {
    retained: P7RetainedDirectoryOwner,
}

pub struct P7BundleWriteGuard {
    _lock: fs::File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P7ArtifactPublishOutcome {
    Published,
    ReusedIdentical,
}

impl P7CohortArtifactOwner {
    fn new(path: PathBuf, directory: platform::DirectoryHandle) -> Self {
        Self {
            retained: P7RetainedDirectoryOwner { path, directory },
        }
    }

    pub fn path(&self) -> &Path {
        self.retained.path()
    }

    pub fn display(&self) -> impl fmt::Display + '_ {
        self.path().display()
    }

    pub fn open_or_create_directory(&self, component: &str) -> io::Result<Self> {
        validate_component(component)?;
        let directory = self
            .retained
            .directory
            .open_or_create_directory(component)?;
        Ok(Self::new(self.path().join(component), directory))
    }

    pub fn create_directory(&self, component: &str) -> io::Result<Self> {
        validate_component(component)?;
        let directory = self.retained.directory.create_directory(component)?;
        Ok(Self::new(self.path().join(component), directory))
    }

    pub fn create_new_file(&self, file_name: &str) -> io::Result<fs::File> {
        validate_component(file_name)?;
        self.retained.directory.create_new_file(file_name)
    }

    pub fn lock_bundle(&self, lock_name: &str) -> io::Result<P7BundleWriteGuard> {
        validate_component(lock_name)?;
        let lock = self.retained.directory.open_and_lock_file(lock_name)?;
        Ok(P7BundleWriteGuard { _lock: lock })
    }

    pub fn discard_uncommitted_file(&self, file_name: &str) -> io::Result<bool> {
        validate_component(file_name)?;
        let Some(file) = self.try_open_existing_file(file_name)? else {
            return Ok(false);
        };
        self.retained
            .directory
            .discard_staged_file(&file, file_name)?;
        Ok(true)
    }

    pub fn open_existing_file(&self, file_name: &str) -> io::Result<fs::File> {
        validate_component(file_name)?;
        self.retained.open_existing_file(file_name)
    }

    pub fn try_open_existing_file(&self, file_name: &str) -> io::Result<Option<fs::File>> {
        self.retained.try_open_existing_file(file_name)
    }

    pub fn verify_existing_file(&self, file_name: &str, file: &fs::File) -> io::Result<()> {
        self.retained.verify_file_identity(file_name, file)
    }

    pub fn publish_staged_file(
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
        match self
            .retained
            .directory
            .publish_staged_file(&staged_file, staged_name, final_name)
        {
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

    pub fn publish_immutable_bytes(
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

    pub fn install_staged_directory(&self, staged_name: &str, final_name: &str) -> io::Result<()> {
        validate_component(staged_name)?;
        validate_component(final_name)?;
        if staged_name == final_name {
            return Err(invalid_input(
                "P7 staged and final directory names must differ",
            ));
        }
        self.retained
            .directory
            .install_directory_no_replace(staged_name, final_name)
    }

    pub fn discard_empty_directory(
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

pub fn initialize_p7_cohort(root: &Path, run_id: &str) -> io::Result<P7CohortArtifactOwner> {
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

pub fn open_p7_cohort_artifact_owner(
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

pub fn open_or_create_p7_release_store(root: &Path) -> io::Result<P7CohortArtifactOwner> {
    let root_owner = platform::DirectoryHandle::open_root(root)?;
    let runner = root_owner.open_or_create_directory("runner")?;
    let releases = runner.open_or_create_directory("releases")?;
    Ok(P7CohortArtifactOwner::new(
        root.join("runner/releases"),
        releases,
    ))
}

pub fn open_or_create_p7_verifier_release_store(root: &Path) -> io::Result<P7CohortArtifactOwner> {
    let root_owner = platform::DirectoryHandle::open_root(root)?;
    let verifier = root_owner.open_or_create_directory("verifier")?;
    let releases = verifier.open_or_create_directory("releases")?;
    Ok(P7CohortArtifactOwner::new(
        root.join("verifier/releases"),
        releases,
    ))
}

#[cfg(unix)]
mod platform {
    use super::{invalid_input, validate_canonical_root};
    use std::{
        ffi::CString,
        fs::File,
        io,
        os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        path::{Component, Path},
    };

    pub(super) struct DirectoryHandle(OwnedFd);

    impl DirectoryHandle {
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
            if let Err(error) = require_same_node(
                staged_file.as_raw_fd(),
                named_final.as_raw_fd(),
                "published artifact",
            ) {
                // The link just created is the only final entry this call may remove.
                unsafe {
                    libc::unlinkat(self.0.as_raw_fd(), final_name.as_ptr(), 0);
                }
                return Err(error);
            }
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
        ) -> io::Result<()> {
            let staged_name = c_component(staged_name)?;
            let final_name = c_component(final_name)?;
            let staged = open_directory_at(
                self.0.as_raw_fd(),
                staged_name
                    .to_str()
                    .map_err(|_| invalid_input("invalid staged directory"))?,
            )?;
            sync_fd(staged.as_raw_fd())?;
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
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "P7 atomic directory no-replace is unsupported on this Unix platform",
                ));
            };
            if status != 0 {
                return Err(io::Error::last_os_error());
            }
            let installed = open_directory_at(
                self.0.as_raw_fd(),
                final_name
                    .to_str()
                    .map_err(|_| invalid_input("invalid final directory"))?,
            )?;
            require_same_node(
                staged.as_raw_fd(),
                installed.as_raw_fd(),
                "published release directory",
            )?;
            sync_fd(installed.as_raw_fd())?;
            sync_fd(self.0.as_raw_fd())
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
    use super::{invalid_input, validate_canonical_root};
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
        ) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "P7 atomic retained directory publication is unsupported on Windows",
            ))
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
