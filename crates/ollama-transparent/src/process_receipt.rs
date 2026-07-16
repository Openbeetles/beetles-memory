use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::runner::{normal_file_name, secure_open_directory};
use crate::{
    process_authority::PersistedProcessAuthority, ManagedProcessKind, ObservedProcess,
    OllamaTransparentError, Result,
};

const RECEIPT_SCHEMA_VERSION: u32 = 3;
const MAX_RECEIPT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ManagedProcessReceiptBook {
    schema_version: u32,
    managed_upstream: Option<ManagedProcessControlRecord>,
    transparent_front: Option<ManagedProcessControlRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ManagedProcessControlRecord {
    pub(crate) process: ObservedProcess,
    pub(crate) authority: Option<PersistedProcessAuthority>,
}

impl ManagedProcessControlRecord {
    pub(crate) fn new(
        process: ObservedProcess,
        authority: Option<PersistedProcessAuthority>,
    ) -> Self {
        Self { process, authority }
    }
}

impl ManagedProcessReceiptBook {
    pub(crate) fn get(&self, kind: ManagedProcessKind) -> Option<&ManagedProcessControlRecord> {
        match kind {
            ManagedProcessKind::ManagedUpstream => self.managed_upstream.as_ref(),
            ManagedProcessKind::TransparentFront => self.transparent_front.as_ref(),
        }
    }

    pub(crate) fn set(
        &mut self,
        kind: ManagedProcessKind,
        record: Option<ManagedProcessControlRecord>,
    ) {
        self.schema_version = RECEIPT_SCHEMA_VERSION;
        match kind {
            ManagedProcessKind::ManagedUpstream => self.managed_upstream = record,
            ManagedProcessKind::TransparentFront => self.transparent_front = record,
        }
    }
}

pub(crate) fn read_receipt_book(path: &Path) -> Result<ManagedProcessReceiptBook> {
    let parent_path = path.parent().ok_or_else(receipt_parent_error)?;
    let name = normal_file_name(path).map_err(receipt_error)?;
    let parent = match secure_open_directory(parent_path, false) {
        Ok(parent) => parent,
        Err(error) if error.message().contains("does not exist") => {
            return Ok(ManagedProcessReceiptBook::default());
        }
        Err(error) => return Err(receipt_error(error)),
    };
    validate_owner_directory(&parent)?;
    let Some(mut file) = openat_optional(&parent, &name)? else {
        return Ok(ManagedProcessReceiptBook::default());
    };
    validate_owner_file(&file)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_RECEIPT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| receipt_io("read", error))?;
    if bytes.len() > MAX_RECEIPT_BYTES {
        return Err(OllamaTransparentError::process_action_failed(
            "managed process receipt exceeds byte budget",
        ));
    }
    let book: ManagedProcessReceiptBook = serde_json::from_slice(&bytes).map_err(|error| {
        OllamaTransparentError::process_action_failed(format!(
            "managed process receipt is invalid: {error}"
        ))
    })?;
    if book.schema_version != RECEIPT_SCHEMA_VERSION {
        return Err(OllamaTransparentError::process_action_failed(
            "managed process receipt schema is unsupported",
        ));
    }
    Ok(book)
}

pub(crate) fn write_receipt_book(path: &Path, book: &ManagedProcessReceiptBook) -> Result<()> {
    let parent_path = path.parent().ok_or_else(receipt_parent_error)?;
    let name = normal_file_name(path).map_err(receipt_error)?;
    let parent = secure_open_directory(parent_path, true).map_err(receipt_error)?;
    validate_owner_directory(&parent)?;
    let bytes = serde_json::to_vec(book).map_err(|error| {
        OllamaTransparentError::process_action_failed(format!(
            "managed process receipt serialization failed: {error}"
        ))
    })?;
    if bytes.len() > MAX_RECEIPT_BYTES {
        return Err(OllamaTransparentError::process_action_failed(
            "managed process receipt exceeds byte budget",
        ));
    }
    let temporary = format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let mut staging = createat_new(&parent, &temporary)?;
    let result = (|| {
        staging
            .write_all(&bytes)
            .and_then(|_| staging.sync_all())
            .map_err(|error| receipt_io("write", error))?;
        renameat_replace(&parent, &temporary, &name)?;
        parent
            .sync_all()
            .map_err(|error| receipt_io("sync directory", error))
    })();
    if result.is_err() {
        let _ = unlinkat(&parent, &temporary);
    }
    result
}

fn receipt_parent_error() -> OllamaTransparentError {
    OllamaTransparentError::process_action_failed(
        "managed process receipt path must have a parent directory",
    )
}

fn receipt_error(error: OllamaTransparentError) -> OllamaTransparentError {
    OllamaTransparentError::process_action_failed(format!(
        "managed process receipt secure path failed: {error}"
    ))
}

