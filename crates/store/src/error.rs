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

    pub fn quota_pressure(
        plane: impl AsRef<str>,
        attempted: usize,
        limit: usize,
        operation: impl AsRef<str>,
        pressure_after_import: bool,
    ) -> Self {
        StoreQuotaViolation::new(plane.as_ref(), attempted, limit, operation.as_ref())
            .with_pressure_after_import(pressure_after_import)
            .to_repair_report()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreQuotaViolation {
    pub plane: String,
    pub attempted: usize,
    pub limit: usize,
    pub operation: String,
    pub pressure_after_import: bool,
}

impl StoreQuotaViolation {
    pub fn new(
        plane: impl Into<String>,
        attempted: usize,
        limit: usize,
        operation: impl Into<String>,
    ) -> Self {
        Self {
            plane: plane.into(),
            attempted,
            limit,
            operation: operation.into(),
            pressure_after_import: false,
        }
    }

    pub fn with_pressure_after_import(mut self, pressure_after_import: bool) -> Self {
        self.pressure_after_import = pressure_after_import;
        self
    }

    pub fn to_repair_report(&self) -> StoreRepairReport {
        StoreRepairReport::report_only(format!(
            "quota_exceeded plane={} operation={} attempted={} limit={} pressure_after_import={} host_deletion_allowed=false",
            self.plane,
            self.operation,
            self.attempted,
            self.limit,
            self.pressure_after_import
        ))
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
