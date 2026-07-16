use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    port_owner::observe_process, runner::open_secure_lock_file, ExecutableFileIdentity,
    OllamaTransparentError, Result,
};

const MAX_LEASE_RECEIPT_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransitionProcessReceipt {
    version: u32,
    pid: u32,
    start_identity: String,
    executable_path: PathBuf,
    executable_identity: ExecutableFileIdentity,
}

pub(crate) struct OsTransitionLease {
    _file: File,
    _receipt: TransitionProcessReceipt,
}

impl OsTransitionLease {
    pub(crate) fn acquire(path: &Path) -> Result<Self> {
        let mut file = open_secure_lock_file(path)
            .map_err(|error| OllamaTransparentError::transition_lease_failed(error.to_string()))?;
        try_lock_exclusive(&file).map_err(|lock_error| {
            let receipt = read_receipt(&mut file).ok();
            let detail = match receipt {
                Some(receipt) if receipt_matches_live_process(&receipt) => format!(
                    "active transition is owned by verified pid {} start {}",
                    receipt.pid, receipt.start_identity
                ),
                Some(receipt) => format!(
                    "transition lock is held but persisted receipt for pid {} is not live-exact",
                    receipt.pid
                ),
                None => {
                    "transition lock is held but its receipt is unavailable or invalid".to_string()
                }
            };
            OllamaTransparentError::transition_lease_failed(format!(
                "failed to acquire OS transition lease: {lock_error}; {detail}"
            ))
        })?;

        let receipt = current_process_receipt()?;
        write_receipt(&mut file, &receipt)?;
        let persisted = read_receipt(&mut file)?;
        if persisted != receipt || !receipt_matches_live_process(&persisted) {
            return Err(OllamaTransparentError::transition_lease_failed(
                "persisted transition receipt failed exact readback validation",
            ));
        }
        Ok(Self {
            _file: file,
            _receipt: receipt,
        })
    }
}

fn current_process_receipt() -> Result<TransitionProcessReceipt> {
    let pid = std::process::id();
    let observed = observe_process(pid, None).ok_or_else(|| {
        OllamaTransparentError::transition_lease_failed(
            "failed to observe current process exact executable/start identity",
        )
    })?;
    Ok(TransitionProcessReceipt {
        version: 1,
        pid,
        start_identity: observed.start_identity.ok_or_else(|| {
            OllamaTransparentError::transition_lease_failed(
                "current process start identity is unavailable",
            )
        })?,
        executable_path: observed.executable,
        executable_identity: observed.executable_identity.ok_or_else(|| {
            OllamaTransparentError::transition_lease_failed(
                "current process executable identity is unavailable",
            )
        })?,
    })
}

fn receipt_matches_live_process(receipt: &TransitionProcessReceipt) -> bool {
    if receipt.version != 1 {
        return false;
    }
    let Some(observed) = observe_process(receipt.pid, None) else {
        return false;
    };
    observed.start_identity.as_deref() == Some(receipt.start_identity.as_str())
        && observed.executable == receipt.executable_path
        && observed.executable_identity.as_ref() == Some(&receipt.executable_identity)
}

fn write_receipt(file: &mut File, receipt: &TransitionProcessReceipt) -> Result<()> {
    let encoded = serde_json::to_vec(receipt).map_err(|error| {
        OllamaTransparentError::transition_lease_failed(format!(
            "failed to encode transition receipt: {error}"
        ))
    })?;
    if encoded.len() as u64 > MAX_LEASE_RECEIPT_BYTES {
        return Err(OllamaTransparentError::transition_lease_failed(
            "transition receipt exceeds its byte budget",
        ));
    }
    file.set_len(0).map_err(lease_io("truncate receipt"))?;
    file.seek(SeekFrom::Start(0))
        .map_err(lease_io("rewind receipt"))?;
    file.write_all(&encoded)
        .map_err(lease_io("write receipt"))?;
    file.sync_all().map_err(lease_io("fsync receipt"))
}

fn read_receipt(file: &mut File) -> Result<TransitionProcessReceipt> {
    file.seek(SeekFrom::Start(0))
        .map_err(lease_io("rewind receipt"))?;
    let mut bytes = Vec::with_capacity(1024);
    file.take(MAX_LEASE_RECEIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(lease_io("read receipt"))?;
    if bytes.len() as u64 > MAX_LEASE_RECEIPT_BYTES {
        return Err(OllamaTransparentError::transition_lease_failed(
            "transition receipt exceeds its byte budget",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        OllamaTransparentError::transition_lease_failed(format!(
            "failed to decode transition receipt: {error}"
        ))
    })
}

fn lease_io(action: &'static str) -> impl FnOnce(std::io::Error) -> OllamaTransparentError {
    move |error| {
        OllamaTransparentError::transition_lease_failed(format!("failed to {action}: {error}"))
    }
}

#[cfg(unix)]
fn try_lock_exclusive(file: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn try_lock_exclusive(_file: &File) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "OS transition lease is unavailable",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn cross_process_lease_rejects_second_controller() {
        if let Some(path) = std::env::var_os("BM_OLLAMA_LEASE_CHILD_PATH") {
            let error = OsTransitionLease::acquire(Path::new(&path))
                .err()
                .expect("child must not acquire held lease");
            assert!(error.message().contains("verified pid"), "{error}");
            return;
        }
        let root = test_root("cross-process");
        let path = root.join("transition.lock");
        let lease = OsTransitionLease::acquire(&path).expect("parent lease");
        assert_eq!(lease._receipt.pid, std::process::id());

        let status = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "lease::tests::cross_process_lease_rejects_second_controller",
                "--nocapture",
            ])
            .env("BM_OLLAMA_LEASE_CHILD_PATH", &path)
            .status()
            .expect("child lease probe");

        assert!(status.success());
        drop(lease);
        std::fs::remove_dir_all(root).expect("lease test cleanup");
    }

    #[test]
    fn receipt_validation_rejects_aba_start_and_executable_identity_changes() {
        let receipt = current_process_receipt().expect("current receipt");
        assert!(receipt_matches_live_process(&receipt));

        let mut reused_pid = receipt.clone();
        reused_pid.start_identity.push_str("-reused");
        assert!(!receipt_matches_live_process(&reused_pid));

        let mut replaced_executable = receipt.clone();
        replaced_executable.executable_identity.inode = replaced_executable
            .executable_identity
            .inode
            .wrapping_add(1);
        assert!(!receipt_matches_live_process(&replaced_executable));
    }

    fn test_root(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "bm-ollama-lease-{label}-{}-{}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("lease test root");
        std::fs::canonicalize(path).expect("canonical lease test root")
    }
}
