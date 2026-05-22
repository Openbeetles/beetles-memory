use std::fs;
use std::hash::Hasher;
use std::io::Read;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{OllamaTransparentConfig, OllamaTransparentError, Result};

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
            message: None,
        }
    }
}

pub trait RunnerInstaller {
    fn inspect(&self, config: &OllamaTransparentConfig) -> Result<ManagedRunnerReport>;

    fn ensure_installed(&self, config: &OllamaTransparentConfig) -> Result<ManagedRunnerReport>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FileSystemRunnerInstaller;

impl RunnerInstaller for FileSystemRunnerInstaller {
    fn inspect(&self, config: &OllamaTransparentConfig) -> Result<ManagedRunnerReport> {
        config.validate()?;
        let source_exists = config.official_ollama_binary.is_file();
        let managed_exists = config.managed_runner_path.is_file();
        let source_digest = if source_exists {
            Some(digest_file(&config.official_ollama_binary)?)
        } else {
            None
        };
        let managed_digest = if managed_exists {
            Some(digest_file(&config.managed_runner_path)?)
        } else {
            None
        };
        let installed = source_digest.is_some() && source_digest == managed_digest;
        Ok(ManagedRunnerReport {
            source_path: config.official_ollama_binary.clone(),
            managed_path: config.managed_runner_path.clone(),
            source_exists,
            managed_exists,
            installed,
            source_digest: source_digest.clone(),
            managed_digest: managed_digest.clone(),
            copy_digest: managed_digest,
            message: if installed {
                None
            } else {
                Some("managed runner is missing or differs from official Ollama binary".to_string())
            },
        })
    }

    fn ensure_installed(&self, config: &OllamaTransparentConfig) -> Result<ManagedRunnerReport> {
        config.validate()?;
        let before = self.inspect(config)?;
        if before.installed {
            return Ok(before);
        }
        if !before.source_exists {
            return Err(OllamaTransparentError::runner_install_failed(format!(
                "official Ollama binary does not exist: {}",
                config.official_ollama_binary.display()
            )));
        }
        if let Some(parent) = config.managed_runner_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                OllamaTransparentError::runner_install_failed(format!(
                    "failed to create managed runner directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        fs::copy(&config.official_ollama_binary, &config.managed_runner_path).map_err(|error| {
            OllamaTransparentError::runner_install_failed(format!(
                "failed to copy official Ollama binary to managed runner path: {error}"
            ))
        })?;
        self.inspect(config)
    }
}

fn digest_file(path: &PathBuf) -> Result<String> {
    let mut file = fs::File::open(path).map_err(|error| {
        OllamaTransparentError::runner_install_failed(format!(
            "failed to open {} for digest: {error}",
            path.display()
        ))
    })?;
    let mut hasher = Fnv1a64::default();
    let mut buf = [0_u8; 8192];
    loop {
        let read = file.read(&mut buf).map_err(|error| {
            OllamaTransparentError::runner_install_failed(format!(
                "failed to read {} for digest: {error}",
                path.display()
            ))
        })?;
        if read == 0 {
            break;
        }
        hasher.write(&buf[..read]);
    }
    Ok(format!("fnv1a64:{:016x}", hasher.finish()))
}

#[derive(Debug)]
struct Fnv1a64(u64);

impl Default for Fnv1a64 {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for Fnv1a64 {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}
