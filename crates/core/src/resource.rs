use crate::orchestrator::PressureLevel;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
pub struct RuntimeResourceObservation {
    pub observed_at_unix_secs: u64,
    pub ttl_ms: u64,
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
    pub unavailable_reason: Option<RuntimeResourceUnavailableReason>,
    pub unavailable_detail: Option<String>,
}

impl RuntimeResourceObservation {
    pub fn unavailable(
        observed_at_unix_secs: u64,
        reason: RuntimeResourceUnavailableReason,
    ) -> Self {
        Self {
            observed_at_unix_secs,
            ttl_ms: 30_000,
            stale: false,
            pressure: PressureLevel::Cautious,
            available_parallelism: None,
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
            unavailable_reason: Some(reason),
            unavailable_detail: None,
        }
    }

    pub fn with_unavailable_detail(mut self, detail: impl Into<String>) -> Self {
        self.unavailable_detail = Some(detail.into());
        self
    }

    fn mark_stale(mut self) -> Self {
        self.stale = true;
        if self.pressure == PressureLevel::Normal {
            self.pressure = PressureLevel::Cautious;
        }
        self.unavailable_reason = Some(RuntimeResourceUnavailableReason::SnapshotStale);
        self
    }

    pub fn is_expired(&self, now_secs: u64) -> bool {
        if now_secs < self.observed_at_unix_secs {
            return true;
        }
        let ttl_secs = self.ttl_ms.div_ceil(1000);
        now_secs - self.observed_at_unix_secs >= ttl_secs
    }

