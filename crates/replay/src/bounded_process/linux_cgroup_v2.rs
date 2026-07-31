//! Generation-neutral parsers for retained Linux cgroup v2 observations.
//!
//! These parsers accept kernel text formats only. They do not assign P8 meaning, trust a path, or
//! convert direct-child RSS into a process-domain resource claim.

use std::collections::{BTreeMap, BTreeSet};
use std::io;

#[cfg(target_os = "linux")]
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::{Read, Seek, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MemoryEventCounters {
    pub(crate) oom: u64,
    pub(crate) oom_kill: u64,
    pub(crate) oom_group_kill: u64,
}

impl MemoryEventCounters {
    pub(crate) fn parse(bytes: &[u8]) -> io::Result<Self> {
        let fields = parse_keyed_u64(bytes, "memory.events")?;
        Ok(Self {
            oom: required_counter(&fields, "oom", "memory.events")?,
            oom_kill: required_counter(&fields, "oom_kill", "memory.events")?,
            oom_group_kill: required_counter(&fields, "oom_group_kill", "memory.events")?,
        })
    }

    pub(crate) fn checked_delta(&self, before: &Self) -> io::Result<Self> {
        Ok(Self {
            oom: self
                .oom
                .checked_sub(before.oom)
                .ok_or_else(|| invalid_data("memory.events oom counter regressed"))?,
            oom_kill: self
                .oom_kill
                .checked_sub(before.oom_kill)
                .ok_or_else(|| invalid_data("memory.events oom_kill counter regressed"))?,
            oom_group_kill: self
                .oom_group_kill
                .checked_sub(before.oom_group_kill)
                .ok_or_else(|| invalid_data("memory.events oom_group_kill counter regressed"))?,
        })
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.oom == 0 && self.oom_kill == 0 && self.oom_group_kill == 0
    }
}

pub(crate) fn parse_memory_peak(bytes: &[u8]) -> io::Result<u64> {
    parse_single_u64(bytes, "memory.peak")
}

pub(crate) fn parse_cgroup_procs(bytes: &[u8]) -> io::Result<BTreeSet<u32>> {
    let text = strict_ascii(bytes, "cgroup.procs")?;
    let mut pids = BTreeSet::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if line.bytes().any(|byte| !byte.is_ascii_digit()) {
            return Err(invalid_data("cgroup.procs contains a non-numeric PID"));
        }
        let pid = line
            .parse::<u32>()
            .map_err(|_| invalid_data("cgroup.procs PID is invalid"))?;
        if pid == 0 || !pids.insert(pid) {
            return Err(invalid_data("cgroup.procs PID is zero or duplicated"));
        }
    }
    Ok(pids)
}

pub(crate) fn parse_unified_proc_membership(bytes: &[u8]) -> io::Result<String> {
    let text = strict_ascii(bytes, "/proc/<pid>/cgroup")?;
    let mut membership = None;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let mut fields = line.splitn(3, ':');
        let hierarchy = fields.next();
        let controllers = fields.next();
        let path = fields.next();
        if hierarchy != Some("0") || controllers != Some("") {
            return Err(invalid_data(
                "/proc/<pid>/cgroup is not a unified cgroup v2 membership",
            ));
        }
        let path = path.ok_or_else(|| invalid_data("cgroup membership path is missing"))?;
        validate_cgroup_path(path)?;
        if membership.replace(path.to_string()).is_some() {
            return Err(invalid_data("duplicate unified cgroup membership"));
        }
    }
    membership.ok_or_else(|| invalid_data("unified cgroup membership is missing"))
}

pub(crate) fn parse_cgroup_populated(bytes: &[u8]) -> io::Result<bool> {
    let fields = parse_keyed_u64(bytes, "cgroup.events")?;
    match required_counter(&fields, "populated", "cgroup.events")? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid_data("cgroup.events populated is not 0 or 1")),
    }
}

fn parse_single_u64(bytes: &[u8], label: &'static str) -> io::Result<u64> {
    let text = strict_ascii(bytes, label)?;
    let value = text.strip_suffix('\n').unwrap_or(text);
    if value.is_empty() || value.contains('\n') || value.bytes().any(|byte| !byte.is_ascii_digit())
    {
        return Err(invalid_data("cgroup scalar is not one canonical integer"));
    }
    value
        .parse()
        .map_err(|_| invalid_data("cgroup scalar overflows u64"))
}

