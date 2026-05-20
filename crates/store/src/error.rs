use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreRepairReport {
    pub checked: bool,
    pub repaired: bool,
    pub findings: Vec<String>,
}

impl StoreRepairReport {
    pub fn clean() -> Self {
        Self {
            checked: true,
            repaired: false,
            findings: Vec::new(),
        }
    }

    pub fn report_only(finding: impl Into<String>) -> Self {
        Self {
            checked: true,
            repaired: false,
            findings: vec![finding.into()],
        }
    }
}

impl Default for StoreRepairReport {
    fn default() -> Self {
        Self::clean()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreOpenReport {
    pub backend: String,
    pub schema_id: String,
    pub repair: StoreRepairReport,
}

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("store config: {0}")]
    Config(String),
    #[error("store schema: {0}")]
    Schema(String),
    #[error("store corruption: {0}")]
    Corruption(String),
    #[error("store quota: {0}")]
    Quota(String),
    #[error("store unsupported profile: {0}")]
    UnsupportedProfile(String),
    #[error("store serde: {0}")]
    Serde(String),
    #[error("store sqlite: {0}")]
    Sqlite(String),
}