    fn host_probe(now_secs: u64, storage_path: Option<&Path>) -> Self {
        let (memory_total_bytes, memory_available_bytes) = host_memory_bytes();
        let (storage_total_bytes, storage_available_bytes) =
            storage_path.map(host_storage_bytes).unwrap_or((None, None));
        let available_parallelism = std::thread::available_parallelism()
            .ok()
            .and_then(|value| u16::try_from(value.get()).ok());
        let pressure = pressure_from_resources(memory_total_bytes, memory_available_bytes);
        let mut observation = Self {
            observed_at_unix_secs: now_secs,
            ttl_ms: 30_000,
            stale: false,
            pressure,
            available_parallelism,
            memory_total_bytes,
            memory_available_bytes,
            internal_heap_free_bytes: None,
            internal_heap_minimum_free_bytes: None,
            internal_heap_largest_block_bytes: None,
            psram_total_bytes: None,
            psram_free_bytes: None,
            psram_largest_block_bytes: None,
            storage_total_bytes,
            storage_available_bytes,
            unavailable_reason: None,
            unavailable_detail: None,
        };
        if memory_total_bytes.is_none() && memory_available_bytes.is_none() {
            observation.unavailable_reason =
                Some(RuntimeResourceUnavailableReason::MemoryUnavailable);
        }
        if storage_path.is_some()
            && storage_total_bytes.is_none()
            && storage_available_bytes.is_none()
        {
            observation.unavailable_reason = observation
                .unavailable_reason
                .or(Some(RuntimeResourceUnavailableReason::StorageUnavailable));
        }
        observation
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeResourceSnapshot {
    pub source: RuntimeResourceProbeSource,
    #[serde(flatten)]
    observation: RuntimeResourceObservation,
}

impl RuntimeResourceSnapshot {
    pub fn unavailable(
        observed_at_unix_secs: u64,
        source: RuntimeResourceProbeSource,
        reason: RuntimeResourceUnavailableReason,
    ) -> Self {
        Self::from_observation(
            source,
            RuntimeResourceObservation::unavailable(observed_at_unix_secs, reason),
        )
    }

    pub(crate) fn from_observation(
        source: RuntimeResourceProbeSource,
        observation: RuntimeResourceObservation,
    ) -> Self {
        Self {
            source,
            observation,
        }
    }

    pub fn with_unavailable_detail(mut self, detail: impl Into<String>) -> Self {
        self.observation.unavailable_detail = Some(detail.into());
        self
    }

    pub fn mark_stale(mut self) -> Self {
        self.observation = self.observation.mark_stale();
        self
    }

    pub fn is_expired(&self, now_secs: u64) -> bool {
        self.observation.is_expired(now_secs)
    }
}

impl Deref for RuntimeResourceSnapshot {
    type Target = RuntimeResourceObservation;

    fn deref(&self) -> &Self::Target {
        &self.observation
    }
}

impl DerefMut for RuntimeResourceSnapshot {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.observation
    }
}

pub trait RuntimeResourceProbe: Send + Sync {
    fn probe(&self, now_secs: u64) -> Result<RuntimeResourceObservation>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostStorageObservation {
    VolatileMemory,
    PersistentFilesystem,
}

// runtime-resource-public-surface: core-owned-host-probe
#[derive(Clone)]
pub(crate) struct RuntimeResourceProbeRegistration {
    attested_source: RuntimeResourceProbeSource,
    host_storage_observation: Option<HostStorageObservation>,
    probe: Arc<dyn RuntimeResourceProbe>,
}

impl RuntimeResourceProbeRegistration {
    pub(crate) fn host(probe: HostRuntimeResourceProbe) -> Self {
        let host_storage_observation = probe.storage_observation();
        Self {
            attested_source: compiled_host_probe_source(),
            host_storage_observation: Some(host_storage_observation),
            probe: Arc::new(probe),
        }
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub(crate) fn nonproduction_host(
        probe: Arc<dyn RuntimeResourceProbe>,
        host_storage_observation: HostStorageObservation,
    ) -> Self {
        Self {
            attested_source: compiled_host_probe_source(),
            host_storage_observation: Some(host_storage_observation),
            probe,
        }
    }

    pub(crate) fn firmware(probe: Arc<dyn RuntimeResourceProbe>) -> Self {
        Self {
            attested_source: RuntimeResourceProbeSource::FirmwareManifest,
            host_storage_observation: None,
            probe,
        }
    }

    pub(crate) fn firmware_unavailable() -> Self {
        Self::firmware(Arc::new(UnavailableRuntimeResourceProbe::new(
            RuntimeResourceUnavailableReason::ProbeNotConfigured,
        )))
    }

    pub(crate) const fn attested_source(&self) -> RuntimeResourceProbeSource {
        self.attested_source
    }

    pub(crate) const fn host_storage_observation(&self) -> Option<HostStorageObservation> {
        self.host_storage_observation
    }

    pub(crate) fn probe_snapshot(&self, now_secs: u64) -> Result<RuntimeResourceSnapshot> {
        self.probe.probe(now_secs).map(|observation| {
            RuntimeResourceSnapshot::from_observation(self.attested_source, observation)
        })
    }
}

impl fmt::Debug for RuntimeResourceProbeRegistration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeResourceProbeRegistration")
            .field("attested_source", &self.attested_source)
            .field("host_storage_observation", &self.host_storage_observation)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct UnavailableRuntimeResourceProbe {
    reason: RuntimeResourceUnavailableReason,
}

impl UnavailableRuntimeResourceProbe {
    pub const fn new(reason: RuntimeResourceUnavailableReason) -> Self {
        Self { reason }
    }
}

impl Default for UnavailableRuntimeResourceProbe {
    fn default() -> Self {
        Self::new(RuntimeResourceUnavailableReason::ProbeNotConfigured)
    }
}

impl RuntimeResourceProbe for UnavailableRuntimeResourceProbe {
    fn probe(&self, now_secs: u64) -> Result<RuntimeResourceObservation> {
        Ok(RuntimeResourceObservation::unavailable(
            now_secs,
            self.reason,
        ))
    }
}

#[derive(Clone, Debug)]
pub struct HostRuntimeResourceProbe {
    storage_path: Option<PathBuf>,
}

impl HostRuntimeResourceProbe {
    pub const fn for_volatile_memory() -> Self {
        Self { storage_path: None }
    }

    pub fn for_persistent_filesystem(data_path: impl Into<PathBuf>) -> Result<Self> {
        let data_path = data_path.into();
        if data_path.as_os_str().is_empty() {
            return Err(Error::config(
                "runtime_resource_probe_config",
                "persistent_filesystem_path_is_empty",
            ));
        }
        let storage_path = nearest_existing_ancestor(&data_path)?;
        Ok(Self {
            storage_path: Some(storage_path),
        })
    }