fn parse_keyed_u64(bytes: &[u8], label: &'static str) -> io::Result<BTreeMap<String, u64>> {
    let text = strict_ascii(bytes, label)?;
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_ascii_whitespace();
        let key = parts
            .next()
            .ok_or_else(|| invalid_data("cgroup counter key is missing"))?;
        let value = parts
            .next()
            .ok_or_else(|| invalid_data("cgroup counter value is missing"))?;
        if parts.next().is_some()
            || key
                .bytes()
                .any(|byte| !(byte.is_ascii_lowercase() || byte == b'_'))
            || value.bytes().any(|byte| !byte.is_ascii_digit())
        {
            return Err(invalid_data("cgroup counter line is malformed"));
        }
        let value = value
            .parse::<u64>()
            .map_err(|_| invalid_data("cgroup counter overflows u64"))?;
        if fields.insert(key.to_string(), value).is_some() {
            return Err(invalid_data("duplicate cgroup counter key"));
        }
    }
    if fields.is_empty() {
        return Err(invalid_data("cgroup counter file is empty"));
    }
    Ok(fields)
}

fn required_counter(
    fields: &BTreeMap<String, u64>,
    key: &str,
    _label: &'static str,
) -> io::Result<u64> {
    fields
        .get(key)
        .copied()
        .ok_or_else(|| invalid_data("required cgroup counter is missing"))
}

fn strict_ascii<'a>(bytes: &'a [u8], _label: &'static str) -> io::Result<&'a str> {
    if bytes.contains(&0) || bytes.iter().any(|byte| !byte.is_ascii()) {
        return Err(invalid_data("cgroup observation is not strict ASCII"));
    }
    std::str::from_utf8(bytes).map_err(|_| invalid_data("cgroup observation is not UTF-8"))
}

fn validate_cgroup_path(path: &str) -> io::Result<()> {
    if !path.starts_with('/')
        || path.contains("//")
        || path
            .split('/')
            .any(|component| matches!(component, "." | ".."))
        || path
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(invalid_data("unified cgroup membership path is invalid"));
    }
    Ok(())
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(target_os = "linux")]
pub(crate) struct LinuxCgroupV2RunRoot {
    path: PathBuf,
    membership_path: String,
    mount_id: u64,
    device: u64,
    inode: u64,
    directory: File,
    cgroup_procs: File,
    cgroup_events: File,
    cgroup_kill: File,
    memory_peak: File,
    memory_events: File,
    memory_events_local: File,
}

#[cfg(target_os = "linux")]
pub(crate) struct LinuxCgroupV2InitialObservation {
    pub(crate) membership_path: String,
    pub(crate) mount_id: u64,
    pub(crate) device: u64,
    pub(crate) inode: u64,
    pub(crate) cgroup_procs: Vec<u8>,
    pub(crate) cgroup_events: Vec<u8>,
    pub(crate) memory_events: Vec<u8>,
    pub(crate) memory_events_local: Vec<u8>,
}

#[cfg(target_os = "linux")]
pub(crate) struct LinuxCgroupV2BarrierObservation {
    pub(crate) child_pid: u32,
    pub(crate) cgroup_procs: Vec<u8>,
    pub(crate) child_proc_cgroup: Vec<u8>,
}

