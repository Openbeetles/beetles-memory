//! Retained P8 artifact-directory authority.
//!
//! This is deliberately a P8 byte-authority wrapper over the generation-neutral retained
//! filesystem primitive. P8 schema, identities, admissions, and receipts remain owned by
//! `p8_semantic`.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Read, Seek};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::retained_artifact_fs::RetainedArtifactDirectory;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum P8ObservedFileStability {
    #[cfg(unix)]
    Unix {
        device: u64,
        inode: u64,
        bytes: u64,
        modified_seconds: i64,
        modified_nanoseconds: i64,
        changed_seconds: i64,
        changed_nanoseconds: i64,
    },
    #[cfg(windows)]
    Windows {
        bytes: u64,
        creation_time: i64,
        last_write_time: i64,
        change_time: i64,
        file_attributes: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct P8ObservedContentIdentity {
    stability: P8ObservedFileStability,
    digest: [u8; 32],
}

pub(crate) struct P8RetainedArtifactDirectory {
    retained: RetainedArtifactDirectory,
}

impl P8RetainedArtifactDirectory {
    pub(crate) fn open(root: &Path) -> io::Result<Self> {
        Ok(Self {
            retained: RetainedArtifactDirectory::open_root(root)?,
        })
    }

    pub(crate) fn open_verified_file(&self, file_name: &str) -> io::Result<File> {
        let file = self.retained.open_existing_read_stable_file(file_name)?;
        self.retained.verify_file_identity(file_name, &file)?;
        Ok(file)
    }

    pub(crate) fn open_terminal_stage(&self, file_name: &str) -> io::Result<File> {
        let file = self.retained.open_existing_terminal_stage(file_name)?;
        self.retained.verify_file_identity(file_name, &file)?;
        Ok(file)
    }

    pub(crate) fn create_terminal_stage(&self, file_name: &str) -> io::Result<File> {
        self.retained.create_new_terminal_stage(file_name)
    }

    pub(crate) fn verify_file_identity(&self, file_name: &str, file: &File) -> io::Result<()> {
        self.retained.verify_file_identity(file_name, file)
    }

    pub(crate) fn exact_regular_file_names(&self) -> io::Result<BTreeSet<String>> {
        self.retained.verify_unchanged()?;
        let names = self.retained.exact_regular_file_names()?;
        self.retained.verify_unchanged()?;
        Ok(names)
    }

    pub(crate) fn verify_unchanged(&self) -> io::Result<()> {
        self.retained.verify_unchanged()
    }

    pub(crate) fn install_file_no_replace_terminal(
        &self,
        staged_file: &File,
        staged_name: &str,
        final_name: &str,
        expected_content: &P8ObservedContentIdentity,
        content_limit: u64,
        verify_deadline: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        self.retained.install_file_no_replace_terminal(
            staged_file,
            staged_name,
            final_name,
            || {
                if observed_content_identity(staged_file, content_limit)? != *expected_content {
                    return Err(io::Error::other(
                        "terminal stage content identity drifted before commit",
                    ));
                }
                Ok(())
            },
            verify_deadline,
        )
    }

    pub(crate) fn discard_same_file(
        &self,
        staged_file: &File,
        staged_name: &str,
    ) -> io::Result<()> {
        self.retained.discard_same_file(staged_file, staged_name)
    }
}

pub(crate) fn observed_file_stability(file: &File) -> io::Result<P8ObservedFileStability> {
    let metadata = file.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(P8ObservedFileStability::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
            bytes: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
        })
    }
    #[cfg(windows)]
    {
        use std::mem::{size_of, MaybeUninit};
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileBasicInfo, GetFileInformationByHandleEx, FILE_BASIC_INFO,
        };
        let mut info = MaybeUninit::<FILE_BASIC_INFO>::uninit();
        // SAFETY: file owns a live handle and info has the layout requested by FileBasicInfo.
        let result = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileBasicInfo,
                info.as_mut_ptr().cast(),
                u32::try_from(size_of::<FILE_BASIC_INFO>())
                    .map_err(|_| io::Error::other("P8 content identity size overflow"))?,
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful GetFileInformationByHandleEx initialized info.
        let info = unsafe { info.assume_init() };
        Ok(P8ObservedFileStability::Windows {
            bytes: metadata.len(),
            creation_time: info.CreationTime,
            last_write_time: info.LastWriteTime,
            change_time: info.ChangeTime,
            file_attributes: info.FileAttributes,
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "P8 observed content identity is unsupported on this platform",
        ))
    }
}

