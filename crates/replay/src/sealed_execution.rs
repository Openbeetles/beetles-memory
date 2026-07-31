//! Generation-neutral sealed executable authority.
//!
//! This module owns only retained executable bytes, Linux memfd sealing/fexecve, and a one-shot
//! inherited execution claim. Generation-specific schema, roles, receipts, roots, and policy stay
//! in their typed wrappers.

use std::{
    fs::File,
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    process::Command,
};

use sha2::{Digest, Sha256};

use crate::retained_artifact_fs::RetainedArtifactDirectory;

pub(crate) const PEER_CHANNEL_MAX_MESSAGE_BYTES: usize = 64 * 1024;
pub(crate) const PEER_CHANNEL_MAX_FILES: usize = 20;

#[cfg(target_os = "linux")]
use std::{
    collections::BTreeSet,
    process::{Child, Stdio},
    sync::{LazyLock, Mutex},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SealedContentIdentity {
    byte_len: u64,
    sha256: String,
}

impl SealedContentIdentity {
    pub(crate) fn byte_len(&self) -> u64 {
        self.byte_len
    }

    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[cfg(target_os = "linux")]
pub(crate) const fn required_linux_seals() -> libc::c_int {
    libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL
}

#[cfg(target_os = "linux")]
pub(crate) enum SealedClaimFailure {
    AlreadyConsumed,
    DescriptorMissing,
    DescriptorInvalid,
    DescriptorReserved,
    LocatorMissing,
    LocatorNotAbsolute,
    Sha256Missing,
    Sha256Invalid,
    NotExecutable,
    SealQueryFailed,
    MissingRequiredSeals,
    NotCurrentExecutionObject,
    Io(io::Error),
}

#[cfg(target_os = "linux")]
impl SealedClaimFailure {
    fn into_neutral_io_error(self) -> io::Error {
        match self {
            Self::AlreadyConsumed => {
                invalid_data("sealed execution authority was already consumed")
            }
            Self::DescriptorMissing => invalid_data("sealed executable descriptor is missing"),
            Self::DescriptorInvalid => invalid_data("sealed executable descriptor is invalid"),
            Self::DescriptorReserved => {
                invalid_data("sealed executable descriptor is reserved or invalid")
            }
            Self::LocatorMissing => invalid_data("sealed executable locator is missing"),
            Self::LocatorNotAbsolute => invalid_data("sealed executable locator must be absolute"),
            Self::Sha256Missing => invalid_data("sealed executable SHA256 is missing"),
            Self::Sha256Invalid => invalid_data("sealed executable SHA256 is invalid"),
            Self::NotExecutable => invalid_data("sealed executable descriptor is not executable"),
            Self::SealQueryFailed | Self::MissingRequiredSeals => {
                invalid_data("sealed executable descriptor is not fully sealed")
            }
            Self::NotCurrentExecutionObject => {
                invalid_data("sealed executable descriptor is not the current execution object")
            }
            Self::Io(error) => error,
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) enum SealedObservationFailure {
    IdentityMismatch,
    Io(io::Error),
}

#[cfg(target_os = "linux")]
impl SealedObservationFailure {
    fn into_neutral_io_error(self, mismatch_message: &'static str) -> io::Error {
        match self {
            Self::IdentityMismatch => invalid_data(mismatch_message),
            Self::Io(error) => error,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SealedExecutionDomain {
    claim_key: &'static str,
    argv0: &'static str,
    fd_env: &'static str,
    locator_env: &'static str,
    sha256_env: &'static str,
    stripped_env_prefixes: &'static [&'static str],
}

impl SealedExecutionDomain {
    pub(crate) const fn new(
        claim_key: &'static str,
        argv0: &'static str,
        fd_env: &'static str,
        locator_env: &'static str,
        sha256_env: &'static str,
        stripped_env_prefixes: &'static [&'static str],
    ) -> Self {
        Self {
            claim_key,
            argv0,
            fd_env,
            locator_env,
            sha256_env,
            stripped_env_prefixes,
        }
    }

    fn validate(self) -> io::Result<()> {
        let values = [
            self.claim_key,
            self.argv0,
            self.fd_env,
            self.locator_env,
            self.sha256_env,
        ];
        if values.iter().any(|value| {
            value.is_empty()
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        }) {
            return Err(invalid_input("sealed execution domain is invalid"));
        }
        if self.fd_env == self.locator_env
            || self.fd_env == self.sha256_env
            || self.locator_env == self.sha256_env
        {
            return Err(invalid_input(
                "sealed execution authority environment keys must be distinct",
            ));
        }
        if self
            .stripped_env_prefixes
            .iter()
            .any(|prefix| prefix.is_empty() || prefix.as_bytes().contains(&b'='))
        {
            return Err(invalid_input(
                "sealed execution stripped environment prefix is invalid",
            ));
        }
        Ok(())
    }

    fn is_reserved_env(self, name: &std::ffi::OsStr) -> bool {
        let bytes = name.as_encoded_bytes();
        bytes == self.fd_env.as_bytes()
            || bytes == self.locator_env.as_bytes()
            || bytes == self.sha256_env.as_bytes()
            || self
                .stripped_env_prefixes
                .iter()
                .any(|prefix| bytes.starts_with(prefix.as_bytes()))
    }
}

pub(crate) struct RetainedExecutable {
    locator: PathBuf,
    file_name: String,
    file: File,
    owner: Option<RetainedArtifactDirectory>,
    admitted_len: u64,
}

impl RetainedExecutable {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(invalid_input("retained executable path must be absolute"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| invalid_input("retained executable has no parent"))?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid_input("retained executable name must be valid UTF-8"))?
            .to_string();
        let owner = RetainedArtifactDirectory::open_root(parent)?;
        let file = owner.open_existing_read_stable_file(&file_name)?;
        let admitted_len = file.metadata()?.len();
        if admitted_len == 0 {
            return Err(invalid_input("retained executable must not be empty"));
        }
        Ok(Self {
            locator: path.to_path_buf(),
            file_name,
            file,
            owner: Some(owner),
            admitted_len,
        })
    }

    #[cfg(unix)]
    pub(crate) fn from_retained_file(
        locator: &Path,
        file: File,
        admitted_len: u64,
    ) -> io::Result<Self> {
        if !locator.is_absolute() {
            return Err(invalid_input(
                "retained executable locator must be absolute",
            ));
        }
        let file_name = locator
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid_input("retained executable name must be valid UTF-8"))?
            .to_string();
        let metadata = file.metadata()?;
        if !metadata.file_type().is_file() || metadata.len() != admitted_len || admitted_len == 0 {
            return Err(invalid_data(
                "retained executable descriptor does not match its admission",
            ));
        }
        Ok(Self {
            locator: locator.to_path_buf(),
            file_name,
            file,
            owner: None,
            admitted_len,
        })
    }

    #[cfg(unix)]
    pub(crate) fn try_clone_file(&self) -> io::Result<File> {
        self.verify_owner_identity()?;
        self.file.try_clone()
    }

    #[cfg(unix)]
    pub(crate) fn unix_physical_identity(&self) -> io::Result<(u64, u64)> {
        use std::os::unix::fs::MetadataExt as _;

        self.verify_owner_identity()?;
        let metadata = self.file.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn inheritable_duplicate(&self) -> io::Result<File> {
        use std::os::fd::{AsRawFd as _, FromRawFd as _};

        self.verify_owner_identity()?;
        let raw = unsafe { libc::fcntl(self.file.as_raw_fd(), libc::F_DUPFD, 3) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fcntl returned a newly owned descriptor with FD_CLOEXEC cleared.
        Ok(unsafe { File::from_raw_fd(raw) })
    }

    pub(crate) fn locator(&self) -> &Path {
        &self.locator
    }

    pub(crate) fn verify_content(&mut self, expected: &SealedContentIdentity) -> io::Result<()> {
        let actual = self.hash_retained()?;
        if &actual != expected {
            return Err(invalid_data(
                "retained executable bytes changed after admission",
            ));
        }
        Ok(())
    }

    pub(crate) fn copy_to_verified(
        &mut self,
        destination: &mut dyn Write,
    ) -> io::Result<SealedContentIdentity> {
        self.verify_owner_identity()?;
        self.file.rewind()?;
        let limit = self
            .admitted_len
            .checked_add(1)
            .ok_or_else(|| invalid_data("retained executable copy limit overflow"))?;
        let mut reader = HashingReader::new((&mut self.file).take(limit));
        io::copy(&mut reader, destination)?;
        let identity = reader.finish();
        self.file.rewind()?;
        if identity.byte_len != self.admitted_len {
            return Err(invalid_data(
                "retained executable length changed while being copied",
            ));
        }
        self.verify_owner_identity()?;
        Ok(identity)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn prepare(
        &mut self,
        domain: SealedExecutionDomain,
        args: &[String],
    ) -> io::Result<PreparedSealedExecutable> {
        linux::prepare(self, domain, args, linux::required_seals())
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn prepare_with_linux_seals_for_legacy_p7_contract(
        &mut self,
        domain: SealedExecutionDomain,
        args: &[String],
        seals: libc::c_int,
    ) -> io::Result<PreparedSealedExecutable> {
        linux::prepare(self, domain, args, seals)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn prepare_with_linux_pre_exec_barrier(
        &mut self,
        domain: SealedExecutionDomain,
        args: &[String],
        attempt_nonce: u64,
    ) -> io::Result<PreparedLinuxBarrierSealedExecutable> {
        linux::prepare_with_barrier(self, domain, args, attempt_nonce, None, &[], Vec::new())
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn prepare_with_linux_pre_exec_barrier_and_environment(
        &mut self,
        domain: SealedExecutionDomain,
        args: &[String],
        attempt_nonce: u64,
        extra_environment: &[(String, String)],
        inheritable_files: Vec<File>,
    ) -> io::Result<PreparedLinuxBarrierSealedExecutable> {
        linux::prepare_with_barrier(
            self,
            domain,
            args,
            attempt_nonce,
            None,
            extra_environment,
            inheritable_files,
        )
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn prepare_with_linux_pre_exec_barrier_joining_process_group_and_environment(
        &mut self,
        domain: SealedExecutionDomain,
        args: &[String],
        attempt_nonce: u64,
        process_group: u32,
        extra_environment: &[(String, String)],
        inheritable_files: Vec<File>,
    ) -> io::Result<PreparedLinuxBarrierSealedExecutable> {
        linux::prepare_with_barrier(
            self,
            domain,
            args,
            attempt_nonce,
            Some(process_group),
            extra_environment,
            inheritable_files,
        )
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn prepare_with_linux_exact_environment(
        &mut self,
        domain: SealedExecutionDomain,
        args: &[String],
        exact_environment: &[(String, String)],
        inheritable_files: Vec<File>,
    ) -> io::Result<PreparedSealedExecutable> {
        linux::prepare_with_exact_environment(
            self,
            domain,
            args,
            exact_environment,
            inheritable_files,
        )
    }

    /// Seals and fexecve-launches a foreign tool that does not implement Beetle's in-process
    /// authority claim protocol. Only the caller-supplied exact environment is exposed.
    #[cfg(target_os = "linux")]
    pub(crate) fn prepare_unclaimed_with_linux_exact_environment(
        &mut self,
        domain: SealedExecutionDomain,
        args: &[String],
        exact_environment: &[(String, String)],
        inheritable_files: Vec<File>,
    ) -> io::Result<PreparedSealedExecutable> {
        linux::prepare_unclaimed_with_exact_environment(
            self,
            domain,
            args,
            exact_environment,
            inheritable_files,
        )
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn prepare(
        &mut self,
        domain: SealedExecutionDomain,
        _args: &[String],
    ) -> io::Result<PreparedSealedExecutable> {
        domain.validate()?;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "sealed byte execution requires a Linux memfd/fexecve authority",
        ))
    }

    fn hash_retained(&mut self) -> io::Result<SealedContentIdentity> {
        self.verify_owner_identity()?;
        let identity = hash_exact_file(&mut self.file, self.admitted_len)?;
        self.verify_owner_identity()?;
        Ok(identity)
    }

    fn verify_owner_identity(&self) -> io::Result<()> {
        if let Some(owner) = &self.owner {
            owner.verify_file_identity(&self.file_name, &self.file)?;
        }
        Ok(())
    }
}

pub(crate) struct PreparedSealedExecutable {
    command: Command,
    guard: SealedLaunchGuard,
    identity: SealedContentIdentity,
}

impl PreparedSealedExecutable {
    pub(crate) fn into_parts(self) -> (Command, SealedLaunchGuard, SealedContentIdentity) {
        (self.command, self.guard, self.identity)
    }
}

pub(crate) struct SealedLaunchGuard {
    #[allow(dead_code)]
    files: Vec<File>,
}

#[cfg(target_os = "linux")]
pub(crate) struct LinuxPeerBoundFdChannel {
    socket: std::os::fd::OwnedFd,
    deadline_monotonic_nanos: u64,
}

#[cfg(target_os = "linux")]
impl LinuxPeerBoundFdChannel {
    pub(crate) fn pair_with_timeout(timeout: Duration) -> io::Result<(Self, Self)> {
        if timeout.is_zero() {
            return Err(invalid_input("peer channel timeout must be non-zero"));
        }
        let deadline_monotonic_nanos = monotonic_nanos()?
            .checked_add(
                u64::try_from(timeout.as_nanos())
                    .map_err(|_| invalid_input("peer channel timeout is too large"))?,
            )
            .ok_or_else(|| invalid_input("peer channel deadline overflow"))?;
        Self::pair_with_deadline_monotonic_nanos(deadline_monotonic_nanos)
    }

    pub(crate) fn pair_with_deadline_monotonic_nanos(
        deadline_monotonic_nanos: u64,
    ) -> io::Result<(Self, Self)> {
        use std::os::fd::{AsRawFd as _, FromRawFd as _};

        if deadline_monotonic_nanos <= monotonic_nanos()? {
            return Err(invalid_input("peer channel deadline must be in the future"));
        }
        let mut sockets = [-1; 2];
        // SAFETY: socketpair initializes both descriptors on success.
        if unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                sockets.as_mut_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: both descriptors are newly owned after successful socketpair.
        let left = unsafe { std::os::fd::OwnedFd::from_raw_fd(sockets[0]) };
        // SAFETY: both descriptors are newly owned after successful socketpair.
        let right = unsafe { std::os::fd::OwnedFd::from_raw_fd(sockets[1]) };
        set_pass_credentials(left.as_raw_fd())?;
        set_pass_credentials(right.as_raw_fd())?;
        set_nonblocking_fd(left.as_raw_fd())?;
        set_nonblocking_fd(right.as_raw_fd())?;
        Ok((
            Self {
                socket: left,
                deadline_monotonic_nanos,
            },
            Self {
                socket: right,
                deadline_monotonic_nanos,
            },
        ))
    }

    pub(crate) fn deadline_monotonic_nanos(&self) -> u64 {
        self.deadline_monotonic_nanos
    }

    pub(crate) fn remaining_time(&self) -> io::Result<Duration> {
        remaining_until_monotonic_deadline(self.deadline_monotonic_nanos)
    }

    pub(crate) fn inheritable_duplicate(&self) -> io::Result<File> {
        use std::os::fd::{AsRawFd as _, FromRawFd as _};

        // F_DUPFD intentionally clears FD_CLOEXEC so the exact descriptor crosses fexecve.
        let raw = unsafe { libc::fcntl(self.socket.as_raw_fd(), libc::F_DUPFD, 3) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fcntl returned a newly owned descriptor.
        Ok(unsafe { File::from_raw_fd(raw) })
    }

    pub(crate) fn claim_inherited(env_key: &str, deadline_env_key: &str) -> io::Result<Self> {
        use std::os::fd::{AsRawFd as _, FromRawFd as _};

        static CLAIM_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _claim = CLAIM_LOCK
            .lock()
            .map_err(|_| invalid_data("peer channel claim lock is poisoned"))?;
        let raw_value = std::env::var(env_key);
        let deadline_value = std::env::var(deadline_env_key);
        std::env::remove_var(env_key);
        std::env::remove_var(deadline_env_key);
        let raw = raw_value
            .map_err(|_| invalid_data("peer channel descriptor is missing"))?
            .parse::<libc::c_int>()
            .map_err(|_| invalid_data("peer channel descriptor is invalid"))?;
        if raw < 3 {
            return Err(invalid_data("peer channel descriptor is reserved"));
        }
        // SAFETY: a non-reserved inherited descriptor is consumed exactly once by this claim.
        let inherited = unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) };
        let deadline_monotonic_nanos = deadline_value
            .map_err(|_| invalid_data("peer channel deadline is missing"))?
            .parse::<u64>()
            .map_err(|_| invalid_data("peer channel deadline is invalid"))?;
        if deadline_monotonic_nanos <= monotonic_nanos()? {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "peer channel deadline elapsed before claim",
            ));
        }
        let duplicate = unsafe { libc::fcntl(inherited.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        if duplicate < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fcntl returned a newly owned descriptor.
        let socket = unsafe { std::os::fd::OwnedFd::from_raw_fd(duplicate) };
        let socket_type = socket_option_i32(socket.as_raw_fd(), libc::SO_TYPE)?;
        if socket_type != libc::SOCK_SEQPACKET {
            return Err(invalid_data("peer channel is not a Unix seqpacket socket"));
        }
        set_pass_credentials(socket.as_raw_fd())?;
        set_nonblocking_fd(socket.as_raw_fd())?;
        Ok(Self {
            socket,
            deadline_monotonic_nanos,
        })
    }

    pub(crate) fn send_with_files(&self, bytes: &[u8], files: &[&File]) -> io::Result<()> {
        use std::os::fd::AsRawFd as _;

        if bytes.is_empty()
            || bytes.len() > PEER_CHANNEL_MAX_MESSAGE_BYTES
            || files.len() > PEER_CHANNEL_MAX_FILES
        {
            return Err(invalid_input("peer channel message shape is invalid"));
        }
        let mut iov = libc::iovec {
            iov_base: bytes.as_ptr().cast_mut().cast(),
            iov_len: bytes.len(),
        };
        let raw_fds = files
            .iter()
            .map(|file| file.as_raw_fd())
            .collect::<Vec<_>>();
        let rights_bytes = raw_fds
            .len()
            .checked_mul(std::mem::size_of::<libc::c_int>())
            .ok_or_else(|| invalid_input("peer channel descriptor count overflow"))?;
        let control_len = if raw_fds.is_empty() {
            0
        } else {
            (unsafe {
                libc::CMSG_SPACE(
                    u32::try_from(rights_bytes)
                        .map_err(|_| invalid_input("peer channel control length overflow"))?,
                )
            }) as usize
        };
        let mut control = vec![0_u8; control_len];
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        if !raw_fds.is_empty() {
            message.msg_control = control.as_mut_ptr().cast();
            message.msg_controllen = control.len();
            // SAFETY: message points to a live, CMSG_SPACE-sized control buffer.
            let header = unsafe { libc::CMSG_FIRSTHDR(&message) };
            if header.is_null() {
                return Err(invalid_data("peer channel control header is unavailable"));
            }
            // SAFETY: header is inside the live control buffer.
            unsafe {
                (*header).cmsg_level = libc::SOL_SOCKET;
                (*header).cmsg_type = libc::SCM_RIGHTS;
                (*header).cmsg_len = libc::CMSG_LEN(
                    u32::try_from(rights_bytes)
                        .map_err(|_| invalid_input("peer channel rights length overflow"))?,
                ) as usize;
                std::ptr::copy_nonoverlapping(
                    raw_fds.as_ptr().cast::<u8>(),
                    libc::CMSG_DATA(header),
                    rights_bytes,
                );
            }
        }
        let sent = loop {
            self.wait_ready(libc::POLLOUT)?;
            // SAFETY: the message and all referenced buffers remain live for the syscall.
            let sent = unsafe {
                libc::sendmsg(
                    self.socket.as_raw_fd(),
                    &message,
                    libc::MSG_NOSIGNAL | libc::MSG_DONTWAIT,
                )
            };
            if sent >= 0 {
                break sent;
            }
            let error = io::Error::last_os_error();
            if matches!(
                error.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) {
                continue;
            }
            return Err(error);
        };
        if usize::try_from(sent).ok() != Some(bytes.len()) {
            return Err(invalid_data("peer channel message was only partially sent"));
        }
        Ok(())
    }

    pub(crate) fn receive_with_files(
        &self,
        expected_peer_pid: u32,
        expected_file_count: usize,
    ) -> io::Result<(Vec<u8>, Vec<File>)> {
        use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};

        if expected_peer_pid == 0 || expected_file_count > PEER_CHANNEL_MAX_FILES {
            return Err(invalid_input("peer channel receive contract is invalid"));
        }
        let rights_bytes = PEER_CHANNEL_MAX_FILES
            .checked_mul(std::mem::size_of::<libc::c_int>())
            .ok_or_else(|| invalid_input("peer channel descriptor count overflow"))?;
        let rights_space = unsafe {
            libc::CMSG_SPACE(
                u32::try_from(rights_bytes)
                    .map_err(|_| invalid_input("peer channel rights length overflow"))?,
            )
        } as usize;
        let credentials_space =
            unsafe { libc::CMSG_SPACE(u32::try_from(std::mem::size_of::<libc::ucred>()).unwrap()) }
                as usize;
        let mut bytes = vec![0_u8; PEER_CHANNEL_MAX_MESSAGE_BYTES];
        let mut control = vec![0_u8; rights_space.saturating_add(credentials_space)];
        let mut iov = libc::iovec {
            iov_base: bytes.as_mut_ptr().cast(),
            iov_len: bytes.len(),
        };
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = control.len();
        let received = loop {
            self.wait_ready(libc::POLLIN)?;
            // SAFETY: the message and all referenced buffers remain live for the syscall.
            let received = unsafe {
                libc::recvmsg(
                    self.socket.as_raw_fd(),
                    &mut message,
                    libc::MSG_CMSG_CLOEXEC | libc::MSG_DONTWAIT,
                )
            };
            if received >= 0 {
                break received;
            }
            let error = io::Error::last_os_error();
            if matches!(
                error.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) {
                continue;
            }
            return Err(error);
        };
        if received <= 0 {
            return if received == 0 {
                Err(invalid_data(
                    "peer channel closed before a complete message",
                ))
            } else {
                Err(io::Error::last_os_error())
            };
        }
        bytes.truncate(
            usize::try_from(received)
                .map_err(|_| invalid_data("peer channel message length is invalid"))?,
        );
        let mut observed_pid = None;
        let mut received_fds = Vec::<OwnedFd>::new();
        let mut rights_message_count = 0_usize;
        let mut protocol_error = None;
        // SAFETY: CMSG traversal stays inside the recvmsg-populated control buffer.
        let mut header = unsafe { libc::CMSG_FIRSTHDR(&message) };
        while !header.is_null() {
            // SAFETY: header is a live control header returned by libc.
            let current = unsafe { &*header };
            if current.cmsg_level == libc::SOL_SOCKET && current.cmsg_type == libc::SCM_CREDENTIALS
            {
                if current.cmsg_len
                    != unsafe {
                        libc::CMSG_LEN(u32::try_from(std::mem::size_of::<libc::ucred>()).unwrap())
                    } as usize
                {
                    protocol_error.get_or_insert("peer credentials control shape is invalid");
                } else {
                    // SAFETY: the credentials cmsg length was checked above.
                    let credentials = unsafe {
                        std::ptr::read_unaligned(libc::CMSG_DATA(header).cast::<libc::ucred>())
                    };
                    if observed_pid.replace(credentials.pid).is_some() {
                        protocol_error.get_or_insert("peer credentials were duplicated");
                    }
                }
            } else if current.cmsg_level == libc::SOL_SOCKET
                && current.cmsg_type == libc::SCM_RIGHTS
            {
                rights_message_count = rights_message_count.saturating_add(1);
                let header_len = unsafe { libc::CMSG_LEN(0) } as usize;
                if let Some(data_len) = current.cmsg_len.checked_sub(header_len) {
                    if data_len % std::mem::size_of::<libc::c_int>() != 0 {
                        protocol_error.get_or_insert("peer rights control data is misaligned");
                    }
                    let count = data_len / std::mem::size_of::<libc::c_int>();
                    // SAFETY: the complete portion of the SCM_RIGHTS payload contains count
                    // c_int values. Ownership is moved into RAII immediately so every later
                    // protocol error closes all descriptors.
                    let slice = unsafe {
                        std::slice::from_raw_parts(
                            libc::CMSG_DATA(header).cast::<libc::c_int>(),
                            count,
                        )
                    };
                    for raw in slice {
                        if *raw < 0 {
                            protocol_error.get_or_insert("peer rights descriptor is invalid");
                        } else {
                            // SAFETY: every SCM_RIGHTS descriptor is newly owned by this process.
                            received_fds.push(unsafe { OwnedFd::from_raw_fd(*raw) });
                        }
                    }
                } else {
                    protocol_error.get_or_insert("peer rights control shape is invalid");
                }
            } else {
                protocol_error.get_or_insert("peer channel received an unknown control message");
            }
            // SAFETY: CMSG_NXTHDR advances within the recvmsg-populated buffer.
            header = unsafe { libc::CMSG_NXTHDR(&message, header) };
        }
        if message.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0 {
            protocol_error.get_or_insert("peer channel message or control data was truncated");
        }
        let expected_pid = i32::try_from(expected_peer_pid)
            .map_err(|_| invalid_input("expected peer PID is invalid"))?;
        let expected_rights_messages = usize::from(expected_file_count != 0);
        if observed_pid != Some(expected_pid)
            || rights_message_count != expected_rights_messages
            || received_fds.len() != expected_file_count
        {
            protocol_error.get_or_insert("peer identity or retained descriptor set is not exact");
        }
        if let Some(message) = protocol_error {
            return Err(invalid_data(message));
        }
        let files = received_fds.into_iter().map(File::from).collect();
        Ok((bytes, files))
    }

    fn wait_ready(&self, events: libc::c_short) -> io::Result<()> {
        use std::os::fd::AsRawFd as _;

        loop {
            let now = monotonic_nanos()?;
            let remaining = self
                .deadline_monotonic_nanos
                .checked_sub(now)
                .filter(|remaining| *remaining != 0)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::TimedOut, "peer channel deadline elapsed")
                })?;
            let millis = remaining
                .saturating_add(999_999)
                .checked_div(1_000_000)
                .unwrap_or(1)
                .min(i32::MAX as u64) as libc::c_int;
            let mut descriptor = libc::pollfd {
                fd: self.socket.as_raw_fd(),
                events,
                revents: 0,
            };
            // SAFETY: descriptor points to one live pollfd.
            let result = unsafe { libc::poll(&mut descriptor, 1, millis) };
            if result > 0 {
                if monotonic_nanos()? > self.deadline_monotonic_nanos {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "peer channel deadline elapsed",
                    ));
                }
                if descriptor.revents & libc::POLLNVAL != 0 {
                    return Err(invalid_data("peer channel descriptor became invalid"));
                }
                return Ok(());
            }
            if result == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "peer channel deadline elapsed",
                ));
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn monotonic_nanos() -> io::Result<u64> {
    let mut value: libc::timespec = unsafe { std::mem::zeroed() };
    // SAFETY: clock_gettime initializes the live timespec on success.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut value) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let seconds = u64::try_from(value.tv_sec)
        .map_err(|_| invalid_data("monotonic clock seconds are invalid"))?;
    let nanos = u64::try_from(value.tv_nsec)
        .map_err(|_| invalid_data("monotonic clock nanoseconds are invalid"))?;
    seconds
        .checked_mul(1_000_000_000)
        .and_then(|value| value.checked_add(nanos))
        .ok_or_else(|| invalid_data("monotonic clock overflow"))
}

#[cfg(target_os = "linux")]
pub(crate) fn remaining_until_monotonic_deadline(
    deadline_monotonic_nanos: u64,
) -> io::Result<Duration> {
    let remaining = deadline_monotonic_nanos
        .checked_sub(monotonic_nanos()?)
        .filter(|value| *value != 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "monotonic deadline elapsed"))?;
    Ok(Duration::from_nanos(remaining))
}

#[cfg(target_os = "linux")]
fn set_nonblocking_fd(fd: std::os::fd::RawFd) -> io::Result<()> {
    // SAFETY: fcntl reads and updates flags on a live descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_pass_credentials(fd: std::os::fd::RawFd) -> io::Result<()> {
    let enabled: libc::c_int = 1;
    // SAFETY: setsockopt reads the live integer option value.
    if unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PASSCRED,
            (&enabled as *const libc::c_int).cast(),
            u32::try_from(std::mem::size_of_val(&enabled))
                .map_err(|_| invalid_input("socket option length overflow"))?,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn socket_option_i32(fd: std::os::fd::RawFd, option: libc::c_int) -> io::Result<libc::c_int> {
    let mut value: libc::c_int = 0;
    let mut length = u32::try_from(std::mem::size_of_val(&value))
        .map_err(|_| invalid_input("socket option length overflow"))?;
    // SAFETY: getsockopt writes at most length bytes into the live integer value.
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            option,
            (&mut value as *mut libc::c_int).cast(),
            &mut length,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if usize::try_from(length).ok() != Some(std::mem::size_of_val(&value)) {
        return Err(invalid_data("socket option length is invalid"));
    }
    Ok(value)
}

#[cfg(target_os = "linux")]
pub(crate) struct PreparedLinuxBarrierSealedExecutable {
    command: Command,
    guard: SealedLaunchGuard,
    identity: SealedContentIdentity,
    broker: LinuxPreExecBarrierBroker,
}

#[cfg(target_os = "linux")]
impl PreparedLinuxBarrierSealedExecutable {
    pub(crate) fn into_broker_and_launch(
        self,
    ) -> (LinuxPreExecBarrierBroker, LinuxBarrierSealedLaunch) {
        (
            self.broker,
            LinuxBarrierSealedLaunch {
                command: self.command,
                guard: self.guard,
                identity: self.identity,
            },
        )
    }
}

#[cfg(target_os = "linux")]
pub(crate) struct LinuxBarrierSealedLaunch {
    command: Command,
    guard: SealedLaunchGuard,
    identity: SealedContentIdentity,
}

#[cfg(target_os = "linux")]
impl LinuxBarrierSealedLaunch {
    pub(crate) fn spawn_piped(mut self) -> io::Result<SpawnedLinuxBarrierSealedExecutable> {
        self.command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = self.command.spawn()?;
        Ok(SpawnedLinuxBarrierSealedExecutable {
            child,
            guard: self.guard,
            identity: self.identity,
        })
    }
}

#[cfg(target_os = "linux")]
pub(crate) struct SpawnedLinuxBarrierSealedExecutable {
    pub(crate) child: Child,
    #[allow(dead_code)]
    guard: SealedLaunchGuard,
    pub(crate) identity: SealedContentIdentity,
}

#[cfg(target_os = "linux")]
impl SpawnedLinuxBarrierSealedExecutable {
    pub(crate) fn into_parts(self) -> (Child, SealedLaunchGuard, SealedContentIdentity) {
        (self.child, self.guard, self.identity)
    }
}

#[cfg(target_os = "linux")]
pub(crate) struct LinuxPreExecBarrierBroker {
    ready: File,
    release: Option<File>,
    attempt_nonce: u64,
    ready_pid: Option<u32>,
    join_process_group: Option<u32>,
}

#[cfg(target_os = "linux")]
impl LinuxPreExecBarrierBroker {
    pub(crate) fn wait_ready(&mut self, timeout: Duration) -> io::Result<u32> {
        linux::broker_wait_ready(self, timeout)
    }

    pub(crate) fn release(mut self, admitted_pid: u32) -> io::Result<()> {
        if self.ready_pid != Some(admitted_pid) {
            return Err(invalid_data(
                "pre-exec barrier release PID differs from the ready child",
            ));
        }
        let mut release = self
            .release
            .take()
            .ok_or_else(|| invalid_data("pre-exec barrier was already released"))?;
        release.write_all(&[linux::BARRIER_RELEASE_TOKEN])?;
        release.flush()
    }
}

pub(crate) struct ClaimedSealedExecution {
    file: File,
    expected_sha256: String,
    locator: PathBuf,
    identity: SealedContentIdentity,
}

impl ClaimedSealedExecution {
    #[cfg(target_os = "linux")]
    pub(crate) fn claim(domain: SealedExecutionDomain) -> io::Result<Self> {
        Self::claim_typed(domain).map_err(SealedClaimFailure::into_neutral_io_error)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn claim_typed(domain: SealedExecutionDomain) -> Result<Self, SealedClaimFailure> {
        linux::claim(domain)
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn claim(domain: SealedExecutionDomain) -> io::Result<Self> {
        domain.validate()?;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "inherited sealed execution authority is available only on Linux",
        ))
    }

    pub(crate) fn locator(&self) -> &Path {
        &self.locator
    }

    pub(crate) fn identity(&self) -> &SealedContentIdentity {
        &self.identity
    }

    pub(crate) fn verify(&mut self) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            self.verify_typed().map_err(|failure| {
                failure.into_neutral_io_error(
                    "inherited sealed executable differs from admitted identity",
                )
            })
        }
        #[cfg(not(target_os = "linux"))]
        let actual = hash_exact_file(&mut self.file, self.identity.byte_len)?;
        #[cfg(not(target_os = "linux"))]
        if actual != self.identity || actual.sha256 != self.expected_sha256 {
            return Err(invalid_data(
                "inherited sealed executable differs from admitted identity",
            ));
        }
        #[cfg(not(target_os = "linux"))]
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn verify_typed(&mut self) -> Result<(), SealedObservationFailure> {
        let actual = hash_exact_file(&mut self.file, self.identity.byte_len)
            .map_err(SealedObservationFailure::Io)?;
        if actual != self.identity || actual.sha256 != self.expected_sha256 {
            return Err(SealedObservationFailure::IdentityMismatch);
        }
        Ok(())
    }

    pub(crate) fn copy_to(
        &mut self,
        destination: &mut dyn Write,
    ) -> io::Result<SealedContentIdentity> {
        #[cfg(target_os = "linux")]
        {
            self.copy_to_typed(destination).map_err(|failure| {
                failure
                    .into_neutral_io_error("inherited sealed executable changed while being copied")
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.copy_to_untyped(destination)
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn copy_to_typed(
        &mut self,
        destination: &mut dyn Write,
    ) -> Result<SealedContentIdentity, SealedObservationFailure> {
        self.file.rewind().map_err(SealedObservationFailure::Io)?;
        let limit = self.identity.byte_len.checked_add(1).ok_or_else(|| {
            SealedObservationFailure::Io(invalid_data("sealed executable copy limit overflow"))
        })?;
        let mut reader = HashingReader::new((&mut self.file).take(limit));
        io::copy(&mut reader, destination).map_err(SealedObservationFailure::Io)?;
        let copied = reader.finish();
        self.file.rewind().map_err(SealedObservationFailure::Io)?;
        if copied != self.identity || copied.sha256 != self.expected_sha256 {
            return Err(SealedObservationFailure::IdentityMismatch);
        }
        Ok(copied)
    }

    #[cfg(not(target_os = "linux"))]
    fn copy_to_untyped(
        &mut self,
        destination: &mut dyn Write,
    ) -> io::Result<SealedContentIdentity> {
        self.file.rewind()?;
        let limit = self
            .identity
            .byte_len
            .checked_add(1)
            .ok_or_else(|| invalid_data("sealed executable copy limit overflow"))?;
        let mut reader = HashingReader::new((&mut self.file).take(limit));
        io::copy(&mut reader, destination)?;
        let copied = reader.finish();
        self.file.rewind()?;
        if copied != self.identity || copied.sha256 != self.expected_sha256 {
            return Err(invalid_data(
                "inherited sealed executable changed while being copied",
            ));
        }
        Ok(copied)
    }
}

fn hash_exact_file(file: &mut File, expected_len: u64) -> io::Result<SealedContentIdentity> {
    let limit = expected_len
        .checked_add(1)
        .ok_or_else(|| invalid_data("sealed executable read limit overflow"))?;
    file.rewind()?;
    let mut reader = HashingReader::new((&mut *file).take(limit));
    io::copy(&mut reader, &mut io::sink())?;
    let identity = reader.finish();
    file.rewind()?;
    if identity.byte_len != expected_len {
        return Err(invalid_data(
            "sealed executable length changed during observation",
        ));
    }
    Ok(identity)
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

    fn finish(self) -> SealedContentIdentity {
        SealedContentIdentity {
            byte_len: self.byte_len,
            sha256: format!("{:x}", self.hasher.finalize()),
        }
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.byte_len = self
            .byte_len
            .checked_add(
                u64::try_from(read)
                    .map_err(|_| invalid_data("sealed executable length overflow"))?,
            )
            .ok_or_else(|| invalid_data("sealed executable length overflow"))?;
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        ffi::{CString, OsStr},
        io::ErrorKind,
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::{ffi::OsStrExt, fs::MetadataExt, process::CommandExt},
        },
        time::Instant,
    };

    const BARRIER_READY_MAGIC: [u8; 8] = *b"BMP8BR01";
    const BARRIER_READY_FRAME_BYTES: usize = 24;
    pub(super) const BARRIER_RELEASE_TOKEN: u8 = 0xa5;

    static CLAIMED_DOMAINS: LazyLock<Mutex<BTreeSet<&'static str>>> =
        LazyLock::new(|| Mutex::new(BTreeSet::new()));

    struct ExecPointerArray(Vec<*const libc::c_char>);

    impl ExecPointerArray {
        fn as_ptr(&self) -> *const *const libc::c_char {
            self.0.as_ptr()
        }
    }

    // SAFETY: pointers refer only to immutable CString buffers captured by the same pre_exec
    // closure. Those buffers outlive the arrays and never move before fexecve.
    unsafe impl Send for ExecPointerArray {}
    // SAFETY: see Send; the closure only reads pointer values.
    unsafe impl Sync for ExecPointerArray {}

    enum EnvironmentPolicy<'a> {
        InheritFiltered(&'a [(String, String)]),
        Exact {
            values: &'a [(String, String)],
            expose_authority: bool,
        },
    }

    pub(super) fn prepare(
        retained: &mut RetainedExecutable,
        domain: SealedExecutionDomain,
        args: &[String],
        seals: libc::c_int,
    ) -> io::Result<PreparedSealedExecutable> {
        let parts = prepare_parts(
            retained,
            domain,
            args,
            seals,
            None,
            EnvironmentPolicy::InheritFiltered(&[]),
            Vec::new(),
        )?;
        Ok(PreparedSealedExecutable {
            command: parts.command,
            guard: parts.guard,
            identity: parts.identity,
        })
    }

    pub(super) fn prepare_with_barrier(
        retained: &mut RetainedExecutable,
        domain: SealedExecutionDomain,
        args: &[String],
        attempt_nonce: u64,
        join_process_group: Option<u32>,
        extra_environment: &[(String, String)],
        inheritable_files: Vec<File>,
    ) -> io::Result<PreparedLinuxBarrierSealedExecutable> {
        if attempt_nonce == 0 {
            return Err(invalid_input(
                "pre-exec barrier attempt nonce must be non-zero",
            ));
        }
        if join_process_group.is_some_and(|process_group| {
            process_group == 0 || i32::try_from(process_group).is_err()
        }) {
            return Err(invalid_input("pre-exec barrier process group is invalid"));
        }
        let (child_barrier, broker) = create_pre_exec_barrier(attempt_nonce, join_process_group)?;
        let parts = prepare_parts(
            retained,
            domain,
            args,
            required_seals(),
            Some(child_barrier),
            EnvironmentPolicy::InheritFiltered(extra_environment),
            inheritable_files,
        )?;
        Ok(PreparedLinuxBarrierSealedExecutable {
            command: parts.command,
            guard: parts.guard,
            identity: parts.identity,
            broker,
        })
    }

    pub(super) fn prepare_with_exact_environment(
        retained: &mut RetainedExecutable,
        domain: SealedExecutionDomain,
        args: &[String],
        exact_environment: &[(String, String)],
        inheritable_files: Vec<File>,
    ) -> io::Result<PreparedSealedExecutable> {
        let parts = prepare_parts(
            retained,
            domain,
            args,
            required_seals(),
            None,
            EnvironmentPolicy::Exact {
                values: exact_environment,
                expose_authority: true,
            },
            inheritable_files,
        )?;
        Ok(PreparedSealedExecutable {
            command: parts.command,
            guard: parts.guard,
            identity: parts.identity,
        })
    }

    pub(super) fn prepare_unclaimed_with_exact_environment(
        retained: &mut RetainedExecutable,
        domain: SealedExecutionDomain,
        args: &[String],
        exact_environment: &[(String, String)],
        inheritable_files: Vec<File>,
    ) -> io::Result<PreparedSealedExecutable> {
        let parts = prepare_parts(
            retained,
            domain,
            args,
            required_seals(),
            None,
            EnvironmentPolicy::Exact {
                values: exact_environment,
                expose_authority: false,
            },
            inheritable_files,
        )?;
        Ok(PreparedSealedExecutable {
            command: parts.command,
            guard: parts.guard,
            identity: parts.identity,
        })
    }

    struct PreparedParts {
        command: Command,
        guard: SealedLaunchGuard,
        identity: SealedContentIdentity,
    }

    struct ChildPreExecBarrier {
        ready_write: File,
        release_read: File,
        parent_ready_read_fd: libc::c_int,
        parent_release_write_fd: libc::c_int,
        attempt_nonce: u64,
        join_process_group: Option<u32>,
    }

    fn prepare_parts(
        retained: &mut RetainedExecutable,
        domain: SealedExecutionDomain,
        args: &[String],
        seals: libc::c_int,
        child_barrier: Option<ChildPreExecBarrier>,
        environment_policy: EnvironmentPolicy<'_>,
        mut inheritable_files: Vec<File>,
    ) -> io::Result<PreparedParts> {
        domain.validate()?;
        let name = CString::new(domain.claim_key).expect("validated static memfd name");
        // SAFETY: name is a valid C string and a successful syscall returns a new descriptor.
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
        // SAFETY: raw is the newly owned memfd returned above.
        let owned = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut target = File::from(owned);
        let mut source = retained.file.try_clone()?;
        source.rewind()?;
        let limit = retained
            .admitted_len
            .checked_add(1)
            .ok_or_else(|| invalid_data("sealed executable copy limit overflow"))?;
        let mut reader = HashingReader::new(source.take(limit));
        io::copy(&mut reader, &mut target)?;
        let identity = reader.finish();
        if identity.byte_len != retained.admitted_len {
            return Err(invalid_data(
                "retained executable length changed while sealing",
            ));
        }
        target.sync_all()?;
        // SAFETY: target is a live anonymous regular file owned by this process.
        if unsafe { libc::fchmod(target.as_raw_fd(), 0o500) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: F_ADD_SEALS applies the requested kernel-enforced seals to the live memfd.
        if unsafe { libc::fcntl(target.as_raw_fd(), libc::F_ADD_SEALS, seals) } != 0 {
            return Err(io::Error::last_os_error());
        }
        retained.verify_content(&identity)?;

        let inherited = duplicate_inheritable(&target)?;
        let mut argv_storage = Vec::with_capacity(args.len() + 1);
        argv_storage.push(CString::new(domain.argv0).expect("validated static argv0"));
        for arg in args {
            argv_storage.push(
                CString::new(arg.as_bytes())
                    .map_err(|_| invalid_input("sealed executable argument contains NUL"))?,
            );
        }
        let (mut environment, additional_environment, expose_authority_environment) =
            match environment_policy {
                EnvironmentPolicy::InheritFiltered(additional) => (
                    std::env::vars_os()
                        .filter(|(name, _)| !domain.is_reserved_env(name))
                        .map(|(name, value)| environment_field(&name, &value))
                        .collect::<io::Result<Vec<_>>>()?,
                    additional,
                    true,
                ),
                EnvironmentPolicy::Exact {
                    values,
                    expose_authority,
                } => (Vec::new(), values, expose_authority),
            };
        if expose_authority_environment {
            environment.push(authority_environment_field(
                domain.sha256_env,
                identity.sha256(),
            )?);
            environment.push(authority_environment_field(
                domain.locator_env,
                retained.locator.as_os_str().to_str().ok_or_else(|| {
                    invalid_input("sealed executable locator must be valid UTF-8")
                })?,
            )?);
            environment.push(authority_environment_field(
                domain.fd_env,
                &inherited.as_raw_fd().to_string(),
            )?);
        }
        let mut extra_names = BTreeSet::new();
        for (name, value) in additional_environment {
            if name == domain.fd_env
                || name == domain.locator_env
                || name == domain.sha256_env
                || !extra_names.insert(name)
            {
                return Err(invalid_input(
                    "sealed executable extra environment key is duplicated or reserved",
                ));
            }
            environment.push(authority_environment_field(name, value)?);
        }

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
        let argv = ExecPointerArray(argv);
        let envp = ExecPointerArray(envp);
        let exec_fd = target.as_raw_fd();
        let mut post_exec_allowlist = Vec::with_capacity(inheritable_files.len() + 1);
        post_exec_allowlist.push(inherited.as_raw_fd());
        post_exec_allowlist.extend(inheritable_files.iter().map(AsRawFd::as_raw_fd));
        post_exec_allowlist.sort_unstable();
        if post_exec_allowlist
            .windows(2)
            .any(|descriptors| descriptors[0] == descriptors[1])
            || post_exec_allowlist.iter().any(|descriptor| *descriptor < 3)
        {
            return Err(invalid_input(
                "sealed executable inherited descriptor allowlist is invalid",
            ));
        }
        let mut command = Command::new("/proc/self/exe");
        // SAFETY: all C strings and pointer arrays are allocated before fork and captured by the
        // closure. The optional barrier uses only raw async-signal-safe syscalls, and fexecve
        // replaces the child image on success.
        unsafe {
            command.pre_exec(move || {
                let _retained_buffers = (&argv_storage, &environment);
                if let Some(barrier) = &child_barrier {
                    child_wait_at_pre_exec_barrier(barrier)?;
                }
                close_ambient_descriptors_for_exec(&post_exec_allowlist)?;
                libc::fexecve(exec_fd, argv.as_ptr(), envp.as_ptr());
                Err(io::Error::last_os_error())
            });
        }
        let mut guard_files = vec![target, inherited];
        guard_files.append(&mut inheritable_files);
        Ok(PreparedParts {
            command,
            guard: SealedLaunchGuard { files: guard_files },
            identity,
        })
    }

    fn close_ambient_descriptors_for_exec(allowlist: &[libc::c_int]) -> io::Result<()> {
        // Mark every descriptor >= 3 CLOEXEC in one async-signal-safe syscall, then clear
        // CLOEXEC only for the exact capability descriptors admitted before fork.
        let result = unsafe {
            libc::syscall(
                libc::SYS_close_range,
                3_u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        for descriptor in allowlist {
            let flags = unsafe { libc::fcntl(*descriptor, libc::F_GETFD) };
            if flags < 0
                || unsafe { libc::fcntl(*descriptor, libc::F_SETFD, flags & !libc::FD_CLOEXEC) }
                    != 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn create_pre_exec_barrier(
        attempt_nonce: u64,
        join_process_group: Option<u32>,
    ) -> io::Result<(ChildPreExecBarrier, LinuxPreExecBarrierBroker)> {
        let (ready_read, ready_write) = cloexec_pipe()?;
        set_nonblocking(&ready_read)?;
        let (release_read, release_write) = cloexec_pipe()?;
        let child = ChildPreExecBarrier {
            parent_ready_read_fd: ready_read.as_raw_fd(),
            parent_release_write_fd: release_write.as_raw_fd(),
            ready_write,
            release_read,
            attempt_nonce,
            join_process_group,
        };
        let broker = LinuxPreExecBarrierBroker {
            ready: ready_read,
            release: Some(release_write),
            attempt_nonce,
            ready_pid: None,
            join_process_group,
        };
        Ok((child, broker))
    }

    fn cloexec_pipe() -> io::Result<(File, File)> {
        let mut descriptors = [-1_i32; 2];
        // SAFETY: descriptors names two writable integers and O_CLOEXEC is a valid pipe2 flag.
        if unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: pipe2 returned two distinct newly owned descriptors.
        let read = File::from(unsafe { OwnedFd::from_raw_fd(descriptors[0]) });
        // SAFETY: see above.
        let write = File::from(unsafe { OwnedFd::from_raw_fd(descriptors[1]) });
        Ok((read, write))
    }

    fn set_nonblocking(file: &File) -> io::Result<()> {
        // SAFETY: F_GETFL only observes flags on the live pipe read descriptor.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: F_SETFL updates only status flags on this read endpoint.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn child_wait_at_pre_exec_barrier(barrier: &ChildPreExecBarrier) -> io::Result<()> {
        // SAFETY: after fork these are the child copies of the broker-only pipe ends. Closing them
        // ensures a dropped broker release writer becomes EOF in the blocked child.
        unsafe {
            libc::close(barrier.parent_ready_read_fd);
            libc::close(barrier.parent_release_write_fd);
        }
        let process_group = barrier
            .join_process_group
            .map(i32::try_from)
            .transpose()
            .map_err(|_| io::Error::from_raw_os_error(libc::EOVERFLOW))?
            .unwrap_or(0);
        // SAFETY: the child has not executed workload code. It either creates its own process
        // group or joins the caller-selected group in the same session before publishing
        // readiness, so the broker and outer process-domain owner observe the exact same PGID.
        if unsafe { libc::setpgid(0, process_group) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: getpid has no failure mode and is async-signal-safe.
        let raw_pid = unsafe { libc::getpid() };
        let pid =
            u32::try_from(raw_pid).map_err(|_| io::Error::from_raw_os_error(libc::EOVERFLOW))?;
        if pid == 0 {
            return Err(io::Error::from_raw_os_error(libc::EOVERFLOW));
        }
        let mut frame = [0_u8; BARRIER_READY_FRAME_BYTES];
        frame[..8].copy_from_slice(&BARRIER_READY_MAGIC);
        frame[8..16].copy_from_slice(&barrier.attempt_nonce.to_le_bytes());
        frame[16..20].copy_from_slice(&pid.to_le_bytes());
        raw_write_all(barrier.ready_write.as_raw_fd(), &frame)?;
        let mut release = [0_u8; 1];
        raw_read_exact(barrier.release_read.as_raw_fd(), &mut release)?;
        if release != [BARRIER_RELEASE_TOKEN] {
            return Err(io::Error::from_raw_os_error(libc::ECANCELED));
        }
        Ok(())
    }

    fn raw_write_all(fd: libc::c_int, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            // SAFETY: bytes points to readable memory and fd is the child-owned pipe writer.
            let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
            if written > 0 {
                bytes = &bytes[usize::try_from(written).unwrap_or(bytes.len())..];
                continue;
            }
            if written == 0 {
                return Err(io::Error::from_raw_os_error(libc::EPIPE));
            }
            // SAFETY: this code is compiled only for Linux and reads thread-local errno.
            let errno = unsafe { *libc::__errno_location() };
            if errno != libc::EINTR {
                return Err(io::Error::from_raw_os_error(errno));
            }
        }
        Ok(())
    }

    fn raw_read_exact(fd: libc::c_int, mut bytes: &mut [u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            // SAFETY: bytes points to writable memory and fd is the child-owned pipe reader.
            let read = unsafe { libc::read(fd, bytes.as_mut_ptr().cast(), bytes.len()) };
            if read > 0 {
                let consumed = usize::try_from(read).unwrap_or(bytes.len());
                bytes = &mut bytes[consumed..];
                continue;
            }
            if read == 0 {
                return Err(io::Error::from_raw_os_error(libc::ECANCELED));
            }
            // SAFETY: this code is compiled only for Linux and reads thread-local errno.
            let errno = unsafe { *libc::__errno_location() };
            if errno != libc::EINTR {
                return Err(io::Error::from_raw_os_error(errno));
            }
        }
        Ok(())
    }

    pub(super) fn broker_wait_ready(
        broker: &mut LinuxPreExecBarrierBroker,
        timeout: Duration,
    ) -> io::Result<u32> {
        if timeout.is_zero() || broker.ready_pid.is_some() {
            return Err(invalid_input(
                "pre-exec barrier ready wait is invalid or repeated",
            ));
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| invalid_input("pre-exec barrier deadline overflow"))?;
        let mut frame = [0_u8; BARRIER_READY_FRAME_BYTES];
        let mut observed = 0_usize;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    ErrorKind::TimedOut,
                    "pre-exec barrier ready deadline elapsed",
                ));
            }
            let timeout_millis = i32::try_from(remaining.as_millis().max(1)).unwrap_or(i32::MAX);
            let mut descriptor = libc::pollfd {
                fd: broker.ready.as_raw_fd(),
                events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
                revents: 0,
            };
            // SAFETY: descriptor references one valid pollfd for the duration of the call.
            let polled = unsafe { libc::poll(&mut descriptor, 1, timeout_millis) };
            if polled == 0 {
                continue;
            }
            if polled < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if descriptor.revents & libc::POLLIN == 0 {
                return Err(invalid_data(
                    "pre-exec barrier ready pipe closed without a frame",
                ));
            }
            match broker.ready.read(&mut frame[observed..]) {
                Ok(0) => {
                    return Err(invalid_data(
                        "pre-exec barrier ready pipe closed during its frame",
                    ));
                }
                Ok(read) => {
                    observed = observed
                        .checked_add(read)
                        .ok_or_else(|| invalid_data("pre-exec barrier frame length overflow"))?;
                    if observed < frame.len() {
                        continue;
                    }
                }
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            }
            if frame[..8] != BARRIER_READY_MAGIC
                || u64::from_le_bytes(frame[8..16].try_into().expect("fixed nonce field"))
                    != broker.attempt_nonce
                || frame[20..24] != [0_u8; 4]
            {
                return Err(invalid_data("pre-exec barrier ready frame is invalid"));
            }
            let pid = u32::from_le_bytes(frame[16..20].try_into().expect("fixed PID field"));
            if pid == 0 {
                return Err(invalid_data("pre-exec barrier ready PID is zero"));
            }
            let raw_pid = i32::try_from(pid)
                .map_err(|_| invalid_data("pre-exec barrier ready PID is invalid"))?;
            // SAFETY: getpgid only observes the live blocked child identified by its ready frame.
            let observed_process_group = unsafe { libc::getpgid(raw_pid) };
            let expected_process_group = broker.join_process_group.unwrap_or(pid);
            if observed_process_group < 0
                || u32::try_from(observed_process_group).ok() != Some(expected_process_group)
            {
                return Err(invalid_data(
                    "pre-exec barrier child process group differs from its admitted domain",
                ));
            }
            broker.ready_pid = Some(pid);
            return Ok(pid);
        }
    }

    pub(super) const fn required_seals() -> libc::c_int {
        required_linux_seals()
    }

    pub(super) fn claim(
        domain: SealedExecutionDomain,
    ) -> Result<ClaimedSealedExecution, SealedClaimFailure> {
        domain.validate().map_err(SealedClaimFailure::Io)?;
        let mut claimed = CLAIMED_DOMAINS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !claimed.insert(domain.claim_key) {
            clear_authority_environment(domain);
            return Err(SealedClaimFailure::AlreadyConsumed);
        }
        let mut raw_fd = None;
        let result = (|| {
            let raw = std::env::var(domain.fd_env)
                .map_err(|_| SealedClaimFailure::DescriptorMissing)?
                .parse::<i32>()
                .map_err(|_| SealedClaimFailure::DescriptorInvalid)?;
            if raw < 3 {
                return Err(SealedClaimFailure::DescriptorReserved);
            }
            raw_fd = Some(raw);
            let locator = std::env::var_os(domain.locator_env)
                .map(PathBuf::from)
                .ok_or(SealedClaimFailure::LocatorMissing)?;
            if !locator.is_absolute() {
                return Err(SealedClaimFailure::LocatorNotAbsolute);
            }
            let expected_sha256 =
                std::env::var(domain.sha256_env).map_err(|_| SealedClaimFailure::Sha256Missing)?;
            validate_sha256(&expected_sha256).map_err(|_| SealedClaimFailure::Sha256Invalid)?;

            // SAFETY: F_DUPFD_CLOEXEC validates raw and returns a new descriptor on success.
            let duplicate = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 3) };
            if duplicate < 0 {
                return Err(SealedClaimFailure::Io(io::Error::last_os_error()));
            }
            // SAFETY: duplicate is newly owned.
            let duplicate = unsafe { OwnedFd::from_raw_fd(duplicate) };
            let mut file = File::from(duplicate);

            // The owned duplicate is the only authority needed from this point onward. Revoke the
            // inherited descriptor and its discoverable environment immediately, before any
            // metadata, current-object, or full-content validation that may take unbounded time.
            let inherited_raw = raw_fd.take().ok_or_else(|| {
                SealedClaimFailure::Io(invalid_data(
                    "sealed executable descriptor state is invalid",
                ))
            })?;
            // SAFETY: inherited_raw is the launcher-issued descriptor and has no Rust owner.
            let close_result = if unsafe { libc::close(inherited_raw) } != 0 {
                Err(SealedClaimFailure::Io(io::Error::last_os_error()))
            } else {
                Ok(())
            };
            clear_authority_environment(domain);
            close_result?;

            let metadata = file.metadata().map_err(SealedClaimFailure::Io)?;
            if !metadata.file_type().is_file() || metadata.mode() & 0o111 == 0 {
                return Err(SealedClaimFailure::NotExecutable);
            }
            // SAFETY: F_GET_SEALS only queries a live descriptor.
            let actual_seals = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) };
            let required_seals = required_linux_seals();
            if actual_seals < 0 {
                return Err(SealedClaimFailure::SealQueryFailed);
            }
            if actual_seals & required_seals != required_seals {
                return Err(SealedClaimFailure::MissingRequiredSeals);
            }
            let current = File::open("/proc/self/exe").map_err(SealedClaimFailure::Io)?;
            let current_metadata = current.metadata().map_err(SealedClaimFailure::Io)?;
            if metadata.dev() != current_metadata.dev() || metadata.ino() != current_metadata.ino()
            {
                return Err(SealedClaimFailure::NotCurrentExecutionObject);
            }
            let identity =
                hash_exact_file(&mut file, metadata.len()).map_err(SealedClaimFailure::Io)?;
            Ok(ClaimedSealedExecution {
                file,
                expected_sha256,
                locator,
                identity,
            })
        })();
        let revoke_result = if let Some(raw) = raw_fd {
            // SAFETY: raw is the launcher-issued inherited descriptor and has no Rust owner.
            if unsafe { libc::close(raw) } != 0 {
                Err(SealedClaimFailure::Io(io::Error::last_os_error()))
            } else {
                Ok(())
            }
        } else {
            Ok(())
        };
        clear_authority_environment(domain);
        drop(claimed);
        revoke_result?;
        result
    }

    fn duplicate_inheritable(file: &File) -> io::Result<File> {
        // SAFETY: F_DUPFD returns a new descriptor; unlike F_DUPFD_CLOEXEC it intentionally
        // remains open across fexecve so the child can claim the exact execution object.
        let raw = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 3) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: raw is newly owned.
        let owned = unsafe { OwnedFd::from_raw_fd(raw) };
        Ok(File::from(owned))
    }

    fn environment_field(name: &OsStr, value: &OsStr) -> io::Result<CString> {
        let mut field = name.as_bytes().to_vec();
        field.push(b'=');
        field.extend_from_slice(value.as_bytes());
        CString::new(field).map_err(|_| invalid_input("sealed execution environment contains NUL"))
    }

    fn authority_environment_field(name: &str, value: &str) -> io::Result<CString> {
        CString::new(format!("{name}={value}"))
            .map_err(|_| invalid_input("sealed execution authority contains NUL"))
    }

    fn clear_authority_environment(domain: SealedExecutionDomain) {
        std::env::remove_var(domain.fd_env);
        std::env::remove_var(domain.locator_env);
        std::env::remove_var(domain.sha256_env);
    }

    fn validate_sha256(value: &str) -> io::Result<()> {
        if value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            Ok(())
        } else {
            Err(invalid_data("sealed executable SHA256 is invalid"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_DOMAIN: SealedExecutionDomain = SealedExecutionDomain::new(
        "sealed-execution-test",
        "sealed-execution-test",
        "BM_TEST_SEALED_FD",
        "BM_TEST_SEALED_PATH",
        "BM_TEST_SEALED_SHA256",
        &["BM_TEST_SEALED_"],
    );

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_sealed_execution_fails_before_spawn() {
        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical executable");
        let mut retained = RetainedExecutable::open(&executable).expect("retain executable");
        let error = match retained.prepare(TEST_DOMAIN, &["--list".to_string()]) {
            Ok(_) => panic!("non-Linux must not prepare a sealed child"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(error.to_string().contains("Linux memfd/fexecve"));
    }

    #[test]
    fn domain_rejects_aliased_authority_keys() {
        let invalid = SealedExecutionDomain::new(
            "alias-test",
            "alias-test",
            "BM_TEST_ALIAS",
            "BM_TEST_ALIAS",
            "BM_TEST_SHA",
            &["BM_TEST_"],
        );
        assert_eq!(
            invalid
                .validate()
                .expect_err("aliased keys must fail")
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn peer_channel_exact_rights_and_mismatch_paths_leave_no_fd_delta() {
        fn fd_count() -> usize {
            std::fs::read_dir("/proc/self/fd")
                .expect("read process fd table")
                .count()
        }

        let (sender, receiver) = LinuxPeerBoundFdChannel::pair_with_timeout(Duration::from_secs(1))
            .expect("peer channel");
        let retained = File::open("/dev/null").expect("open retained fixture");
        let baseline = fd_count();
        sender
            .send_with_files(b"mismatch", &[&retained])
            .expect("send one descriptor");
        let error = receiver
            .receive_with_files(std::process::id(), 0)
            .expect_err("unexpected rights must fail");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fd_count(), baseline);

        sender
            .send_with_files(b"exact", &[&retained])
            .expect("send exact descriptor");
        let (bytes, files) = receiver
            .receive_with_files(std::process::id(), 1)
            .expect("receive exact descriptor");
        assert_eq!(bytes, b"exact");
        assert_eq!(files.len(), 1);
        drop(files);
        assert_eq!(fd_count(), baseline);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn peer_channel_deadline_is_fail_closed() {
        let (_sender, receiver) =
            LinuxPeerBoundFdChannel::pair_with_timeout(Duration::from_millis(1))
                .expect("peer channel");
        let error = receiver
            .receive_with_files(std::process::id(), 0)
            .expect_err("empty peer must reach the absolute deadline");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }
}
