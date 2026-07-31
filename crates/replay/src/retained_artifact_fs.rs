//! Generation-neutral retained artifact filesystem primitives.
//!
//! This module owns only stable directory/file handles, exact same-node discard, and terminal
//! no-replace publication. It contains no P7/P8 schema, cohort, release, or benchmark semantics.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn validate_component(component: &str) -> io::Result<()> {
    let mut components = Path::new(component).components();
    if component.is_empty()
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(invalid_input(
            "retained artifact name must be one non-empty path component",
        ));
    }
    Ok(())
}

fn validate_canonical_root(root: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid_input(
            "retained artifact root must be a real directory",
        ));
    }
    if fs::canonicalize(root)? != root {
        return Err(invalid_input("retained artifact root must be canonical"));
    }
    Ok(())
}

pub(crate) struct RetainedArtifactDirectory {
    path: PathBuf,
    directory: platform::DirectoryHandle,
}

impl RetainedArtifactDirectory {
    pub(crate) fn open_root(path: &Path) -> io::Result<Self> {
        validate_canonical_root(path)?;
        let directory = platform::DirectoryHandle::open_root(path)?;
        let value = Self {
            path: path.to_path_buf(),
            directory,
        };
        value.verify_unchanged()?;
        Ok(value)
    }

    #[cfg(unix)]
    pub(crate) fn from_retained_directory_file(path: &Path, file: File) -> io::Result<Self> {
        validate_canonical_root(path)?;
        let directory = platform::DirectoryHandle::from_file(file)?;
        let value = Self {
            path: path.to_path_buf(),
            directory,
        };
        value.verify_unchanged()?;
        Ok(value)
    }

