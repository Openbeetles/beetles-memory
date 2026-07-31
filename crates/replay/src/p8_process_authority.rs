//! One-shot parent authority for the private P8 verifier subprocess.
//!
//! The public operator is the only entry point allowed to mint this authority. The child consumes
//! it before reading artifacts or creating staging files, clears every reserved environment key,
//! and proves that its live parent is the same executable. This is an engineering process
//! boundary; trusted Linux sealed-execution remains a separate P8.5 quality-release requirement.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::p8_semantic::P8VerifierIdentityV1;

const PARENT_PID_ENV: &str = "BM_P8_INTERNAL_PARENT_PID";
const AUTHORITY_TOKEN_ENV: &str = "BM_P8_INTERNAL_AUTHORITY_TOKEN";
const PARENT_EXECUTABLE_IDENTITY_ENV: &str = "BM_P8_INTERNAL_PARENT_EXECUTABLE_IDENTITY";
const RESERVED_ENV: [&str; 3] = [
    PARENT_PID_ENV,
    AUTHORITY_TOKEN_ENV,
    PARENT_EXECUTABLE_IDENTITY_ENV,
];

static CLAIMED: AtomicBool = AtomicBool::new(false);

pub(crate) fn authorize_internal_child(
    command: &mut Command,
    child_arguments: &[&OsStr],
) -> Result<P8VerifierIdentityV1, String> {
    for name in RESERVED_ENV {
        command.env_remove(name);
    }
    let parent_identity = P8VerifierIdentityV1::for_current_process()
        .map_err(|failures| format!("P8 parent executable identity failed: {failures:?}"))?;
    let parent_pid = std::process::id();
    let issued_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "P8 internal authority clock is before the Unix epoch".to_string())?
        .as_nanos();
    let mut hasher = Sha256::new();
    hasher.update(b"beetle_memory_p8_internal_process_authority_v1");
    hasher.update(parent_pid.to_be_bytes());
    hasher.update(issued_at.to_be_bytes());
    hasher.update(parent_identity.identity_digest().as_str().as_bytes());
    for argument in child_arguments {
        let bytes = argument.as_encoded_bytes();
        hasher.update(
            u64::try_from(bytes.len())
                .map_err(|_| "P8 internal authority argument length overflow".to_string())?
                .to_be_bytes(),
        );
        hasher.update(bytes);
    }
    let token = format!("{:x}", hasher.finalize());
    platform::install_authority_pipe(command, token.as_bytes())?;
    command
        .env(PARENT_PID_ENV, parent_pid.to_string())
        .env(AUTHORITY_TOKEN_ENV, token)
        .env(
            PARENT_EXECUTABLE_IDENTITY_ENV,
            parent_identity.executable_identity.as_str(),
        );
    Ok(parent_identity)
}

pub(crate) fn claim_internal_child_authority() -> Result<P8VerifierIdentityV1, String> {
    if CLAIMED.swap(true, Ordering::AcqRel) {
        clear_reserved_environment();
        return Err("P8 internal process authority was already consumed".into());
    }
    let parent_pid = std::env::var(PARENT_PID_ENV)
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let token = std::env::var(AUTHORITY_TOKEN_ENV).ok();
    let expected_parent_executable_identity = std::env::var(PARENT_EXECUTABLE_IDENTITY_ENV).ok();
    clear_reserved_environment();

    let parent_pid =
        parent_pid.ok_or_else(|| "P8 internal process authority parent is missing".to_string())?;
    let token =
        token.ok_or_else(|| "P8 internal process authority token is missing".to_string())?;
    if token.len() != 64
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("P8 internal process authority token is malformed".into());
    }
    if platform::parent_pid()? != parent_pid {
        return Err("P8 internal process authority parent PID does not match".into());
    }
    let parent_executable = platform::parent_executable(parent_pid)?;
    let child_executable =
        std::env::current_exe().map_err(|_| "P8 child executable path is unavailable")?;
    let parent_executable = std::fs::canonicalize(&parent_executable)
        .map_err(|_| "P8 live parent executable path cannot be canonicalized")?;
    let child_executable = std::fs::canonicalize(&child_executable)
        .map_err(|_| "P8 child executable path cannot be canonicalized")?;
    if parent_executable != child_executable {
        return Err("P8 internal child parent path is not the same verifier executable".into());
    }
    let mut inherited_token = Vec::with_capacity(65);
    std::io::stdin()
        .take(66)
        .read_to_end(&mut inherited_token)
        .map_err(|_| "P8 inherited authority pipe could not be consumed".to_string())?;
    if inherited_token.len() != 65
        || inherited_token.last() != Some(&b'\n')
        || inherited_token[..64] != token.as_bytes()[..]
    {
        return Err("P8 inherited authority pipe does not match the one-shot token".into());
    }
    let child_identity = P8VerifierIdentityV1::for_executable(&child_executable)
        .map_err(|failures| format!("P8 child executable identity failed: {failures:?}"))?;
    if expected_parent_executable_identity.as_deref()
        != Some(child_identity.executable_identity.as_str())
    {
        return Err("P8 internal process authority parent executable does not match".into());
    }
    Ok(child_identity)
}