    const fn storage_observation(&self) -> HostStorageObservation {
        if self.storage_path.is_some() {
            HostStorageObservation::PersistentFilesystem
        } else {
            HostStorageObservation::VolatileMemory
        }
    }
}

fn nearest_existing_ancestor(data_path: &Path) -> Result<PathBuf> {
    let mut candidate = if data_path.is_absolute() {
        data_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                Error::config(
                    "runtime_resource_probe_config",
                    format!("persistent_filesystem_current_dir_unavailable:{error}"),
                )
            })?
            .join(data_path)
    };

    loop {
        match candidate.try_exists() {
            Ok(true) => return Ok(candidate),
            Ok(false) => {}
            Err(error) => {
                return Err(Error::config(
                    "runtime_resource_probe_config",
                    format!(
                        "persistent_filesystem_path_observation_failed:{}:{error}",
                        candidate.display()
                    ),
                ));
            }
        }
        if !candidate.pop() {
            return Err(Error::config(
                "runtime_resource_probe_config",
                "persistent_filesystem_has_no_existing_ancestor",
            ));
        }
    }
}

impl Default for HostRuntimeResourceProbe {
    fn default() -> Self {
        Self::for_volatile_memory()
    }
}

impl RuntimeResourceProbe for HostRuntimeResourceProbe {
    fn probe(&self, now_secs: u64) -> Result<RuntimeResourceObservation> {
        Ok(RuntimeResourceObservation::host_probe(
            now_secs,
            self.storage_path.as_deref(),
        ))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeResourceSnapshotCache {
    snapshot: RuntimeResourceSnapshot,
    refresh_deadline: Instant,
}

impl RuntimeResourceSnapshotCache {
    pub(crate) fn new(snapshot: RuntimeResourceSnapshot) -> Self {
        let refresh_deadline = monotonic_refresh_deadline(snapshot.ttl_ms);
        Self {
            snapshot,
            refresh_deadline,
        }
    }

    pub(crate) fn requires_refresh(&self, now_secs: u64) -> bool {
        self.snapshot.stale
            || self.snapshot.is_expired(now_secs)
            || Instant::now() >= self.refresh_deadline
    }

    pub(crate) fn current(&self, now_secs: u64) -> RuntimeResourceSnapshot {
        if !self.snapshot.stale && self.requires_refresh(now_secs) {
            return self.snapshot.clone().mark_stale();
        }
        self.snapshot.clone()
    }

    pub(crate) fn replace(&mut self, snapshot: RuntimeResourceSnapshot) {
        self.refresh_deadline = monotonic_refresh_deadline(snapshot.ttl_ms);
        self.snapshot = snapshot;
    }
}

fn monotonic_refresh_deadline(ttl_ms: u64) -> Instant {
    Instant::now()
        .checked_add(Duration::from_millis(ttl_ms))
        .unwrap_or_else(Instant::now)
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

#[cfg(target_os = "macos")]
const fn compiled_host_probe_source() -> RuntimeResourceProbeSource {
    RuntimeResourceProbeSource::HostMacos
}

#[cfg(target_os = "linux")]
const fn compiled_host_probe_source() -> RuntimeResourceProbeSource {
    RuntimeResourceProbeSource::HostLinux
}

#[cfg(target_os = "windows")]
const fn compiled_host_probe_source() -> RuntimeResourceProbeSource {
    RuntimeResourceProbeSource::HostWindows
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
const fn compiled_host_probe_source() -> RuntimeResourceProbeSource {
    RuntimeResourceProbeSource::HostOther
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
    #[cfg(target_os = "windows")]
    {
        return windows_memory_bytes();
    }
    #[allow(unreachable_code)]
    (None, None)
}

#[cfg(target_os = "linux")]
fn linux_memory_bytes() -> (Option<u64>, Option<u64>) {
    linux_memory_bytes_from_reader(&FilesystemLinuxResourceReader)
}

trait LinuxResourceReader {
    fn read_to_string(&self, path: &Path) -> Option<String>;
}

struct FilesystemLinuxResourceReader;

impl LinuxResourceReader for FilesystemLinuxResourceReader {
    fn read_to_string(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }
}

fn linux_memory_bytes_from_reader(reader: &dyn LinuxResourceReader) -> (Option<u64>, Option<u64>) {
    let Some(text) = reader.read_to_string(Path::new("/proc/meminfo")) else {
        return (None, None);
    };
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        if let Some(value) = parse_meminfo_kib(line, "MemTotal:") {
            total = value.checked_mul(1024);
        }
        if let Some(value) = parse_meminfo_kib(line, "MemAvailable:") {
            available = value.checked_mul(1024);
        }
    }
    let Some((limit, current)) = linux_cgroup_memory(reader) else {
        return (total, available);
    };
    let cgroup_available = limit.saturating_sub(current);
    (
        Some(total.map_or(limit, |host_total| host_total.min(limit))),
        Some(available.map_or(cgroup_available, |host_available| {
            host_available.min(cgroup_available)
        })),
    )
}

fn parse_meminfo_kib(line: &str, prefix: &str) -> Option<u64> {
    let rest = line.strip_prefix(prefix)?.trim();
    rest.split_whitespace().next()?.parse::<u64>().ok()
}

fn linux_cgroup_memory(reader: &dyn LinuxResourceReader) -> Option<(u64, u64)> {
    let cgroup = reader.read_to_string(Path::new("/proc/self/cgroup"))?;
    let mountinfo = reader.read_to_string(Path::new("/proc/self/mountinfo"));
    if let Some(relative) = cgroup.lines().find_map(|line| {
        let mut fields = line.splitn(3, ':');
        match (fields.next(), fields.next(), fields.next()) {
            (Some("0"), Some(""), Some(path)) => Some(path),
            _ => None,
        }
    }) {
        let mount = mountinfo
            .as_deref()
            .and_then(|text| cgroup_mountpoint(text, "cgroup2", None))
            .unwrap_or_else(|| PathBuf::from("/sys/fs/cgroup"));
        let directory = join_cgroup_path(&mount, relative);
        let limit = parse_cgroup_limit(&reader.read_to_string(&directory.join("memory.max"))?)?;
        let current =
            parse_cgroup_value(&reader.read_to_string(&directory.join("memory.current"))?)?;
        return Some((limit, current));
    }

    let relative = cgroup.lines().find_map(|line| {
        let mut fields = line.splitn(3, ':');
        let _hierarchy = fields.next()?;
        let controllers = fields.next()?;
        let path = fields.next()?;
        controllers
            .split(',')
            .any(|value| value == "memory")
            .then_some(path)
    })?;
    let mount = mountinfo
        .as_deref()
        .and_then(|text| cgroup_mountpoint(text, "cgroup", Some("memory")))
        .unwrap_or_else(|| PathBuf::from("/sys/fs/cgroup/memory"));
    let directory = join_cgroup_path(&mount, relative);
    let limit =
        parse_cgroup_limit(&reader.read_to_string(&directory.join("memory.limit_in_bytes"))?)?;
    let current =
        parse_cgroup_value(&reader.read_to_string(&directory.join("memory.usage_in_bytes"))?)?;
    Some((limit, current))
}

fn cgroup_mountpoint(text: &str, fs_type: &str, controller: Option<&str>) -> Option<PathBuf> {
    text.lines().find_map(|line| {
        let (before, after) = line.split_once(" - ")?;
        let mut after_fields = after.split_whitespace();
        if after_fields.next()? != fs_type {
            return None;
        }
        let _source = after_fields.next()?;
        let super_options = after_fields.next().unwrap_or_default();
        if controller.is_some_and(|value| !super_options.split(',').any(|item| item == value)) {
            return None;
        }
        before.split_whitespace().nth(4).map(PathBuf::from)
    })
}

fn join_cgroup_path(mount: &Path, relative: &str) -> PathBuf {
    mount.join(relative.trim_start_matches('/'))
}

fn parse_cgroup_limit(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if raw == "max" {
        return None;
    }
    let value = raw.parse::<u64>().ok()?;
    (value < (1_u64 << 60)).then_some(value)
}

fn parse_cgroup_value(raw: &str) -> Option<u64> {
    raw.trim().parse::<u64>().ok()
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn host_storage_bytes(path: &Path) -> (Option<u64>, Option<u64>) {
    df_storage_bytes(path)
}

#[cfg(target_os = "windows")]
fn host_storage_bytes(path: &Path) -> (Option<u64>, Option<u64>) {
    windows_storage_bytes(path)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn host_storage_bytes(path: &Path) -> (Option<u64>, Option<u64>) {
    let _ = path;
    (None, None)
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct WindowsMemoryStatusEx {
    length: u32,
    memory_load: u32,
    total_physical: u64,
    available_physical: u64,
    total_page_file: u64,
    available_page_file: u64,
    total_virtual: u64,
    available_virtual: u64,
    available_extended_virtual: u64,
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalMemoryStatusEx(status: *mut WindowsMemoryStatusEx) -> i32;
    fn GetDiskFreeSpaceExW(
        directory_name: *const u16,
        free_bytes_available: *mut u64,
        total_number_of_bytes: *mut u64,
        total_number_of_free_bytes: *mut u64,
    ) -> i32;
}

#[cfg(target_os = "windows")]
fn windows_memory_bytes() -> (Option<u64>, Option<u64>) {
    let mut status = WindowsMemoryStatusEx {
        length: std::mem::size_of::<WindowsMemoryStatusEx>() as u32,
        memory_load: 0,
        total_physical: 0,
        available_physical: 0,
        total_page_file: 0,
        available_page_file: 0,
        total_virtual: 0,
        available_virtual: 0,
        available_extended_virtual: 0,
    };
    // SAFETY: `status` is writable, correctly sized, and initialized as required by Win32.
    let succeeded = unsafe { GlobalMemoryStatusEx(&mut status) } != 0;
    if succeeded {
        (Some(status.total_physical), Some(status.available_physical))
    } else {
        (None, None)
    }
}

#[cfg(target_os = "windows")]
fn windows_storage_bytes(path: &Path) -> (Option<u64>, Option<u64>) {
    use std::os::windows::ffi::OsStrExt;

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut available = 0_u64;
    let mut total = 0_u64;
    let mut total_free = 0_u64;
    // SAFETY: `wide` is NUL-terminated and all output pointers remain valid for the call.
    let succeeded =
        unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut total_free) }
            != 0;
    if succeeded {
        (Some(total), Some(available))
    } else {
        (None, None)
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn df_storage_bytes(path: &Path) -> (Option<u64>, Option<u64>) {
    let output = std::process::Command::new("/bin/df")
        .arg("-k")
        .arg(path)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .or_else(|| {
            std::process::Command::new("df")
                .arg("-k")
                .arg(path)
                .output()
                .ok()
                .filter(|output| output.status.success())
        });
    let Some(output) = output else {
        return (None, None);
    };
    let Ok(text) = String::from_utf8(output.stdout) else {
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
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FixtureLinuxResourceReader {
        files: BTreeMap<PathBuf, String>,
    }

    impl FixtureLinuxResourceReader {
        fn with(mut self, path: &str, value: &str) -> Self {
            self.files.insert(PathBuf::from(path), value.to_string());
            self
        }
    }

    impl LinuxResourceReader for FixtureLinuxResourceReader {
        fn read_to_string(&self, path: &Path) -> Option<String> {
            self.files.get(path).cloned()
        }
    }

    #[test]
    fn unavailable_probe_never_reports_full_budget() {
        let probe = UnavailableRuntimeResourceProbe::default();
        let snapshot = probe.probe(10).unwrap();
        assert_eq!(
            snapshot.unavailable_reason,
            Some(RuntimeResourceUnavailableReason::ProbeNotConfigured)
        );
        assert_eq!(snapshot.memory_available_bytes, None);
        assert_eq!(snapshot.available_parallelism, None);
        assert_eq!(snapshot.pressure, PressureLevel::Cautious);
    }

    #[test]
    fn stale_cache_marks_snapshot_without_reprobing() {
        let mut snapshot = RuntimeResourceSnapshot::unavailable(
            1,
            RuntimeResourceProbeSource::Unavailable,
            RuntimeResourceUnavailableReason::ProbeNotConfigured,
        );
        snapshot.pressure = PressureLevel::Normal;
        let cache = RuntimeResourceSnapshotCache::new(snapshot);
        let stale = cache.current(60);
        assert!(stale.stale);
        assert_eq!(stale.observed_at_unix_secs, 1);
        assert_eq!(stale.pressure, PressureLevel::Cautious);
        assert_eq!(
            stale.unavailable_reason,
            Some(RuntimeResourceUnavailableReason::SnapshotStale)
        );
    }

    #[test]
    fn ttl_expires_at_the_exact_boundary() {
        let mut snapshot = RuntimeResourceSnapshot::unavailable(
            10,
            RuntimeResourceProbeSource::Unavailable,
            RuntimeResourceUnavailableReason::ProbeNotConfigured,
        );
        snapshot.ttl_ms = 1_000;
        assert!(!snapshot.is_expired(10));
        assert!(snapshot.is_expired(11));
    }

    #[test]
    fn wall_clock_rollback_expires_the_observation() {
        let mut snapshot = RuntimeResourceSnapshot::unavailable(
            100,
            RuntimeResourceProbeSource::HostLinux,
            RuntimeResourceUnavailableReason::MemoryUnavailable,
        );
        snapshot.ttl_ms = 30_000;

        assert!(snapshot.is_expired(99));
        assert!(!snapshot.is_expired(100));
    }

    #[test]
    fn persistent_probe_observes_existing_ancestor_without_creating_data_path() {
        let unique = format!(
            "bm-resource-probe-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let missing_root = std::env::temp_dir().join(unique);
        let data_path = missing_root.join("nested").join("store.sqlite");
        assert!(!missing_root.exists());

        let probe = HostRuntimeResourceProbe::for_persistent_filesystem(&data_path).unwrap();

        assert_eq!(
            probe.storage_path.as_deref(),
            Some(std::env::temp_dir().as_path())
        );
        assert!(!missing_root.exists());
    }

    #[test]
    fn linux_cgroup_v2_caps_host_memory_by_limit_minus_current() {
        let reader = FixtureLinuxResourceReader::default()
            .with(
                "/proc/meminfo",
                "MemTotal:       16777216 kB\nMemAvailable:   12582912 kB\n",
            )
            .with("/proc/self/cgroup", "0::/workload.slice/test\n")
            .with(
                "/proc/self/mountinfo",
                "29 23 0:26 / /sys/fs/cgroup rw - cgroup2 cgroup rw\n",
            )
            .with(
                "/sys/fs/cgroup/workload.slice/test/memory.max",
                "1073741824\n",
            )
            .with(
                "/sys/fs/cgroup/workload.slice/test/memory.current",
                "268435456\n",
            );

        assert_eq!(
            linux_memory_bytes_from_reader(&reader),
            (Some(1_073_741_824), Some(805_306_368))
        );
    }

    #[test]
    fn linux_cgroup_v1_caps_host_memory_and_treats_kernel_sentinel_as_unlimited() {
        let base = FixtureLinuxResourceReader::default()
            .with(
                "/proc/meminfo",
                "MemTotal:       8388608 kB\nMemAvailable:   4194304 kB\n",
            )
            .with("/proc/self/cgroup", "5:memory:/docker/test\n")
            .with(
                "/proc/self/mountinfo",
                "31 23 0:28 / /sys/fs/cgroup/memory rw - cgroup cgroup rw,memory\n",
            )
            .with(
                "/sys/fs/cgroup/memory/docker/test/memory.limit_in_bytes",
                "2147483648\n",
            )
            .with(
                "/sys/fs/cgroup/memory/docker/test/memory.usage_in_bytes",
                "536870912\n",
            );
        assert_eq!(
            linux_memory_bytes_from_reader(&base),
            (Some(2_147_483_648), Some(1_610_612_736))
        );

        let unlimited = base.with(
            "/sys/fs/cgroup/memory/docker/test/memory.limit_in_bytes",
            "9223372036854771712\n",
        );
        assert_eq!(
            linux_memory_bytes_from_reader(&unlimited),
            (Some(8_589_934_592), Some(4_294_967_296))
        );
    }

    #[test]
    fn malformed_cgroup_facts_fail_back_to_host_memory_without_inventing_capacity() {
        let reader = FixtureLinuxResourceReader::default()
            .with(
                "/proc/meminfo",
                "MemTotal:       1048576 kB\nMemAvailable:   524288 kB\n",
            )
            .with("/proc/self/cgroup", "0::/broken\n")
            .with("/sys/fs/cgroup/broken/memory.max", "invalid\n")
            .with("/sys/fs/cgroup/broken/memory.current", "1\n");

        assert_eq!(
            linux_memory_bytes_from_reader(&reader),
            (Some(1_073_741_824), Some(536_870_912))
        );
    }
}