    #[cfg(unix)]
    pub(crate) fn try_clone_directory_file(&self) -> io::Result<File> {
        self.verify_unchanged()?;
        self.directory.try_clone_file()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn inheritable_directory_file(&self) -> io::Result<File> {
        use std::os::fd::AsRawFd as _;

        let file = self.try_clone_directory_file()?;
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
        if flags < 0
            || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, flags & !libc::FD_CLOEXEC) }
                != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(file)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn retained_observation_path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.directory.raw_fd()))
    }

    #[cfg(unix)]
    pub(crate) fn unix_physical_identity(&self) -> io::Result<(u64, u64)> {
        self.verify_unchanged()?;
        self.directory.physical_identity()
    }

    #[cfg(not(unix))]
    pub(crate) fn unix_physical_identity(&self) -> io::Result<(u64, u64)> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unix retained directory identity is unavailable",
        ))
    }

    pub(crate) fn open_existing_read_stable_file(&self, file_name: &str) -> io::Result<File> {
        validate_component(file_name)?;
        self.verify_unchanged()?;
        let file = self.directory.open_existing_read_stable_file(file_name)?;
        self.directory.verify_file(file_name, &file)?;
        self.verify_unchanged()?;
        Ok(file)
    }

    pub(crate) fn open_existing_terminal_stage(&self, file_name: &str) -> io::Result<File> {
        validate_component(file_name)?;
        self.verify_unchanged()?;
        let file = self.directory.open_existing_terminal_stage(file_name)?;
        self.directory.verify_file(file_name, &file)?;
        self.verify_unchanged()?;
        Ok(file)
    }

    pub(crate) fn create_new_terminal_stage(&self, file_name: &str) -> io::Result<File> {
        validate_component(file_name)?;
        self.verify_unchanged()?;
        let file = self.directory.create_new_terminal_stage(file_name)?;
        self.directory.verify_file(file_name, &file)?;
        self.verify_unchanged()?;
        Ok(file)
    }

    pub(crate) fn verify_file_identity(&self, file_name: &str, file: &File) -> io::Result<()> {
        validate_component(file_name)?;
        self.verify_unchanged()?;
        self.directory.verify_file(file_name, file)?;
        self.verify_unchanged()
    }

    pub(crate) fn exact_regular_file_names(&self) -> io::Result<BTreeSet<String>> {
        self.verify_unchanged()?;
        let names = self.directory.exact_regular_file_names()?;
        self.verify_unchanged()?;
        Ok(names)
    }

    pub(crate) fn verify_unchanged(&self) -> io::Result<()> {
        self.directory.verify_path(&self.path)
    }

    #[cfg(unix)]
    pub(crate) fn create_new_subdirectory(&self, component: &str) -> io::Result<Self> {
        validate_component(component)?;
        self.verify_unchanged()?;
        let directory = self.directory.create_new_directory(component)?;
        let value = Self {
            path: self.path.join(component),
            directory,
        };
        value.verify_unchanged()?;
        self.verify_unchanged()?;
        Ok(value)
    }

    #[cfg(unix)]
    pub(crate) fn open_existing_subdirectory(&self, component: &str) -> io::Result<Self> {
        validate_component(component)?;
        self.verify_unchanged()?;
        let directory = self.directory.open_existing_directory(component)?;
        let value = Self {
            path: self.path.join(component),
            directory,
        };
        value.verify_unchanged()?;
        self.verify_unchanged()?;
        Ok(value)
    }

    #[cfg(unix)]
    pub(crate) fn verify_subdirectory_absent(&self, component: &str) -> io::Result<()> {
        validate_component(component)?;
        self.verify_unchanged()?;
        match self.directory.open_existing_directory(component) {
            Ok(_) => Err(invalid_input(
                "retained artifact subdirectory unexpectedly exists",
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => self.verify_unchanged(),
            Err(error) => Err(error),
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn open_existing_subdirectory(&self, _component: &str) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "retained directory transaction is currently available only on Unix",
        ))
    }

    #[cfg(not(unix))]
    pub(crate) fn verify_subdirectory_absent(&self, _component: &str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "retained directory absence verification is currently available only on Unix",
        ))
    }

    #[cfg(not(unix))]
    pub(crate) fn create_new_subdirectory(&self, _component: &str) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "retained directory transaction is currently available only on Unix",
        ))
    }

    pub(crate) fn sync_exact_regular_files(&self) -> io::Result<BTreeSet<String>> {
        let names = self.exact_regular_file_names()?;
        for name in &names {
            let file = self.open_existing_read_stable_file(name)?;
            file.sync_all()?;
            self.verify_file_identity(name, &file)?;
        }
        #[cfg(unix)]
        self.directory.sync_all()?;
        self.verify_unchanged()?;
        Ok(names)
    }

    #[cfg(unix)]
    pub(crate) fn install_directory_no_replace_terminal(
        &self,
        staged: &mut Self,
        staged_name: &str,
        final_name: &str,
        mut verify_before_commit: impl FnMut(&Self) -> io::Result<()>,
    ) -> io::Result<()> {
        validate_component(staged_name)?;
        validate_component(final_name)?;
        if staged_name == final_name
            || staged.path != self.path.join(staged_name)
            || staged
                .path
                .parent()
                .is_none_or(|parent| parent != self.path)
        {
            return Err(invalid_input(
                "staged retained directory is not an exact child of its publisher",
            ));
        }
        staged.sync_exact_regular_files()?;
        self.verify_unchanged()?;
        self.directory
            .verify_directory(staged_name, &staged.directory)?;
        verify_before_commit(staged)?;
        staged.verify_unchanged()?;
        self.directory
            .verify_directory(staged_name, &staged.directory)?;
        verify_before_commit(staged)?;
        self.directory
            .install_directory_no_replace(staged_name, final_name)?;
        staged.path = self.path.join(final_name);
        staged.verify_unchanged()?;
        self.directory.sync_all()?;
        self.verify_unchanged()
    }

    #[cfg(not(unix))]
    pub(crate) fn install_directory_no_replace_terminal(
        &self,
        _staged: &mut Self,
        _staged_name: &str,
        _final_name: &str,
        _verify_before_commit: impl FnMut(&Self) -> io::Result<()>,
    ) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "retained directory transaction is currently available only on Unix",
        ))
    }

    #[cfg(unix)]
    pub(crate) fn discard_empty_same_directory(
        &self,
        staged: &Self,
        staged_name: &str,
    ) -> io::Result<()> {
        validate_component(staged_name)?;
        if staged.path != self.path.join(staged_name)
            || !staged.exact_regular_file_names()?.is_empty()
        {
            return Err(invalid_input(
                "discarded retained directory is not the expected empty child",
            ));
        }
        self.verify_unchanged()?;
        self.directory
            .verify_directory(staged_name, &staged.directory)?;
        self.directory
            .discard_empty_same_directory(staged_name, &staged.directory)?;
        self.directory.sync_all()?;
        self.verify_unchanged()
    }

    #[cfg(not(unix))]
    pub(crate) fn discard_empty_same_directory(
        &self,
        _staged: &Self,
        _staged_name: &str,
    ) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "retained directory transaction is currently available only on Unix",
        ))
    }

    pub(crate) fn install_file_no_replace_terminal(
        &self,
        staged_file: &File,
        staged_name: &str,
        final_name: &str,
        mut verify_content_before_binding: impl FnMut() -> io::Result<()>,
        verify_before_commit: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        validate_component(staged_name)?;
        validate_component(final_name)?;
        if staged_name == final_name {
            return Err(invalid_input("staged and final artifact names must differ"));
        }
        staged_file.sync_all()?;
        self.verify_unchanged()?;
        verify_content_before_binding()?;
        self.directory.install_file_no_replace_terminal(
            staged_file,
            staged_name,
            final_name,
            verify_before_commit,
        )
    }

    pub(crate) fn discard_same_file(
        &self,
        staged_file: &File,
        staged_name: &str,
    ) -> io::Result<()> {
        validate_component(staged_name)?;
        self.verify_unchanged()?;
        self.directory.discard_same_file(staged_file, staged_name)?;
        self.verify_unchanged()
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::ffi::{CStr, CString};
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn clear_readdir_errno() {
        // SAFETY: the selected platform function returns this thread's live errno slot.
        unsafe { *errno_location() = 0 };
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn clear_readdir_errno() {}

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn readdir_errno() -> libc::c_int {
        // SAFETY: the selected platform function returns this thread's live errno slot.
        unsafe { *errno_location() }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn readdir_errno() -> libc::c_int {
        0
    }

    #[cfg(target_os = "linux")]
    unsafe fn errno_location() -> *mut libc::c_int {
        // SAFETY: caller treats the returned pointer as the current thread's errno slot.
        unsafe { libc::__errno_location() }
    }

    #[cfg(target_os = "macos")]
    unsafe fn errno_location() -> *mut libc::c_int {
        // SAFETY: caller treats the returned pointer as the current thread's errno slot.
        unsafe { libc::__error() }
    }

    pub(super) struct DirectoryHandle(OwnedFd);

    impl DirectoryHandle {
        pub(super) fn open_root(path: &Path) -> io::Result<Self> {
            let path = CString::new(path.as_os_str().as_encoded_bytes())
                .map_err(|_| invalid_input("retained artifact root contains NUL"))?;
            // SAFETY: path is a live NUL-terminated string.
            let fd = unsafe {
                libc::open(
                    path.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            let value = Self(owned_fd(fd)?);
            require_directory_fd(value.0.as_raw_fd())?;
            Ok(value)
        }

        pub(super) fn from_file(file: File) -> io::Result<Self> {
            let fd = OwnedFd::from(file);
            require_directory_fd(fd.as_raw_fd())?;
            Ok(Self(fd))
        }

        pub(super) fn try_clone_file(&self) -> io::Result<File> {
            Ok(File::from(self.0.try_clone()?))
        }

        pub(super) fn raw_fd(&self) -> RawFd {
            self.0.as_raw_fd()
        }

        pub(super) fn exact_regular_file_names(&self) -> io::Result<BTreeSet<String>> {
            let duplicate = self.0.try_clone()?.into_raw_fd();
            // SAFETY: duplicate is a new owned directory descriptor transferred to fdopendir.
            let directory = unsafe { libc::fdopendir(duplicate) };
            if directory.is_null() {
                // SAFETY: fdopendir failed and therefore did not consume duplicate.
                unsafe { libc::close(duplicate) };
                return Err(io::Error::last_os_error());
            }
            // SAFETY: directory is a live DIR pointer and rewinddir resets this enumeration pass.
            unsafe { libc::rewinddir(directory) };
            let result = (|| {
                let mut names = BTreeSet::new();
                loop {
                    clear_readdir_errno();
                    // SAFETY: directory remains live for the complete loop.
                    let entry = unsafe { libc::readdir(directory) };
                    if entry.is_null() {
                        let errno = readdir_errno();
                        if errno != 0 {
                            return Err(io::Error::from_raw_os_error(errno));
                        }
                        break;
                    }
                    // SAFETY: d_name is NUL-terminated storage owned by the live DIR entry.
                    let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }
                        .to_str()
                        .map_err(|_| invalid_input("retained artifact name must be valid UTF-8"))?;
                    if matches!(name, "." | "..") {
                        continue;
                    }
                    validate_component(name)?;
                    let file = self.open_existing_read_stable_file(name)?;
                    self.verify_file(name, &file)?;
                    if !names.insert(name.to_string()) {
                        return Err(invalid_input("duplicate retained artifact name"));
                    }
                }
                Ok(names)
            })();
            // SAFETY: fdopendir consumed duplicate; closedir closes that descriptor exactly once.
            if unsafe { libc::closedir(directory) } != 0 {
                return Err(io::Error::last_os_error());
            }
            result
        }

        pub(super) fn create_new_directory(&self, component: &str) -> io::Result<Self> {
            let component = c_component(component)?;
            // SAFETY: the retained parent descriptor and component are live and validated.
            if unsafe { libc::mkdirat(self.0.as_raw_fd(), component.as_ptr(), 0o700) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let result = (|| {
                // SAFETY: openat is constrained to the retained parent and rejects symlinks.
                let fd = unsafe {
                    libc::openat(
                        self.0.as_raw_fd(),
                        component.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                let value = Self(owned_fd(fd)?);
                require_directory_fd(value.0.as_raw_fd())?;
                self.verify_directory(
                    component
                        .to_str()
                        .map_err(|_| invalid_input("invalid retained directory name"))?,
                    &value,
                )?;
                Ok(value)
            })();
            if result.is_err() {
                // SAFETY: remove only the exact component just created under the retained parent.
                unsafe {
                    libc::unlinkat(self.0.as_raw_fd(), component.as_ptr(), libc::AT_REMOVEDIR);
                }
            }
            result
        }

        pub(super) fn open_existing_directory(&self, component: &str) -> io::Result<Self> {
            let component = c_component(component)?;
            // SAFETY: openat is constrained to the retained parent and rejects symlinks.
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    component.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            let value = Self(owned_fd(fd)?);
            require_directory_fd(value.0.as_raw_fd())?;
            self.verify_directory(
                component
                    .to_str()
                    .map_err(|_| invalid_input("invalid retained directory name"))?,
                &value,
            )?;
            Ok(value)
        }

        pub(super) fn verify_path(&self, path: &Path) -> io::Result<()> {
            let current = Self::open_root(path)?;
            require_same_node(self.0.as_raw_fd(), current.0.as_raw_fd(), "directory")
        }

        pub(super) fn open_existing_read_stable_file(&self, file_name: &str) -> io::Result<File> {
            self.open_file(file_name, libc::O_RDONLY)
        }

        pub(super) fn open_existing_terminal_stage(&self, file_name: &str) -> io::Result<File> {
            self.open_file(file_name, libc::O_RDWR)
        }

        pub(super) fn create_new_terminal_stage(&self, file_name: &str) -> io::Result<File> {
            let name = c_component(file_name)?;
            // SAFETY: retained directory descriptor and component remain live.
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
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
            Ok(File::from(fd))
        }

        fn open_file(&self, file_name: &str, access: libc::c_int) -> io::Result<File> {
            let name = c_component(file_name)?;
            // SAFETY: the retained directory descriptor and component remain live.
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    access | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            let fd = owned_fd(fd)?;
            require_regular_fd(fd.as_raw_fd())?;
            Ok(File::from(fd))
        }

        pub(super) fn verify_file(&self, file_name: &str, file: &File) -> io::Result<()> {
            let current = self.open_existing_read_stable_file(file_name)?;
            require_same_node(file.as_raw_fd(), current.as_raw_fd(), "artifact")
        }

        pub(super) fn verify_directory(&self, component: &str, directory: &Self) -> io::Result<()> {
            let component = c_component(component)?;
            // SAFETY: openat is constrained to the retained parent and rejects symlinks.
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    component.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            let current = Self(owned_fd(fd)?);
            require_directory_fd(current.0.as_raw_fd())?;
            require_same_node(directory.0.as_raw_fd(), current.0.as_raw_fd(), "directory")
        }

        pub(super) fn install_directory_no_replace(
            &self,
            staged_name: &str,
            final_name: &str,
        ) -> io::Result<()> {
            let staged_name = c_component(staged_name)?;
            let final_name = c_component(final_name)?;
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
                    "atomic directory no-replace is unsupported on this Unix",
                ));
            };
            if status != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub(super) fn discard_empty_same_directory(
            &self,
            component: &str,
            directory: &Self,
        ) -> io::Result<()> {
            self.verify_directory(component, directory)?;
            let component = c_component(component)?;
            // SAFETY: unlinkat is constrained to the retained parent and exact verified child.
            if unsafe { libc::unlinkat(self.0.as_raw_fd(), component.as_ptr(), libc::AT_REMOVEDIR) }
                != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub(super) fn sync_all(&self) -> io::Result<()> {
            // SAFETY: fsync operates on the retained live directory descriptor.
            if unsafe { libc::fsync(self.0.as_raw_fd()) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub(super) fn physical_identity(&self) -> io::Result<(u64, u64)> {
            let stat = file_stat(self.0.as_raw_fd())?;
            #[cfg(target_os = "linux")]
            let device = stat.st_dev;
            #[cfg(not(target_os = "linux"))]
            let device = u64::try_from(stat.st_dev)
                .map_err(|_| invalid_input("retained directory device is invalid"))?;
            Ok((device, stat.st_ino))
        }

        pub(super) fn install_file_no_replace_terminal(
            &self,
            staged_file: &File,
            staged_name: &str,
            final_name: &str,
            mut verify_before_commit: impl FnMut() -> io::Result<()>,
        ) -> io::Result<()> {
            let staged_name = c_component(staged_name)?;
            let final_name = c_component(final_name)?;
            let current = self.open_existing_read_stable_file(
                staged_name
                    .to_str()
                    .map_err(|_| invalid_input("invalid staged artifact name"))?,
            )?;
            require_same_node(
                staged_file.as_raw_fd(),
                current.as_raw_fd(),
                "staged artifact",
            )?;
            verify_before_commit()?;
            let rebound = self.open_existing_read_stable_file(
                staged_name
                    .to_str()
                    .map_err(|_| invalid_input("invalid staged artifact name"))?,
            )?;
            require_same_node(
                staged_file.as_raw_fd(),
                rebound.as_raw_fd(),
                "staged artifact",
            )?;
            verify_before_commit()?;
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
                    "atomic artifact no-replace is unsupported on this Unix",
                ));
            };
            if status != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub(super) fn discard_same_file(
            &self,
            staged_file: &File,
            staged_name: &str,
        ) -> io::Result<()> {
            let staged_name = c_component(staged_name)?;
            let current = self.open_existing_read_stable_file(
                staged_name
                    .to_str()
                    .map_err(|_| invalid_input("invalid staged artifact name"))?,
            )?;
            require_same_node(
                staged_file.as_raw_fd(),
                current.as_raw_fd(),
                "discarded artifact",
            )?;
            // SAFETY: the retained directory and exact verified component remain live.
            if unsafe { libc::unlinkat(self.0.as_raw_fd(), staged_name.as_ptr(), 0) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    fn c_component(component: &str) -> io::Result<CString> {
        validate_component(component)?;
        CString::new(component).map_err(|_| invalid_input("artifact name contains NUL"))
    }

    fn owned_fd(fd: RawFd) -> io::Result<OwnedFd> {
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a non-negative descriptor returned by open/openat is newly owned.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    fn require_regular_fd(fd: RawFd) -> io::Result<()> {
        let stat = file_stat(fd)?;
        if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(invalid_input("retained artifact must be a regular file"));
        }
        Ok(())
    }

    fn require_directory_fd(fd: RawFd) -> io::Result<()> {
        let stat = file_stat(fd)?;
        if stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
            return Err(invalid_input("retained artifact root must be a directory"));
        }
        Ok(())
    }

    fn file_stat(fd: RawFd) -> io::Result<libc::stat> {
        // SAFETY: fstat initializes the supplied stat buffer.
        let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
        // SAFETY: fd is live and stat points to writable storage.
        if unsafe { libc::fstat(fd, &mut stat) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(stat)
    }

    fn require_same_node(left: RawFd, right: RawFd, label: &str) -> io::Result<()> {
        let left = file_stat(left)?;
        let right = file_stat(right)?;
        if left.st_dev != right.st_dev || left.st_ino != right.st_ino {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("retained {label} handle differs from its directory entry"),
            ));
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod directory_transaction_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("wall clock")
            .as_nanos();
        fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!(
                "bm-retained-directory-{label}-{}-{nonce}",
                std::process::id()
            ))
    }

    #[test]
    fn directory_publish_is_no_replace_and_preserves_the_first_winner() {
        let root = fixture_root("no-replace");
        fs::create_dir(&root).expect("create root");
        let parent = RetainedArtifactDirectory::open_root(&root).expect("retain root");

        let mut first = parent
            .create_new_subdirectory(".stage-first")
            .expect("create first stage");
        fs::write(first.path.join("manifest.json"), b"first").expect("write first manifest");
        parent
            .install_directory_no_replace_terminal(
                &mut first,
                ".stage-first",
                "release-address",
                |_| Ok(()),
            )
            .expect("publish first");
        assert_eq!(
            fs::read(root.join("release-address/manifest.json")).expect("read winner"),
            b"first"
        );

        let mut second = parent
            .create_new_subdirectory(".stage-second")
            .expect("create second stage");
        fs::write(second.path.join("manifest.json"), b"second").expect("write second manifest");
        assert!(parent
            .install_directory_no_replace_terminal(
                &mut second,
                ".stage-second",
                "release-address",
                |_| Ok(()),
            )
            .is_err());
        assert_eq!(
            fs::read(root.join("release-address/manifest.json")).expect("read preserved winner"),
            b"first"
        );
        assert_eq!(
            fs::read(root.join(".stage-second/manifest.json")).expect("read losing stage"),
            b"second"
        );

        fs::remove_dir_all(&root).expect("remove fixture");
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::mem::{offset_of, size_of, zeroed};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileDispositionInfo, FileRenameInfo, GetFileInformationByHandle,
        SetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_RENAME_INFO,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
    };

    pub(super) struct DirectoryHandle {
        path: PathBuf,
        file: File,
    }

    impl DirectoryHandle {
        pub(super) fn open_root(path: &Path) -> io::Result<Self> {
            let file = fs::OpenOptions::new()
                .access_mode(FILE_GENERIC_READ | SYNCHRONIZE)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            require_directory(&file)?;
            Ok(Self {
                path: path.to_path_buf(),
                file,
            })
        }

        pub(super) fn verify_path(&self, path: &Path) -> io::Result<()> {
            let current = Self::open_root(path)?;
            require_same_node(&self.file, &current.file, "directory")
        }

        pub(super) fn exact_regular_file_names(&self) -> io::Result<BTreeSet<String>> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "retained-handle exact directory enumeration is not implemented on Windows",
            ))
        }

        pub(super) fn open_existing_read_stable_file(&self, file_name: &str) -> io::Result<File> {
            self.open_file(file_name, FILE_GENERIC_READ | SYNCHRONIZE, FILE_SHARE_READ)
        }

        pub(super) fn open_existing_terminal_stage(&self, file_name: &str) -> io::Result<File> {
            self.open_file(
                file_name,
                FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | SYNCHRONIZE,
                FILE_SHARE_READ,
            )
        }

        pub(super) fn create_new_terminal_stage(&self, file_name: &str) -> io::Result<File> {
            validate_component(file_name)?;
            let file = fs::OpenOptions::new()
                .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | SYNCHRONIZE)
                .share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .create_new(true)
                .open(self.path.join(file_name))?;
            require_regular(&file)?;
            Ok(file)
        }

        fn open_file(&self, file_name: &str, access: u32, share: u32) -> io::Result<File> {
            validate_component(file_name)?;
            let file = fs::OpenOptions::new()
                .access_mode(access)
                .share_mode(share)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(self.path.join(file_name))?;
            require_regular(&file)?;
            Ok(file)
        }

        pub(super) fn verify_file(&self, file_name: &str, file: &File) -> io::Result<()> {
            let current = self.open_file(
                file_name,
                FILE_GENERIC_READ | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            )?;
            require_same_node(file, &current, "artifact")
        }

        pub(super) fn install_file_no_replace_terminal(
            &self,
            staged_file: &File,
            staged_name: &str,
            final_name: &str,
            mut verify_before_commit: impl FnMut() -> io::Result<()>,
        ) -> io::Result<()> {
            self.verify_file(staged_name, staged_file)?;
            validate_component(final_name)?;
            let final_name = final_name.encode_utf16().collect::<Vec<_>>();
            let name_bytes = final_name
                .len()
                .checked_mul(size_of::<u16>())
                .ok_or_else(|| invalid_input("artifact name is too long"))?;
            let total_bytes = offset_of!(FILE_RENAME_INFO, FileName)
                .checked_add(name_bytes)
                .ok_or_else(|| invalid_input("artifact name is too long"))?;
            let mut storage = vec![0_usize; total_bytes.div_ceil(size_of::<usize>())];
            let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            // SAFETY: storage is aligned and large enough for FILE_RENAME_INFO plus the name.
            unsafe {
                (*info).Anonymous.ReplaceIfExists = false;
                (*info).RootDirectory = self.file.as_raw_handle();
                (*info).FileNameLength = u32::try_from(name_bytes)
                    .map_err(|_| invalid_input("artifact name is too long"))?;
                std::ptr::copy_nonoverlapping(
                    final_name.as_ptr(),
                    (*info).FileName.as_mut_ptr(),
                    final_name.len(),
                );
            }
            verify_before_commit()?;
            self.verify_file(staged_name, staged_file)?;
            verify_before_commit()?;
            // SAFETY: staged_file, directory handle, and rename buffer remain live.
            if unsafe {
                SetFileInformationByHandle(
                    staged_file.as_raw_handle(),
                    FileRenameInfo,
                    info.cast(),
                    u32::try_from(total_bytes)
                        .map_err(|_| invalid_input("rename buffer is too long"))?,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub(super) fn discard_same_file(
            &self,
            staged_file: &File,
            staged_name: &str,
        ) -> io::Result<()> {
            self.verify_file(staged_name, staged_file)?;
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: staged_file is live and disposition has the requested layout.
            if unsafe {
                SetFileInformationByHandle(
                    staged_file.as_raw_handle(),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    fn file_information(file: &File) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
        // SAFETY: GetFileInformationByHandle initializes the supplied buffer on success.
        let mut info = unsafe { zeroed::<BY_HANDLE_FILE_INFORMATION>() };
        // SAFETY: file is live and info points to writable storage.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info)
    }

    fn require_regular(file: &File) -> io::Result<()> {
        let info = file_information(file)?;
        if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
            return Err(invalid_input(
                "retained artifact must be a regular non-reparse file",
            ));
        }
        Ok(())
    }

    fn require_directory(file: &File) -> io::Result<()> {
        let info = file_information(file)?;
        if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(invalid_input(
                "retained artifact root must be a non-reparse directory",
            ));
        }
        Ok(())
    }

    fn require_same_node(left: &File, right: &File, label: &str) -> io::Result<()> {
        let left = file_information(left)?;
        let right = file_information(right)?;
        if left.dwVolumeSerialNumber != right.dwVolumeSerialNumber
            || left.nFileIndexHigh != right.nFileIndexHigh
            || left.nFileIndexLow != right.nFileIndexLow
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("retained {label} handle differs from its directory entry"),
            ));
        }
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use super::*;

    pub(super) struct DirectoryHandle;

    impl DirectoryHandle {
        pub(super) fn open_root(_path: &Path) -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "retained artifact filesystem is unsupported",
            ))
        }

        pub(super) fn verify_path(&self, _path: &Path) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "retained artifact filesystem is unsupported",
            ))
        }

        pub(super) fn exact_regular_file_names(&self) -> io::Result<BTreeSet<String>> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "retained artifact filesystem is unsupported",
            ))
        }

        pub(super) fn open_existing_read_stable_file(&self, _file_name: &str) -> io::Result<File> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "retained artifact filesystem is unsupported",
            ))
        }

        pub(super) fn open_existing_terminal_stage(&self, _file_name: &str) -> io::Result<File> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "retained artifact filesystem is unsupported",
            ))
        }

        pub(super) fn create_new_terminal_stage(&self, _file_name: &str) -> io::Result<File> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "retained artifact filesystem is unsupported",
            ))
        }

        pub(super) fn verify_file(&self, _file_name: &str, _file: &File) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "retained artifact filesystem is unsupported",
            ))
        }

        pub(super) fn install_file_no_replace_terminal(
            &self,
            _staged_file: &File,
            _staged_name: &str,
            _final_name: &str,
            _verify_before_commit: impl FnMut() -> io::Result<()>,
        ) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "retained artifact filesystem is unsupported",
            ))
        }

        pub(super) fn discard_same_file(
            &self,
            _staged_file: &File,
            _staged_name: &str,
        ) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "retained artifact filesystem is unsupported",
            ))
        }
    }
}
