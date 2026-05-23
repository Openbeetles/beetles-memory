use crate::orchestrator::PressureLevel;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeResourceProbeSource {
    Unavailable,
    StaticManifest,
    FirmwareManifest,
    HostMacos,
    HostLinux,
    HostWindows,
    HostOther,
    Injected,
    Cached,
}

impl RuntimeResourceProbeSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::StaticManifest => "static_manifest",
            Self::FirmwareManifest => "firmware_manifest",
            Self::HostMacos => "host_macos",
            Self::HostLinux => "host_linux",
            Self::HostWindows => "host_windows",
            Self::HostOther => "host_other",
            Self::Injected => "injected",
            Self::Cached => "cached",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeResourceUnavailableReason {
    ProbeNotConfigured,
    ProbeFailed,
    SnapshotStale,
    MemoryUnavailable,
    StorageUnavailable,
}

impl RuntimeResourceUnavailableReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProbeNotConfigured => "probe_not_configured",
            Self::ProbeFailed => "probe_failed",
            Self::SnapshotStale => "snapshot_stale",
            Self::MemoryUnavailable => "memory_unavailable",
            Self::StorageUnavailable => "storage_unavailable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeResourceSnapshot {
    pub observed_at_unix_secs: u64,
    pub ttl_ms: u64,
    pub source: RuntimeResourceProbeSource,
    pub stale: bool,
    pub pressure: PressureLevel,
    pub available_parallelism: Option<u16>,
    pub memory_total_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
    pub internal_heap_free_bytes: Option<u64>,
    pub internal_heap_minimum_free_bytes: Option<u64>,
    pub internal_heap_largest_block_bytes: Option<u64>,
    pub psram_total_bytes: Option<u64>,
    pub psram_free_bytes: Option<u64>,
    pub psram_largest_block_bytes: Option<u64>,
    pub storage_total_bytes: Option<u64>,
    pub storage_available_bytes: Option<u64>,
    pub active_http_count: u32,
    pub active_wss_count: u32,
    pub active_runtime_jobs: u32,
    pub inbound_queue_depth: u32,
    pub outbound_queue_depth: u32,
    pub tls_fragmentation_risk: bool,
    pub storage_contention_risk: bool,
    pub unavailable_reason: Option<RuntimeResourceUnavailableReason>,
    pub unavailable_detail: Option<String>,
}

impl RuntimeResourceSnapshot {
    pub fn unavailable(
        observed_at_unix_secs: u64,
        source: RuntimeResourceProbeSource,
        reason: RuntimeResourceUnavailableReason,
    ) -> Self {
        Self {
            observed_at_unix_secs,
            ttl_ms: 30_000,
            source,
            stale: false,
            pressure: PressureLevel::Cautious,
            available_parallelism: std::thread::available_parallelism()
                .ok()
                .and_then(|value| u16::try_from(value.get()).ok()),
            memory_total_bytes: None,
            memory_available_bytes: None,
            internal_heap_free_bytes: None,
            internal_heap_minimum_free_bytes: None,
            internal_heap_largest_block_bytes: None,
            psram_total_bytes: None,
            psram_free_bytes: None,
            psram_largest_block_bytes: None,
            storage_total_bytes: None,
            storage_available_bytes: None,
            active_http_count: 0,
            active_wss_count: 0,
            active_runtime_jobs: 0,
            inbound_queue_depth: 0,
            outbound_queue_depth: 0,
            tls_fragmentation_risk: false,
            storage_contention_risk: false,
            unavailable_reason: Some(reason),
            unavailable_detail: None,
        }
    }

    pub fn with_unavailable_detail(mut self, detail: impl Into<String>) -> Self {
        self.unavailable_detail = Some(detail.into());
        self
    }

    pub fn mark_stale(mut self, now_secs: u64) -> Self {
        self.stale = true;
        self.observed_at_unix_secs = now_secs;
        self.unavailable_reason = Some(RuntimeResourceUnavailableReason::SnapshotStale);
        self
    }

    pub fn is_expired(&self, now_secs: u64) -> bool {
        let ttl_secs = self.ttl_ms.div_ceil(1000);
        now_secs.saturating_sub(self.observed_at_unix_secs) > ttl_secs
    }