pub(crate) fn observed_content_identity(
    file: &File,
    content_limit: u64,
) -> io::Result<P8ObservedContentIdentity> {
    const DOMAIN: &[u8] = b"beetle_memory_observed_terminal_stage_v1";
    let stability = observed_file_stability(file)?;
    let declared_bytes = file.metadata()?.len();
    if declared_bytes > content_limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "terminal stage content length is invalid",
        ));
    }
    let mut reader = file.try_clone()?;
    reader.rewind()?;
    let mut hasher = Sha256::new();
    hasher.update(
        u64::try_from(DOMAIN.len())
            .map_err(|_| io::Error::other("terminal content domain length overflow"))?
            .to_be_bytes(),
    );
    hasher.update(DOMAIN);
    hasher.update(declared_bytes.to_be_bytes());
    let mut observed_bytes = 0_u64;
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        observed_bytes = observed_bytes
            .checked_add(
                u64::try_from(read)
                    .map_err(|_| io::Error::other("terminal content length overflow"))?,
            )
            .ok_or_else(|| io::Error::other("terminal content length overflow"))?;
        if observed_bytes > declared_bytes || observed_bytes > content_limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "terminal stage content grew during observation",
            ));
        }
        hasher.update(&chunk[..read]);
    }
    if observed_bytes != declared_bytes || observed_file_stability(file)? != stability {
        return Err(io::Error::other(
            "terminal stage content drifted during observation",
        ));
    }
    Ok(P8ObservedContentIdentity {
        stability,
        digest: hasher.finalize().into(),
    })
}