fn clear_reserved_environment() {
    for name in RESERVED_ENV {
        // SAFETY: the P8 operator claims authority at single-threaded process entry before any
        // worker threads are created.
        unsafe { std::env::remove_var(name) };
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::os::fd::{FromRawFd, OwnedFd};

    pub(super) fn install_authority_pipe(
        command: &mut Command,
        token: &[u8],
    ) -> Result<(), String> {
        let mut descriptors = [-1; 2];
        // SAFETY: descriptors points to storage for exactly two pipe descriptors.
        if unsafe { libc::pipe(descriptors.as_mut_ptr()) } != 0 {
            return Err("P8 internal authority pipe could not be created".into());
        }
        // SAFETY: a successful pipe call returned two newly owned descriptors.
        let reader = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
        // SAFETY: a successful pipe call returned two newly owned descriptors.
        let writer = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };
        let mut writer = std::fs::File::from(writer);
        writer
            .write_all(token)
            .and_then(|()| writer.write_all(b"\n"))
            .map_err(|_| "P8 internal authority pipe could not be sealed".to_string())?;
        drop(writer);
        command.stdin(Stdio::from(std::fs::File::from(reader)));
        Ok(())
    }

    pub(super) fn parent_pid() -> Result<u32, String> {
        // SAFETY: getppid has no preconditions.
        u32::try_from(unsafe { libc::getppid() })
            .map_err(|_| "P8 parent PID cannot be represented".to_string())
    }

    #[cfg(target_os = "linux")]
    pub(super) fn parent_executable(parent_pid: u32) -> Result<PathBuf, String> {
        std::fs::read_link(format!("/proc/{parent_pid}/exe"))
            .map_err(|_| "P8 live parent executable path is unavailable".to_string())
    }

    #[cfg(target_os = "macos")]
    pub(super) fn parent_executable(parent_pid: u32) -> Result<PathBuf, String> {
        const PROC_PIDPATHINFO_MAXSIZE: usize = 4096;
        let mut buffer = vec![0_u8; PROC_PIDPATHINFO_MAXSIZE];
        // SAFETY: buffer is live and its exact capacity is supplied to proc_pidpath.
        let written = unsafe {
            libc::proc_pidpath(
                i32::try_from(parent_pid).map_err(|_| "P8 parent PID exceeds i32".to_string())?,
                buffer.as_mut_ptr().cast(),
                u32::try_from(buffer.len())
                    .map_err(|_| "P8 parent executable buffer is too large".to_string())?,
            )
        };
        if written <= 0 {
            return Err("P8 live parent executable path is unavailable".into());
        }
        let written = usize::try_from(written)
            .map_err(|_| "P8 parent executable path length is invalid".to_string())?;
        buffer.truncate(written);
        let path = std::str::from_utf8(&buffer)
            .map_err(|_| "P8 parent executable path is not UTF-8".to_string())?;
        Ok(PathBuf::from(path))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub(super) fn parent_executable(_parent_pid: u32) -> Result<PathBuf, String> {
        Err("P8 internal process authority has no parent executable broker on this Unix".into())
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Security::SECURITY_ATTRIBUTES,
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Pipes::CreatePipe,
            Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    };

    pub(super) fn install_authority_pipe(
        command: &mut Command,
        token: &[u8],
    ) -> Result<(), String> {
        let mut read_handle = std::ptr::null_mut();
        let mut write_handle = std::ptr::null_mut();
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
                .map_err(|_| "P8 authority security attributes overflow".to_string())?,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        // SAFETY: handle outputs and security attributes are live.
        if unsafe { CreatePipe(&mut read_handle, &mut write_handle, &attributes, 0) } == 0 {
            return Err("P8 internal authority pipe could not be created".into());
        }
        // SAFETY: CreatePipe returned newly owned handles.
        let reader = unsafe { OwnedHandle::from_raw_handle(read_handle) };
        // SAFETY: CreatePipe returned newly owned handles.
        let writer = unsafe { OwnedHandle::from_raw_handle(write_handle) };
        let mut writer = std::fs::File::from(writer);
        writer
            .write_all(token)
            .and_then(|()| writer.write_all(b"\n"))
            .map_err(|_| "P8 internal authority pipe could not be sealed".to_string())?;
        drop(writer);
        command.stdin(Stdio::from(std::fs::File::from(reader)));
        Ok(())
    }

    pub(super) fn parent_pid() -> Result<u32, String> {
        // SAFETY: system-wide process enumeration accepts a zero process id.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("P8 process snapshot is unavailable".into());
        }
        let mut entry = unsafe { std::mem::zeroed::<PROCESSENTRY32W>() };
        entry.dwSize = u32::try_from(size_of::<PROCESSENTRY32W>())
            .map_err(|_| "P8 process entry size overflow".to_string())?;
        // SAFETY: snapshot and entry are live.
        let mut present = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        let current = std::process::id();
        while present {
            if entry.th32ProcessID == current {
                // SAFETY: snapshot is a live ToolHelp handle.
                unsafe { CloseHandle(snapshot) };
                return Ok(entry.th32ParentProcessID);
            }
            // SAFETY: snapshot and entry are live.
            present = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        // SAFETY: snapshot is a live ToolHelp handle.
        unsafe { CloseHandle(snapshot) };
        Err("P8 live parent PID is unavailable".into())
    }

    pub(super) fn parent_executable(parent_pid: u32) -> Result<PathBuf, String> {
        // SAFETY: requested access is read-only process metadata.
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, parent_pid) };
        if raw.is_null() {
            return Err("P8 live parent process cannot be opened".into());
        }
        // SAFETY: OpenProcess returned an owned handle.
        let process = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut buffer = vec![0_u16; 32_768];
        let mut length = u32::try_from(buffer.len())
            .map_err(|_| "P8 parent executable buffer is too large".to_string())?;
        // SAFETY: process and output buffer are live.
        if unsafe {
            QueryFullProcessImageNameW(
                process.as_raw_handle(),
                PROCESS_NAME_WIN32,
                buffer.as_mut_ptr(),
                &mut length,
            )
        } == 0
        {
            return Err("P8 live parent executable path is unavailable".into());
        }
        buffer.truncate(
            usize::try_from(length)
                .map_err(|_| "P8 parent executable path length overflow".to_string())?,
        );
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer)))
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use super::*;

    pub(super) fn install_authority_pipe(
        _command: &mut Command,
        _token: &[u8],
    ) -> Result<(), String> {
        Err("P8 internal process authority has no inherited pipe broker".into())
    }

    pub(super) fn parent_pid() -> Result<u32, String> {
        Err("P8 internal process authority has no parent PID broker".into())
    }

    pub(super) fn parent_executable(_parent_pid: u32) -> Result<PathBuf, String> {
        Err("P8 internal process authority has no parent executable broker".into())
    }
}