    pub fn host_probe(now_secs: u64) -> Self {
        let (memory_total_bytes, memory_available_bytes) = host_memory_bytes();
        let (storage_total_bytes, storage_available_bytes) = host_storage_bytes();
        let available_parallelism = std::thread::available_parallelism()
            .ok()
            .and_then(|value| u16::try_from(value.get()).ok());
        let pressure = pressure_from_resources(memory_total_bytes, memory_available_bytes);
        let source = host_probe_source();
        let mut snapshot = Self {
            observed_at_unix_secs: now_secs,
            ttl_ms: 30_000,
            source,
            stale: false,
            pressure,
            available_parallelism,
            memory_total_bytes,
            memory_available_bytes,
            internal_heap_free_bytes: memory_available_bytes,
            internal_heap_minimum_free_bytes: memory_available_bytes,
            internal_heap_largest_block_bytes: memory_available_bytes,
            psram_total_bytes: None,
            psram_free_bytes: None,
            psram_largest_block_bytes: None,
            storage_total_bytes,
            storage_available_bytes,
            active_http_count: 0,
            active_wss_count: 0,
            active_runtime_jobs: 0,
            inbound_queue_depth: 0,
            outbound_queue_depth: 0,
            tls_fragmentation_risk: false,
            storage_contention_risk: false,
            unavailable_reason: None,
            unavailable_detail: None,
        };
        if memory_total_bytes.is_none() && memory_available_bytes.is_none() {
            snapshot.unavailable_reason = Some(RuntimeResourceUnavailableReason::MemoryUnavailable);
        }
        if storage_total_bytes.is_none() && storage_available_bytes.is_none() {
            snapshot.unavailable_reason = snapshot
                .unavailable_reason
                .or(Some(RuntimeResourceUnavailableReason::StorageUnavailable));
        }
        snapshot
    }
}

pub trait RuntimeResourceProbe: Send + Sync {
    fn probe(&self, now_secs: u64) -> Result<RuntimeResourceSnapshot>;
}

#[derive(Clone, Debug)]
pub struct UnavailableRuntimeResourceProbe {
    source: RuntimeResourceProbeSource,
    reason: RuntimeResourceUnavailableReason,
}

impl UnavailableRuntimeResourceProbe {
    pub const fn new(
        source: RuntimeResourceProbeSource,
        reason: RuntimeResourceUnavailableReason,
    ) -> Self {
        Self { source, reason }
    }
}

impl Default for UnavailableRuntimeResourceProbe {
    fn default() -> Self {
        Self::new(
            RuntimeResourceProbeSource::Unavailable,
            RuntimeResourceUnavailableReason::ProbeNotConfigured,
        )
    }
}

impl RuntimeResourceProbe for UnavailableRuntimeResourceProbe {
    fn probe(&self, now_secs: u64) -> Result<RuntimeResourceSnapshot> {
        Ok(RuntimeResourceSnapshot::unavailable(
            now_secs,
            self.source,
            self.reason,
        ))
    }
}

#[derive(Clone, Debug)]
pub struct HostRuntimeResourceProbe;

impl RuntimeResourceProbe for HostRuntimeResourceProbe {
    fn probe(&self, now_secs: u64) -> Result<RuntimeResourceSnapshot> {
        Ok(RuntimeResourceSnapshot::host_probe(now_secs))
    }
}

#[derive(Clone, Debug)]
pub struct StaticRuntimeResourceProbe {
    snapshot: RuntimeResourceSnapshot,
}

impl StaticRuntimeResourceProbe {
    pub fn new(snapshot: RuntimeResourceSnapshot) -> Self {
        Self { snapshot }
    }
}

impl RuntimeResourceProbe for StaticRuntimeResourceProbe {
    fn probe(&self, _now_secs: u64) -> Result<RuntimeResourceSnapshot> {
        Ok(self.snapshot.clone())
    }
}

#[derive(Debug)]
pub struct RuntimeResourceSnapshotCache {
    snapshot: Mutex<RuntimeResourceSnapshot>,
}

impl RuntimeResourceSnapshotCache {
    pub fn new(snapshot: RuntimeResourceSnapshot) -> Self {
        Self {
            snapshot: Mutex::new(snapshot),
        }
    }

    pub fn current(&self, now_secs: u64) -> RuntimeResourceSnapshot {
        let snapshot = self.snapshot.lock().expect("resource snapshot cache");
        if snapshot.is_expired(now_secs) {
            return snapshot.clone().mark_stale(now_secs);
        }
        snapshot.clone()
    }

    pub fn refresh_from_probe(
        &self,
        probe: &dyn RuntimeResourceProbe,
        now_secs: u64,
    ) -> Result<RuntimeResourceSnapshot> {
        let snapshot = probe.probe(now_secs)?;
        *self.snapshot.lock().expect("resource snapshot cache") = snapshot.clone();
        Ok(snapshot)
    }
}

pub fn probe_host_runtime_resource(now_secs: u64) -> RuntimeResourceSnapshot {
    RuntimeResourceSnapshot::host_probe(now_secs)
}

fn pressure_from_resources(
    memory_total_bytes: Option<u64>,
    memory_available_bytes: Option<u64>,
) -> PressureLevel {
    let Some(available) = memory_available_bytes else {
        return PressureLevel::Cautious;
    };
    if available < 64 * 1024 * 1024 {
        return PressureLevel::Critical;
    }
    if available < 256 * 1024 * 1024 {
        return PressureLevel::Cautious;
    }
    if let Some(total) = memory_total_bytes {
        if total > 0 && available.saturating_mul(100) / total < 5 {
            return PressureLevel::Critical;
        }
        if total > 0 && available.saturating_mul(100) / total < 15 {
            return PressureLevel::Cautious;
        }
    }
    PressureLevel::Normal
}