#[cfg(target_os = "linux")]
impl LinuxCgroupV2RunRoot {
    pub(crate) fn open_existing_fresh(
        path: &Path,
    ) -> io::Result<(Self, LinuxCgroupV2InitialObservation)> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cgroup v2 run-root path must be absolute",
            ));
        }
        let canonical = std::fs::canonicalize(path)?;
        if canonical != path {
            return Err(invalid_data(
                "cgroup v2 run-root path must already be canonical",
            ));
        }
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&canonical)?;
        require_cgroup2_filesystem(&directory)?;
        let metadata = directory.metadata()?;
        let (mount_id, membership_path) = resolve_cgroup2_membership_path(&canonical)?;
        let cgroup_procs = open_interface_at(&directory, "cgroup.procs", true)?;
        let cgroup_events = open_interface_at(&directory, "cgroup.events", false)?;
        let cgroup_kill = open_interface_at(&directory, "cgroup.kill", true)?;
        let memory_peak = open_interface_at(&directory, "memory.peak", false)?;
        let memory_events = open_interface_at(&directory, "memory.events", false)?;
        let memory_events_local = open_interface_at(&directory, "memory.events.local", false)?;
        let mut root = Self {
            path: canonical.clone(),
            membership_path: membership_path.clone(),
            mount_id,
            device: metadata.dev(),
            inode: metadata.ino(),
            directory,
            cgroup_procs,
            cgroup_events,
            cgroup_kill,
            memory_peak,
            memory_events,
            memory_events_local,
        };
        let cgroup_procs = read_interface(&mut root.cgroup_procs, 1024 * 1024)?;
        let cgroup_events = read_interface(&mut root.cgroup_events, 64 * 1024)?;
        let memory_events = read_interface(&mut root.memory_events, 64 * 1024)?;
        let memory_events_local = read_interface(&mut root.memory_events_local, 64 * 1024)?;
        if !parse_cgroup_procs(&cgroup_procs)?.is_empty() || parse_cgroup_populated(&cgroup_events)?
        {
            return Err(invalid_data(
                "cgroup v2 run-root must be fresh and initially empty",
            ));
        }
        MemoryEventCounters::parse(&memory_events)?;
        MemoryEventCounters::parse(&memory_events_local)?;
        let observation = LinuxCgroupV2InitialObservation {
            membership_path,
            mount_id,
            device: root.device,
            inode: root.inode,
            cgroup_procs,
            cgroup_events,
            memory_events,
            memory_events_local,
        };
        Ok((root, observation))
    }

    pub(crate) fn attach_blocked_child(
        &mut self,
        child_pid: u32,
    ) -> io::Result<LinuxCgroupV2BarrierObservation> {
        if child_pid == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "blocked child PID must be non-zero",
            ));
        }
        self.verify_directory_identity()?;
        self.cgroup_procs.rewind()?;
        self.cgroup_procs
            .write_all(child_pid.to_string().as_bytes())?;
        let cgroup_procs = read_interface(&mut self.cgroup_procs, 1024 * 1024)?;
        if parse_cgroup_procs(&cgroup_procs)? != BTreeSet::from([child_pid]) {
            return Err(invalid_data(
                "cgroup v2 barrier membership is not the exact blocked child",
            ));
        }
        let child_proc_cgroup = std::fs::read(format!("/proc/{child_pid}/cgroup"))?;
        if parse_unified_proc_membership(&child_proc_cgroup)? != self.membership_path {
            return Err(invalid_data(
                "blocked child /proc membership differs from cgroup run-root",
            ));
        }
        self.verify_directory_identity()?;
        Ok(LinuxCgroupV2BarrierObservation {
            child_pid,
            cgroup_procs,
            child_proc_cgroup,
        })
    }

    pub(crate) fn read_memory_peak(&mut self) -> io::Result<Vec<u8>> {
        let bytes = read_interface(&mut self.memory_peak, 64 * 1024)?;
        parse_memory_peak(&bytes)?;
        Ok(bytes)
    }

    pub(crate) fn kill_all(&mut self) -> io::Result<()> {
        self.verify_directory_identity()?;
        self.cgroup_kill.write_all(b"1")
    }

    pub(crate) fn read_cgroup_procs(&mut self) -> io::Result<Vec<u8>> {
        let bytes = read_interface(&mut self.cgroup_procs, 1024 * 1024)?;
        parse_cgroup_procs(&bytes)?;
        Ok(bytes)
    }

    pub(crate) fn read_cgroup_events(&mut self) -> io::Result<Vec<u8>> {
        let bytes = read_interface(&mut self.cgroup_events, 64 * 1024)?;
        parse_cgroup_populated(&bytes)?;
        Ok(bytes)
    }

    pub(crate) fn read_memory_events(&mut self) -> io::Result<Vec<u8>> {
        let bytes = read_interface(&mut self.memory_events, 64 * 1024)?;
        MemoryEventCounters::parse(&bytes)?;
        Ok(bytes)
    }

    pub(crate) fn read_memory_events_local(&mut self) -> io::Result<Vec<u8>> {
        let bytes = read_interface(&mut self.memory_events_local, 64 * 1024)?;
        MemoryEventCounters::parse(&bytes)?;
        Ok(bytes)
    }

    fn verify_directory_identity(&self) -> io::Result<()> {
        require_cgroup2_filesystem(&self.directory)?;
        let retained_metadata = self.directory.metadata()?;
        if retained_metadata.dev() != self.device || retained_metadata.ino() != self.inode {
            return Err(invalid_data(
                "retained cgroup v2 run-root physical identity changed",
            ));
        }
        let current = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&self.path)?;
        require_cgroup2_filesystem(&current)?;
        let metadata = current.metadata()?;
        if metadata.dev() != self.device || metadata.ino() != self.inode {
            return Err(invalid_data("cgroup v2 run-root physical identity changed"));
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn open_interface_at(directory: &File, name: &str, writable: bool) -> io::Result<File> {
    if name.is_empty() || name.as_bytes().contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cgroup interface name is not terminal",
        ));
    }
    let name = CString::new(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "cgroup interface name contains NUL",
        )
    })?;
    let access = if writable {
        libc::O_RDWR
    } else {
        libc::O_RDONLY
    };
    // SAFETY: directory is a retained live directory descriptor, name is a terminal C string, and
    // the returned descriptor is immediately transferred to OwnedFd.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            access | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new owned descriptor and this branch transfers it exactly once.
    Ok(File::from(unsafe { OwnedFd::from_raw_fd(descriptor) }))
}

