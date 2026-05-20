//! Store helpers for Beetle Memory.
//!
//! Concrete deployments can provide their own stores. This crate ships neutral
//! in-memory stores that satisfy the SDK contracts for tests, embedded hosts,
//! and adapter bring-up.

use bm_core::platform::{SkillMetaStore, SkillStorage, StateFs};
use bm_core::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

#[derive(Default)]
pub struct InMemoryStateFs {
    files: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl StateFs for InMemoryStateFs {
    fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .files
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(rel_path)
            .cloned())
    }

    fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
        self.files
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(rel_path.to_string(), data.to_vec());
        Ok(())
    }

    fn remove(&self, rel_path: &str) -> Result<()> {
        self.files
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(rel_path);
        Ok(())
    }

    fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
        let prefix = rel_path.trim_end_matches('/');
        let prefix = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };
        let mut out = self
            .files
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .keys()
            .filter_map(|key| key.strip_prefix(&prefix).map(ToString::to_string))
            .collect::<Vec<_>>();
        out.sort();
        Ok(out)
    }
}

#[derive(Default)]
pub struct InMemorySkillStorage {
    entries: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl SkillStorage for InMemorySkillStorage {
    fn list_names(&self) -> Result<Vec<String>> {
        Ok(self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .keys()
            .cloned()
            .collect())
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        Ok(self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(name)
            .cloned()
            .unwrap_or_default())
    }

    fn write(&self, name: &str, content: &[u8]) -> Result<()> {
        self.entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(name.to_string(), content.to_vec());
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<()> {
        self.entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(name);
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemorySkillMetaStore {
    order: Mutex<Vec<String>>,
    disabled: Mutex<BTreeSet<String>>,
}

impl SkillMetaStore for InMemorySkillMetaStore {
    fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)> {
        Ok((
            self.order
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
            self.disabled
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .iter()
                .cloned()
                .collect(),
        ))
    }

    fn write_meta(&self, order: &[String], disabled: &[String]) -> Result<()> {
        *self.order.lock().unwrap_or_else(|error| error.into_inner()) = order.to_vec();
        *self
            .disabled
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = disabled.iter().cloned().collect();
        Ok(())
    }
}
