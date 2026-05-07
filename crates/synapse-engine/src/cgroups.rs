#[cfg(target_os = "linux")]
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[cfg(target_os = "linux")]
use crate::{temp_path, Providers, SandboxError};

#[cfg(target_os = "linux")]
const DEFAULT_CGROUP_V2_ROOT: &str = "/sys/fs/cgroup";
#[cfg(target_os = "linux")]
const CGROUP_V2_ROOT_ENV: &str = "SYNAPSE_CGROUP_V2_ROOT";
#[cfg(target_os = "linux")]
const DEFAULT_CPU_MAX: &str = "100000 100000";
#[cfg(target_os = "linux")]
const DEFAULT_PIDS_MAX: &str = "64";

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CgroupSupport {
    pub root: PathBuf,
    pub controllers: Vec<String>,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
pub struct ExecutionCgroup {
    path: PathBuf,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryEvents {
    pub oom: u64,
    pub oom_kill: u64,
}

#[cfg(target_os = "linux")]
impl ExecutionCgroup {
    pub fn try_create(
        providers: &dyn Providers,
        memory_limit_mb: u32,
    ) -> Result<Option<Self>, SandboxError> {
        let Some(support) = read_support(providers)? else {
            return Ok(None);
        };

        enable_controllers(&support.root, &support.controllers)?;

        let temp_path = temp_path(providers, "synapse-cgroup");
        let cgroup_name = temp_path
            .file_name()
            .ok_or_else(|| SandboxError::Execution("invalid cgroup path".to_string()))?;
        let path = support.root.join(cgroup_name);
        fs::create_dir(&path)?;

        if let Err(error) = configure_limits(&path, memory_limit_mb) {
            let _ = fs::remove_dir(&path);
            return Err(error.into());
        }

        Ok(Some(Self { path }))
    }

    pub fn attach(&self, pid: u32) -> Result<(), SandboxError> {
        write_file(self.path.join("cgroup.procs"), pid.to_string()).map_err(SandboxError::from)
    }

    pub fn cpu_usage_usec(&self) -> Result<u64, SandboxError> {
        read_cpu_usage_usec(&self.path).map_err(SandboxError::from)
    }

    pub fn memory_events(&self) -> Result<MemoryEvents, SandboxError> {
        read_memory_events(&self.path).map_err(SandboxError::from)
    }
}

#[cfg(target_os = "linux")]
impl Drop for ExecutionCgroup {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
    }
}

#[cfg(target_os = "linux")]
pub fn probe_support(providers: &dyn Providers) -> Result<CgroupSupport, SandboxError> {
    match read_support(providers)? {
        Some(support) => Ok(support),
        None => Err(SandboxError::Execution(format!(
            "cgroups v2 is not available at {}",
            cgroup_v2_root(providers).display()
        ))),
    }
}

#[cfg(target_os = "linux")]
fn read_support(providers: &dyn Providers) -> Result<Option<CgroupSupport>, SandboxError> {
    let root = cgroup_v2_root(providers);
    let controllers_path = root.join("cgroup.controllers");
    if !controllers_path.exists() {
        return Ok(None);
    }

    let controllers = fs::read_to_string(&controllers_path)?
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();

    for required in ["cpu", "memory", "pids"] {
        if !controllers.iter().any(|controller| controller == required) {
            return Err(SandboxError::Execution(format!(
                "cgroups v2 controller {required} is unavailable at {}",
                root.display()
            )));
        }
    }

    Ok(Some(CgroupSupport { root, controllers }))
}

#[cfg(target_os = "linux")]
fn configure_limits(path: &Path, memory_limit_mb: u32) -> io::Result<()> {
    let memory_limit_bytes = u64::from(memory_limit_mb)
        .checked_mul(1024 * 1024)
        .ok_or_else(|| io::Error::other("memory limit is too large"))?;

    write_file(path.join("memory.max"), memory_limit_bytes.to_string())?;
    if path.join("memory.swap.max").exists() {
        write_file(path.join("memory.swap.max"), "0")?;
    }
    write_file(path.join("cpu.max"), DEFAULT_CPU_MAX)?;
    write_file(path.join("pids.max"), DEFAULT_PIDS_MAX)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn enable_controllers(root: &Path, controllers: &[String]) -> io::Result<()> {
    let subtree_control = root.join("cgroup.subtree_control");
    if !subtree_control.exists() {
        return Err(io::Error::other(format!(
            "cgroups v2 subtree control is unavailable at {}",
            root.display()
        )));
    }

    let desired = controllers
        .iter()
        .filter(|controller| matches!(controller.as_str(), "cpu" | "memory" | "pids"))
        .map(|controller| format!("+{controller}"))
        .collect::<Vec<_>>()
        .join(" ");

    if desired.is_empty() {
        return Ok(());
    }

    write_file(subtree_control, desired)
}

#[cfg(target_os = "linux")]
fn read_cpu_usage_usec(path: &Path) -> io::Result<u64> {
    let cpu_stat = fs::read_to_string(path.join("cpu.stat"))?;
    parse_cpu_usage_usec(&cpu_stat)
}

#[cfg(target_os = "linux")]
fn read_memory_events(path: &Path) -> io::Result<MemoryEvents> {
    let memory_events = fs::read_to_string(path.join("memory.events"))?;
    parse_memory_events(&memory_events)
}

#[cfg(target_os = "linux")]
fn parse_cpu_usage_usec(cpu_stat: &str) -> io::Result<u64> {
    for line in cpu_stat.lines() {
        let mut parts = line.split_whitespace();
        if matches!(parts.next(), Some("usage_usec")) {
            let value = parts
                .next()
                .ok_or_else(|| io::Error::other("cpu.stat usage_usec is missing"))?;
            return value.parse::<u64>().map_err(|error| {
                io::Error::other(format!("invalid cpu.stat usage_usec: {error}"))
            });
        }
    }

    Err(io::Error::other("cpu.stat usage_usec is missing"))
}

#[cfg(target_os = "linux")]
fn parse_memory_events(memory_events: &str) -> io::Result<MemoryEvents> {
    let mut oom = None;
    let mut oom_kill = None;

    for line in memory_events.lines() {
        let mut parts = line.split_whitespace();
        let key = parts.next();
        let value = parts.next();

        match (key, value) {
            (Some("oom"), Some(value)) => {
                oom = Some(value.parse::<u64>().map_err(|error| {
                    io::Error::other(format!("invalid memory.events oom: {error}"))
                })?);
            }
            (Some("oom_kill"), Some(value)) => {
                oom_kill = Some(value.parse::<u64>().map_err(|error| {
                    io::Error::other(format!("invalid memory.events oom_kill: {error}"))
                })?);
            }
            _ => {}
        }
    }

    Ok(MemoryEvents {
        oom: oom.ok_or_else(|| io::Error::other("memory.events oom is missing"))?,
        oom_kill: oom_kill.ok_or_else(|| io::Error::other("memory.events oom_kill is missing"))?,
    })
}

#[cfg(target_os = "linux")]
fn cgroup_v2_root(providers: &dyn Providers) -> PathBuf {
    providers
        .env_var(CGROUP_V2_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CGROUP_V2_ROOT))
}

#[cfg(target_os = "linux")]
fn write_file(path: PathBuf, contents: impl AsRef<[u8]>) -> io::Result<()> {
    fs::write(path, contents)
}

#[cfg(target_os = "linux")]
#[cfg(test)]
mod tests {
    use super::{
        configure_limits, enable_controllers, parse_cpu_usage_usec, parse_memory_events,
        probe_support, CgroupSupport, ExecutionCgroup, MemoryEvents,
    };
    use crate::Providers;
    use std::{
        collections::HashMap,
        env,
        ffi::OsString,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[derive(Debug, Default)]
    struct FakeProviders {
        env: HashMap<String, String>,
    }

    impl Providers for FakeProviders {
        fn env_var(&self, key: &str) -> Option<String> {
            self.env.get(key).cloned()
        }

        fn env_var_os(&self, key: &str) -> Option<OsString> {
            self.env.get(key).map(OsString::from)
        }

        fn temp_dir(&self) -> PathBuf {
            env::temp_dir()
        }

        fn process_id(&self) -> u32 {
            1
        }

        fn now_unix_nanos(&self) -> u128 {
            1
        }
    }

    fn unique_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir().join(format!("{prefix}-{nanos}"))
    }

    fn make_v2_root() -> PathBuf {
        let root = unique_path("synapse-cgroup-test");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("cgroup.controllers"), b"cpu memory pids io").unwrap();
        fs::write(root.join("cgroup.subtree_control"), b"").unwrap();
        root
    }

    #[test]
    fn probe_support_reads_unified_root() {
        let root = make_v2_root();
        let mut providers = FakeProviders::default();
        providers.env.insert(
            "SYNAPSE_CGROUP_V2_ROOT".to_string(),
            root.to_string_lossy().into_owned(),
        );

        let support = probe_support(&providers).unwrap();

        assert_eq!(
            support,
            CgroupSupport {
                root: root.clone(),
                controllers: vec![
                    "cpu".to_string(),
                    "memory".to_string(),
                    "pids".to_string(),
                    "io".to_string(),
                ],
            }
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn enable_controllers_writes_required_entries() {
        let root = make_v2_root();

        enable_controllers(
            &root,
            &[
                "cpu".to_string(),
                "memory".to_string(),
                "pids".to_string(),
                "io".to_string(),
            ],
        )
        .unwrap();

        let contents = fs::read_to_string(root.join("cgroup.subtree_control")).unwrap();
        assert_eq!(contents, "+cpu +memory +pids");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn configure_limits_writes_memory_cpu_and_pids_limits() {
        let root = unique_path("synapse-cgroup-limits");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("memory.max"), b"").unwrap();
        fs::write(root.join("memory.swap.max"), b"").unwrap();
        fs::write(root.join("cpu.max"), b"").unwrap();
        fs::write(root.join("pids.max"), b"").unwrap();

        configure_limits(&root, 128).unwrap();

        assert_eq!(
            fs::read_to_string(root.join("memory.max")).unwrap(),
            "134217728"
        );
        assert_eq!(
            fs::read_to_string(root.join("memory.swap.max")).unwrap(),
            "0"
        );
        assert_eq!(
            fs::read_to_string(root.join("cpu.max")).unwrap(),
            "100000 100000"
        );
        assert_eq!(fs::read_to_string(root.join("pids.max")).unwrap(), "64");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parse_cpu_usage_usec_reads_usage_counter() {
        let usage =
            parse_cpu_usage_usec("usage_usec 4242\nuser_usec 4000\nsystem_usec 242\n").unwrap();
        assert_eq!(usage, 4242);
    }

    #[test]
    fn execution_cgroup_reads_cpu_usage_from_cpu_stat() {
        let root = unique_path("synapse-cgroup-cpu-stat");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("cpu.stat"), b"usage_usec 77\nuser_usec 55\n").unwrap();

        let cgroup = ExecutionCgroup { path: root.clone() };
        assert_eq!(cgroup.cpu_usage_usec().unwrap(), 77);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parse_memory_events_reads_oom_counters() {
        let events = parse_memory_events("low 0\nhigh 0\nmax 12\noom 2\noom_kill 1\n").unwrap();
        assert_eq!(
            events,
            MemoryEvents {
                oom: 2,
                oom_kill: 1,
            }
        );
    }

    #[test]
    fn execution_cgroup_reads_memory_events() {
        let root = unique_path("synapse-cgroup-memory-events");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("memory.events"),
            b"low 0\nhigh 0\nmax 3\noom 1\noom_kill 1\n",
        )
        .unwrap();

        let cgroup = ExecutionCgroup { path: root.clone() };
        assert_eq!(
            cgroup.memory_events().unwrap(),
            MemoryEvents {
                oom: 1,
                oom_kill: 1,
            }
        );

        let _ = fs::remove_dir_all(root);
    }
}