fn host_probe_source() -> RuntimeResourceProbeSource {
    match std::env::consts::OS {
        "macos" => RuntimeResourceProbeSource::HostMacos,
        "linux" => RuntimeResourceProbeSource::HostLinux,
        "windows" => RuntimeResourceProbeSource::HostWindows,
        _ => RuntimeResourceProbeSource::HostOther,
    }
}

fn host_memory_bytes() -> (Option<u64>, Option<u64>) {
    #[cfg(target_os = "linux")]
    {
        return linux_memory_bytes();
    }
    #[cfg(target_os = "macos")]
    {
        return macos_memory_bytes();
    }
    #[allow(unreachable_code)]
    (None, None)
}

#[cfg(target_os = "linux")]
fn linux_memory_bytes() -> (Option<u64>, Option<u64>) {
    let Ok(text) = std::fs::read_to_string("/proc/meminfo") else {
        return (None, None);
    };
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        if let Some(value) = parse_meminfo_kib(line, "MemTotal:") {
            total = Some(value * 1024);
        }
        if let Some(value) = parse_meminfo_kib(line, "MemAvailable:") {
            available = Some(value * 1024);
        }
    }
    (total, available)
}

#[cfg(target_os = "linux")]
fn parse_meminfo_kib(line: &str, prefix: &str) -> Option<u64> {
    let rest = line.strip_prefix(prefix)?.trim();
    rest.split_whitespace().next()?.parse::<u64>().ok()
}

#[cfg(target_os = "macos")]
fn macos_memory_bytes() -> (Option<u64>, Option<u64>) {
    let total = command_output("/usr/sbin/sysctl", &["-n", "hw.memsize"])
        .and_then(|text| text.trim().parse::<u64>().ok());
    let available = macos_vm_stat_available_bytes();
    (total, available)
}

#[cfg(target_os = "macos")]
fn macos_vm_stat_available_bytes() -> Option<u64> {
    let text = command_output("/usr/bin/vm_stat", &[])?;
    let page_size = text
        .lines()
        .next()
        .and_then(|line| line.split("page size of ").nth(1))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(4096);
    let mut pages = 0_u64;
    for prefix in ["Pages free:", "Pages inactive:", "Pages speculative:"] {
        pages = pages.saturating_add(parse_vm_stat_pages(&text, prefix).unwrap_or(0));
    }
    Some(pages.saturating_mul(page_size))
}

#[cfg(target_os = "macos")]
fn parse_vm_stat_pages(text: &str, prefix: &str) -> Option<u64> {
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix(prefix) {
            let number = rest.trim().trim_end_matches('.').replace('.', "");
            return number.parse::<u64>().ok();
        }
    }
    None
}

fn host_storage_bytes() -> (Option<u64>, Option<u64>) {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        return df_storage_bytes(".");
    }
    #[allow(unreachable_code)]
    (None, None)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn df_storage_bytes(path: &str) -> (Option<u64>, Option<u64>) {
    let Some(text) =
        command_output("/bin/df", &["-k", path]).or_else(|| command_output("df", &["-k", path]))
    else {
        return (None, None);
    };
    let Some(line) = text.lines().last() else {
        return (None, None);
    };
    let mut parts = line.split_whitespace();
    let _filesystem = parts.next();
    let total = parts.next().and_then(|value| value.parse::<u64>().ok());
    let _used = parts.next();
    let available = parts.next().and_then(|value| value.parse::<u64>().ok());
    (
        total.map(|value| value * 1024),
        available.map(|value| value * 1024),
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

pub fn probe_error(detail: impl Into<String>) -> Error {
    Error::config("runtime_resource_probe", detail.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_probe_never_reports_full_budget() {
        let probe = UnavailableRuntimeResourceProbe::default();
        let snapshot = probe.probe(10).unwrap();
        assert_eq!(
            snapshot.unavailable_reason,
            Some(RuntimeResourceUnavailableReason::ProbeNotConfigured)
        );
        assert_eq!(snapshot.memory_available_bytes, None);
        assert_eq!(snapshot.pressure, PressureLevel::Cautious);
    }

    #[test]
    fn stale_cache_marks_snapshot_without_reprobing() {
        let snapshot = RuntimeResourceSnapshot::unavailable(
            1,
            RuntimeResourceProbeSource::Unavailable,
            RuntimeResourceUnavailableReason::ProbeNotConfigured,
        );
        let cache = RuntimeResourceSnapshotCache::new(snapshot);
        let stale = cache.current(60);
        assert!(stale.stale);
        assert_eq!(
            stale.unavailable_reason,
            Some(RuntimeResourceUnavailableReason::SnapshotStale)
        );
    }
}