pub(crate) fn observed_content_identity_for_bytes(
    file: &File,
    validated_bytes: &[u8],
    content_limit: u64,
) -> io::Result<P8ObservedContentIdentity> {
    const DOMAIN: &[u8] = b"beetle_memory_observed_terminal_stage_v1";
    let identity = observed_content_identity(file, content_limit)?;
    let expected_len = u64::try_from(validated_bytes.len())
        .map_err(|_| io::Error::other("validated terminal content length overflow"))?;
    if expected_len != file.metadata()?.len() {
        return Err(io::Error::other(
            "validated terminal bytes differ from retained length",
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(
        u64::try_from(DOMAIN.len())
            .map_err(|_| io::Error::other("terminal content domain length overflow"))?
            .to_be_bytes(),
    );
    hasher.update(DOMAIN);
    hasher.update(expected_len.to_be_bytes());
    hasher.update(validated_bytes);
    let expected_digest: [u8; 32] = hasher.finalize().into();
    if identity.digest != expected_digest {
        return Err(io::Error::other(
            "validated terminal bytes differ from retained content",
        ));
    }
    Ok(identity)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("wall clock")
            .as_nanos();
        fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!(
                "bm-p8-retained-dir-{label}-{}-{nonce}",
                std::process::id()
            ))
    }

    #[test]
    fn p8_retained_directory_detects_root_rename_and_recreate() {
        let root = fixture_root("root-swap");
        let displaced = root.with_extension("displaced");
        fs::create_dir(&root).expect("create retained root");
        fs::write(root.join("artifact.json"), b"admitted").expect("write admitted artifact");
        let retained = P8RetainedArtifactDirectory::open(&root).expect("retain root");
        assert_eq!(
            retained
                .exact_regular_file_names()
                .expect("enumerate retained root"),
            BTreeSet::from(["artifact.json".to_string()])
        );

        fs::rename(&root, &displaced).expect("displace retained root");
        fs::create_dir(&root).expect("recreate pathname root");
        fs::write(root.join("artifact.json"), b"replacement").expect("write replacement artifact");
        assert!(retained.verify_unchanged().is_err());
        assert!(retained.exact_regular_file_names().is_err());

        fs::remove_dir_all(&root).expect("remove replacement root");
        fs::remove_dir_all(&displaced).expect("remove retained root");
    }

    #[test]
    fn p8_retained_directory_rejects_child_replacement_and_symlink() {
        let root = fixture_root("child-swap");
        fs::create_dir(&root).expect("create retained root");
        let path = root.join("artifact.json");
        fs::write(&path, b"admitted").expect("write admitted artifact");
        let retained = P8RetainedArtifactDirectory::open(&root).expect("retain root");
        let admitted = retained
            .open_verified_file("artifact.json")
            .expect("retain admitted artifact");

        fs::rename(&path, root.join("admitted.json")).expect("displace admitted path");
        fs::write(&path, b"replacement").expect("write replacement artifact");
        assert!(retained
            .verify_file_identity("artifact.json", &admitted)
            .is_err());
        fs::remove_file(&path).expect("remove replacement");
        symlink(root.join("admitted.json"), &path).expect("install symlink");
        assert!(retained.open_verified_file("artifact.json").is_err());

        fs::remove_dir_all(&root).expect("remove retained fixture");
    }

    #[test]
    fn p8_terminal_no_replace_commit_never_overwrites_a_winner() {
        let root = fixture_root("terminal-race");
        fs::create_dir(&root).expect("create retained root");
        fs::write(root.join("first.stage"), b"first").expect("write first stage");
        fs::write(root.join("second.stage"), b"second").expect("write second stage");
        let retained = P8RetainedArtifactDirectory::open(&root).expect("retain root");
        let first = retained
            .open_terminal_stage("first.stage")
            .expect("open first stage");
        let second = retained
            .open_terminal_stage("second.stage")
            .expect("open second stage");
        let first_identity =
            observed_content_identity(&first, 1024).expect("first content identity");
        let second_identity =
            observed_content_identity(&second, 1024).expect("second content identity");

        retained
            .install_file_no_replace_terminal(
                &first,
                "first.stage",
                "report.json",
                &first_identity,
                1024,
                || Ok(()),
            )
            .expect("first terminal commit");
        assert!(!root.join("first.stage").exists());
        assert_eq!(
            fs::read(root.join("report.json")).expect("read committed report"),
            b"first"
        );
        retained
            .install_file_no_replace_terminal(
                &second,
                "second.stage",
                "report.json",
                &second_identity,
                1024,
                || Ok(()),
            )
            .expect_err("second terminal commit must not replace winner");
        assert_eq!(
            fs::read(root.join("report.json")).expect("winner remains"),
            b"first"
        );
        assert_eq!(
            fs::read(root.join("second.stage")).expect("losing stage remains"),
            b"second"
        );

        fs::remove_dir_all(&root).expect("remove retained fixture");
    }

    #[test]
    fn p8_terminal_commit_rejects_same_inode_rewrite_and_source_replacement() {
        let root = fixture_root("terminal-drift");
        fs::create_dir(&root).expect("create retained root");
        let retained = P8RetainedArtifactDirectory::open(&root).expect("retain root");

        fs::write(root.join("rewrite.stage"), b"admitted").expect("write rewrite stage");
        let rewrite = retained
            .open_terminal_stage("rewrite.stage")
            .expect("open rewrite stage");
        let rewrite_identity =
            observed_content_identity(&rewrite, 1024).expect("freeze rewrite identity");
        fs::write(root.join("rewrite.stage"), b"rewritte").expect("same-size rewrite");
        retained
            .install_file_no_replace_terminal(
                &rewrite,
                "rewrite.stage",
                "rewrite.json",
                &rewrite_identity,
                1024,
                || Ok(()),
            )
            .expect_err("same-inode rewrite must not commit");
        assert!(!root.join("rewrite.json").exists());
        assert_eq!(
            fs::read(root.join("rewrite.stage")).expect("rewritten source remains"),
            b"rewritte"
        );

        fs::write(root.join("replace.stage"), b"admitted").expect("write replace stage");
        let replace = retained
            .open_terminal_stage("replace.stage")
            .expect("open replace stage");
        let replace_identity =
            observed_content_identity(&replace, 1024).expect("freeze replace identity");
        fs::rename(root.join("replace.stage"), root.join("replace.displaced"))
            .expect("displace source pathname");
        fs::write(root.join("replace.stage"), b"replacement").expect("write replacement source");
        retained
            .install_file_no_replace_terminal(
                &replace,
                "replace.stage",
                "replace.json",
                &replace_identity,
                1024,
                || Ok(()),
            )
            .expect_err("source replacement must not commit");
        assert!(!root.join("replace.json").exists());
        assert_eq!(
            fs::read(root.join("replace.stage")).expect("replacement source remains"),
            b"replacement"
        );

        fs::write(root.join("callback.stage"), b"admitted").expect("write callback stage");
        let callback = retained
            .open_terminal_stage("callback.stage")
            .expect("open callback stage");
        let callback_identity =
            observed_content_identity(&callback, 1024).expect("freeze callback identity");
        retained
            .install_file_no_replace_terminal(
                &callback,
                "callback.stage",
                "callback.json",
                &callback_identity,
                1024,
                || {
                    fs::rename(root.join("callback.stage"), root.join("callback.displaced"))?;
                    fs::write(root.join("callback.stage"), b"replacement")?;
                    Ok(())
                },
            )
            .expect_err("source replacement during final callback must not commit");
        assert!(!root.join("callback.json").exists());
        assert_eq!(
            fs::read(root.join("callback.stage")).expect("callback replacement remains"),
            b"replacement"
        );

        fs::remove_dir_all(&root).expect("remove retained fixture");
    }

    #[test]
    fn p8_terminal_identity_is_bound_to_the_validated_readback_bytes() {
        let root = fixture_root("validated-readback");
        fs::create_dir(&root).expect("create retained root");
        fs::write(root.join("report.stage"), b"validated").expect("write report stage");
        let retained = P8RetainedArtifactDirectory::open(&root).expect("retain root");
        let report = retained
            .open_terminal_stage("report.stage")
            .expect("open report stage");
        observed_content_identity_for_bytes(&report, b"validated", 1024)
            .expect("exact validated readback");
        observed_content_identity_for_bytes(&report, b"rewritten", 1024)
            .expect_err("different same-size bytes must not freeze as validated");
        fs::remove_dir_all(&root).expect("remove retained fixture");
    }
}