fn receipt_io(action: &str, error: std::io::Error) -> OllamaTransparentError {
    OllamaTransparentError::process_action_failed(format!(
        "managed process receipt {action} failed: {error}"
    ))
}

#[cfg(unix)]
fn validate_owner_directory(directory: &File) -> Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;
    let metadata = directory
        .metadata()
        .map_err(|error| receipt_io("directory metadata", error))?;
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(OllamaTransparentError::process_action_failed(
            "managed process receipt directory is not owned by current user",
        ));
    }
    if unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } != 0 {
        return Err(receipt_io(
            "directory permissions",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_owner_directory(_directory: &File) -> Result<()> {
    Err(OllamaTransparentError::unsupported(
        "managed process receipts require secure Unix directory handles",
    ))
}

#[cfg(unix)]
fn validate_owner_file(file: &File) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file
        .metadata()
        .map_err(|error| receipt_io("file metadata", error))?;
    if !metadata.file_type().is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(OllamaTransparentError::process_action_failed(
            "managed process receipt must be an owner-only regular file",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_owner_file(_file: &File) -> Result<()> {
    Err(OllamaTransparentError::unsupported(
        "managed process receipts require secure Unix file handles",
    ))
}

#[cfg(unix)]
fn openat_optional(parent: &File, name: &str) -> Result<Option<File>> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = CString::new(name).expect("validated receipt name");
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
            Err(receipt_io("open", error))
        }
    }
}

#[cfg(not(unix))]
fn openat_optional(_parent: &File, _name: &str) -> Result<Option<File>> {
    Err(OllamaTransparentError::unsupported(
        "managed process receipts require openat",
    ))
}

#[cfg(unix)]
fn createat_new(parent: &File, name: &str) -> Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = CString::new(name).expect("generated receipt name");
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        Err(receipt_io(
            "create staging",
            std::io::Error::last_os_error(),
        ))
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

#[cfg(not(unix))]
fn createat_new(_parent: &File, _name: &str) -> Result<File> {
    Err(OllamaTransparentError::unsupported(
        "managed process receipts require openat",
    ))
}

#[cfg(unix)]
fn renameat_replace(parent: &File, source: &str, destination: &str) -> Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    let source = CString::new(source).expect("generated receipt name");
    let destination = CString::new(destination).expect("validated receipt name");
    if unsafe {
        libc::renameat(
            parent.as_raw_fd(),
            source.as_ptr(),
            parent.as_raw_fd(),
            destination.as_ptr(),
        )
    } == 0
    {
        Ok(())
    } else {
        Err(receipt_io("publish", std::io::Error::last_os_error()))
    }
}

#[cfg(not(unix))]
fn renameat_replace(_parent: &File, _source: &str, _destination: &str) -> Result<()> {
    Err(OllamaTransparentError::unsupported(
        "managed process receipts require renameat",
    ))
}

#[cfg(unix)]
fn unlinkat(parent: &File, name: &str) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    let name = CString::new(name).expect("generated receipt name");
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn unlinkat(_parent: &File, _name: &str) -> std::io::Result<()> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, MetadataExt};

    fn root(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "bm-process-receipt-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir(&path).expect("test root");
        std::fs::canonicalize(path).expect("canonical test root")
    }

    fn receipt(pid: u32) -> ObservedProcess {
        ObservedProcess::new(pid, "managed", "/tmp/managed")
            .with_start_identity(format!("start-{pid}"))
    }

    #[test]
    fn receipt_book_roundtrips_atomically_with_owner_only_permissions() {
        let root = root("roundtrip");
        let path = root.join("control").join("managed-processes.json");
        let mut book = ManagedProcessReceiptBook::default();
        book.set(
            ManagedProcessKind::ManagedUpstream,
            Some(ManagedProcessControlRecord::new(receipt(41), None)),
        );
        write_receipt_book(&path, &book).expect("write receipt book");

        assert_eq!(read_receipt_book(&path).expect("read receipt book"), book);
        assert_eq!(
            std::fs::metadata(path.parent().expect("receipt parent"))
                .expect("parent metadata")
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).expect("receipt metadata").mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn receipt_book_rejects_unknown_fields_and_symlink_leaf() {
        let root = root("fail-closed");
        let control = root.join("control");
        std::fs::create_dir(&control).expect("control directory");
        let path = control.join("managed-processes.json");
        std::fs::write(&path, br#"{"schemaVersion":1,"unknown":true}"#).expect("unknown receipt");
        assert!(read_receipt_book(&path).is_err());

        std::fs::remove_file(&path).expect("remove unknown receipt");
        let target = control.join("target");
        std::fs::write(&target, b"{}").expect("symlink target");
        symlink(&target, &path).expect("receipt symlink");
        assert!(read_receipt_book(&path).is_err());
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