#[cfg(target_os = "linux")]
fn read_interface(file: &mut File, limit: u64) -> io::Result<Vec<u8>> {
    file.rewind()?;
    let mut bytes = Vec::new();
    Read::by_ref(file)
        .take(
            limit
                .checked_add(1)
                .ok_or_else(|| invalid_data("cgroup interface read limit overflow"))?,
        )
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(invalid_data("cgroup interface exceeds its read limit"));
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn require_cgroup2_filesystem(directory: &File) -> io::Result<()> {
    let mut stats = unsafe { std::mem::zeroed::<libc::statfs>() };
    // SAFETY: stats points to writable storage and the retained directory descriptor is live.
    if unsafe { libc::fstatfs(directory.as_raw_fd(), &mut stats) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if stats.f_type != libc::CGROUP2_SUPER_MAGIC {
        return Err(invalid_data("run-root is not on a cgroup v2 filesystem"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn resolve_cgroup2_membership_path(path: &Path) -> io::Result<(u64, String)> {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo")?;
    let mut selected: Option<(usize, u64, PathBuf, String)> = None;
    for line in mountinfo.lines() {
        let (left, right) = line
            .split_once(" - ")
            .ok_or_else(|| invalid_data("/proc/self/mountinfo line is malformed"))?;
        let mut right_fields = right.split_ascii_whitespace();
        if right_fields.next() != Some("cgroup2") {
            continue;
        }
        let fields = left.split_ascii_whitespace().collect::<Vec<_>>();
        if fields.len() < 6 {
            return Err(invalid_data("cgroup v2 mountinfo fields are incomplete"));
        }
        if fields[3].contains('\\') || fields[4].contains('\\') {
            return Err(invalid_data(
                "escaped cgroup v2 mount paths are unsupported",
            ));
        }
        let mount_id = fields[0]
            .parse::<u64>()
            .map_err(|_| invalid_data("cgroup v2 mount id is invalid"))?;
        let mount_root = fields[3].to_string();
        let mount_point = PathBuf::from(fields[4]);
        if !path.starts_with(&mount_point) {
            continue;
        }
        let specificity = mount_point.as_os_str().as_bytes().len();
        if selected
            .as_ref()
            .is_none_or(|(current, _, _, _)| specificity > *current)
        {
            selected = Some((specificity, mount_id, mount_point, mount_root));
        }
    }
    let (_, mount_id, mount_point, mount_root) =
        selected.ok_or_else(|| invalid_data("cgroup v2 mount for run-root is missing"))?;
    let relative = path
        .strip_prefix(&mount_point)
        .map_err(|_| invalid_data("cgroup v2 run-root is outside its mount"))?;
    let mut membership = PathBuf::from(mount_root);
    membership.push(relative);
    let membership = membership
        .to_str()
        .ok_or_else(|| invalid_data("cgroup v2 membership path is not UTF-8"))?
        .to_string();
    validate_cgroup_path(&membership)?;
    Ok((mount_id, membership))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_events_accept_unknown_keys_but_require_unique_oom_counters() {
        let before =
            MemoryEventCounters::parse(b"low 0\noom 1\noom_kill 2\noom_group_kill 3\nfuture 9\n")
                .expect("before");
        let after =
            MemoryEventCounters::parse(b"future 10\noom_group_kill 3\noom_kill 2\noom 1\nlow 0\n")
                .expect("after");
        assert!(after.checked_delta(&before).expect("delta").is_zero());
        assert!(
            MemoryEventCounters::parse(b"oom 0\noom 1\noom_kill 0\noom_group_kill 0\n").is_err()
        );
        assert!(MemoryEventCounters::parse(b"oom 0\noom_kill 0\n").is_err());
    }

    #[test]
    fn cgroup_scalar_membership_and_population_are_strict() {
        assert_eq!(parse_memory_peak(b"4096\n").expect("peak"), 4096);
        assert!(parse_memory_peak(b"4096\n1\n").is_err());
        assert_eq!(
            parse_cgroup_procs(b"41\n42\n").expect("pids"),
            BTreeSet::from([41, 42])
        );
        assert!(parse_cgroup_procs(b"41\n41\n").is_err());
        assert_eq!(
            parse_unified_proc_membership(b"0::/beetle/p8/run-arm\n").expect("membership"),
            "/beetle/p8/run-arm"
        );
        assert!(parse_unified_proc_membership(b"0::/beetle/../escape\n").is_err());
        assert!(!parse_cgroup_populated(b"populated 0\nfrozen 0\n").expect("empty"));
        assert!(parse_cgroup_populated(b"populated 2\n").is_err());
    }
}
