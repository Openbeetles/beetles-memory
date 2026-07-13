//! Typed before/after images shared by governed persistence closure validators.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernedDocumentImage<T> {
    pub physical_key: String,
    pub before: Option<T>,
    pub after: Option<T>,
}

impl<T> GovernedDocumentImage<T> {
    pub fn created(physical_key: impl Into<String>, after: T) -> Self {
        Self {
            physical_key: physical_key.into(),
            before: None,
            after: Some(after),
        }
    }

    pub fn updated(physical_key: impl Into<String>, before: T, after: T) -> Self {
        Self {
            physical_key: physical_key.into(),
            before: Some(before),
            after: Some(after),
        }
    }

    pub fn deleted(physical_key: impl Into<String>, before: T) -> Self {
        Self {
            physical_key: physical_key.into(),
            before: Some(before),
            after: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GovernedPostImageValidation {
    pub accepted: bool,
    pub failures: Vec<String>,
}

impl GovernedPostImageValidation {
    pub(crate) fn from_failures(mut failures: Vec<String>) -> Self {
        failures.sort();
        failures.dedup();
        Self {
            accepted: failures.is_empty(),
            failures,
        }
    }
}

pub(crate) fn revision_is_exact_successor(before: Option<u64>, after: Option<u64>) -> bool {
    match (before, after) {
        (None, Some(1)) => true,
        (Some(before), Some(after)) => before.checked_add(1) == Some(after),
        (Some(_), None) => true,
        (None, None) => false,
        (None, Some(_)) => false,
    }
}
