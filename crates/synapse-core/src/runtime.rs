use std::{
    ffi::OsString,
    fs as stdfs, io,
    path::{Path, PathBuf},
    process::{Command as StdCommand, Stdio as StdStdio},
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
use std::os::fd::RawFd;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use tokio::{
    fs,
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time::timeout,
};

#[cfg(target_os = "linux")]
use crate::seccomp::{self, ExportedSeccompFilter};
use crate::{find_command, temp_path, ExecuteResponse, SynapseError, SystemProviders};

const OUTPUT_LIMIT_BYTES: usize = 1024 * 1024;
const MINIMAL_PATH: &str = "/usr/bin:/bin";
const SANDBOX_WORKDIR: &str = "/workspace";
const SANDBOX_SCRIPT_PATH: &str = "/workspace/main.py";

#[derive(Clone, Debug)]
pub struct PreparedSandbox {
    root: PathBuf,
}

#[derive(Clone, Debug)]
enum SandboxStrategy {
    #[cfg(not(target_os = "linux"))]
    Direct,
    #[cfg(target_os = "linux")]
    Bubblewrap { bwrap: PathBuf },
}

#[cfg(target_os = "linux")]
enum SeccompPlan {
    Bubblewrap(ExportedSeccompFilter),
}

#[cfg(target_os = "linux")]
impl SeccompPlan {
    fn bubblewrap_fd(&self) -> Option<RawFd> {
        match self {
            Self::Bubblewrap(filter) => Some(filter.fd()),
        }
    }
}

impl PreparedSandbox {
    pub fn path(&self) -> &Path {
        &self.root
    }

    pub async fn reset(&self) -> Result<(), SynapseError> {
        recreate_sandbox_dir(&self.root).await.map_err(Into::into)
    }

    pub fn reset_blocking(&self) -> Result<(), SynapseError> {
        recreate_sandbox_dir_blocking(&self.root).map_err(Into::into)
    }

    pub fn destroy_blocking(self) -> Result<(), SynapseError> {
        destroy_sandbox_dir_blocking(&self.root).map_err(Into::into)
    }
}

pub async fn prepare_sandbox() -> Result<PreparedSandbox, SynapseError> {
    let root = sandbox_dir();
    create_sandbox_dir(&root).await?;
    Ok(PreparedSandbox { root })
}

pub fn prepare_sandbox_blocking() -> Result<PreparedSandbox, SynapseError> {
    let root = sandbox_dir();
    create_sandbox_dir_blocking(&root)?;
    Ok(PreparedSandbox { root })
}

pub(crate) async fn execute_binary(
    binary: &Path,
    code: &str,
    sandbox_dir: &Path,
    timeout_ms: u64,
    memory_limit_mb: u32,
) -> Result<ExecuteResponse, SynapseError> {
    let script_path = sandbox_dir.join("main.py");
    write_script(&script_path, code).await?;
    run_process(
        binary,
        &script_path,
        sandbox_dir,
        timeout_ms,
        memory_limit_mb,
    )
    .await
}

async fn create_sandbox_dir(path: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(path).await?;

    #[cfg(unix)]
    {
        fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
    }

    Ok(())
}

fn create_sandbox_dir_blocking(path: &Path) -> Result<(), std::io::Error> {
    stdfs::create_dir_all(path)?;

    #[cfg(unix)]
    {
        stdfs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }

    Ok(())
}

async fn recreate_sandbox_dir(path: &Path) -> Result<(), std::io::Error> {
    if fs::try_exists(path).await? {
        fs::remove_dir_all(path).await?;
    }
    create_sandbox_dir(path).await
}

fn recreate_sandbox_dir_blocking(path: &Path) -> Result<(), std::io::Error> {
    if path.exists() {
        stdfs::remove_dir_all(path)?;
    }
    create_sandbox_dir_blocking(path)
}

fn destroy_sandbox_dir_blocking(path: &Path) -> Result<(), std::io::Error> {
    if path.exists() {
        stdfs::remove_dir_all(path)?;
    }
    Ok(())
}

async fn write_script(path: &Path, code: &str) -> Result<(), std::io::Error> {
    fs::write(path, code.as_bytes()).await?;

    #[cfg(unix)]
    {
        fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    }

    Ok(())
}

async fn run_process(
    binary: &Path,
    script_path: &Path,
    sandbox_dir: &Path,
    timeout_ms: u64,
    memory_limit_mb: u32,
) -> Result<ExecuteResponse, SynapseError> {
    let started = Instant::now();
    let strategy = sandbox_strategy()?;
    #[cfg(target_os = "linux")]
    let seccomp_plan = prepare_seccomp(&strategy, sandbox_dir)?;
    let mut command = build_command(
        &strategy,
        binary,
        script_path,
        sandbox_dir,
        #[cfg(target_os = "linux")]
        seccomp_plan.bubblewrap_fd(),
        #[cfg(not(target_os = "linux"))]
        None,
    );
    command
        .stdin(StdStdio::null())
        .stdout(StdStdio::piped())
        .stderr(StdStdio::piped())
        .env_clear()
        .env("PATH", MINIMAL_PATH)
        .env("LANG", "C.UTF-8")
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONUNBUFFERED", "1")
        .kill_on_drop(true);
    configure_command(
        &mut command,
        memory_limit_mb,
        #[cfg(target_os = "linux")]
        false,
        #[cfg(not(target_os = "linux"))]
        false,
    )?;

    let mut child = command.spawn()?;
    #[cfg(target_os = "linux")]
    drop(seccomp_plan);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| SynapseError::Execution("failed to capture stdout".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| SynapseError::Execution("failed to capture stderr".to_string()))?;

    let stdout_task = tokio::spawn(read_stream(stdout));
    let stderr_task = tokio::spawn(read_stream(stderr));

    let wait_result = timeout(Duration::from_millis(timeout_ms), child.wait()).await;
    let duration_ms = elapsed_ms(started);

    match wait_result {
        Ok(status) => {
            let status = status?;
            let stdout = collect_output(stdout_task).await?;
            let stderr = collect_output(stderr_task).await?;

            Ok(ExecuteResponse {
                stdout,
                stderr,
                exit_code: status.code().unwrap_or(-1),
                duration_ms,
            })
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;

            let stdout = collect_output(stdout_task).await?;
            let mut stderr = collect_output(stderr_task).await?;
            if !stderr.is_empty() {
                stderr.push('\n');
            }
            stderr.push_str("execution timed out");

            Ok(ExecuteResponse {
                stdout,
                stderr,
                exit_code: -1,
                duration_ms,
            })
        }
    }
}

#[cfg(target_os = "linux")]
fn prepare_seccomp(
    strategy: &SandboxStrategy,
    sandbox_dir: &Path,
) -> Result<SeccompPlan, SynapseError> {
    match strategy {
        SandboxStrategy::Bubblewrap { .. } => {
            let filter_path = sandbox_dir.join("seccomp.bpf");
            let filter = seccomp::export_blacklist_bpf(&filter_path).map_err(|error| {
                SynapseError::Execution(format!("failed to export seccomp profile: {error}"))
            })?;
            Ok(SeccompPlan::Bubblewrap(filter))
        }
    }
}

fn build_command(
    strategy: &SandboxStrategy,
    binary: &Path,
    _script_path: &Path,
    sandbox_dir: &Path,
    #[cfg(target_os = "linux")] seccomp_fd: Option<RawFd>,
    #[cfg(not(target_os = "linux"))] _seccomp_fd: Option<()>,
) -> Command {
    match strategy {
        #[cfg(not(target_os = "linux"))]
        SandboxStrategy::Direct => {
            let mut command = Command::new(binary);
            command.arg(_script_path).current_dir(sandbox_dir);
            command
        }
        #[cfg(target_os = "linux")]
        SandboxStrategy::Bubblewrap { bwrap } => {
            let mut command = Command::new(bwrap);
            command
                .args(bubblewrap_args(binary, sandbox_dir, seccomp_fd))
                .current_dir(sandbox_dir);
            command
        }
    }
}

fn sandbox_strategy() -> Result<SandboxStrategy, SynapseError> {
    #[cfg(target_os = "linux")]
    {
        detect_linux_sandbox_strategy()
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(SandboxStrategy::Direct)
    }
}

#[cfg(target_os = "linux")]
fn detect_linux_sandbox_strategy() -> Result<SandboxStrategy, SynapseError> {
    let probe_binary = resolve_binary("true")?;

    if let Ok(bwrap) = resolve_binary("bwrap") {
        if bubblewrap_supported(&bwrap, &probe_binary) {
            return Ok(SandboxStrategy::Bubblewrap { bwrap });
        }
    }

    Err(SynapseError::Execution(
        "bubblewrap is required for secure execution on Linux; install bwrap".to_string(),
    ))
}

#[cfg(target_os = "linux")]
fn bubblewrap_supported(bwrap: &Path, probe_binary: &Path) -> bool {
    match StdCommand::new(bwrap)
        .args(bubblewrap_probe_args(probe_binary))
        .stdin(StdStdio::null())
        .stdout(StdStdio::null())
        .stderr(StdStdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn bubblewrap_args(binary: &Path, sandbox_dir: &Path, seccomp_fd: Option<RawFd>) -> Vec<OsString> {
    let mut args = bubblewrap_base_args();
    if let Some(fd) = seccomp_fd {
        args.push(OsString::from("--seccomp"));
        args.push(OsString::from(fd.to_string()));
    }
    args.extend([
        OsString::from("--bind"),
        sandbox_dir.as_os_str().to_os_string(),
        OsString::from(SANDBOX_WORKDIR),
        OsString::from("--ro-bind"),
        OsString::from("/usr"),
        OsString::from("/usr"),
        OsString::from("--ro-bind-try"),
        OsString::from("/bin"),
        OsString::from("/bin"),
        OsString::from("--ro-bind-try"),
        OsString::from("/lib"),
        OsString::from("/lib"),
        OsString::from("--ro-bind-try"),
        OsString::from("/lib64"),
        OsString::from("/lib64"),
        OsString::from("--ro-bind-try"),
        OsString::from("/sbin"),
        OsString::from("/sbin"),
        OsString::from("--chdir"),
        OsString::from(SANDBOX_WORKDIR),
        binary.as_os_str().to_os_string(),
        OsString::from(SANDBOX_SCRIPT_PATH),
    ]);
    args
}

#[cfg(target_os = "linux")]
fn bubblewrap_probe_args(binary: &Path) -> Vec<OsString> {
    let mut args = bubblewrap_base_args();
    args.extend([
        OsString::from("--ro-bind"),
        OsString::from("/usr"),
        OsString::from("/usr"),
        OsString::from("--ro-bind-try"),
        OsString::from("/bin"),
        OsString::from("/bin"),
        OsString::from("--ro-bind-try"),
        OsString::from("/lib"),
        OsString::from("/lib"),
        OsString::from("--ro-bind-try"),
        OsString::from("/lib64"),
        OsString::from("/lib64"),
        OsString::from("--chdir"),
        OsString::from("/"),
        binary.as_os_str().to_os_string(),
    ]);
    args
}

#[cfg(target_os = "linux")]
fn bubblewrap_base_args() -> Vec<OsString> {
    [
        "--die-with-parent",
        "--new-session",
        "--unshare-user",
        "--uid",
        "0",
        "--gid",
        "0",
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
        "--unshare-net",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
        "--dir",
        SANDBOX_WORKDIR,
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

async fn collect_output(
    task: tokio::task::JoinHandle<Result<Vec<u8>, std::io::Error>>,
) -> Result<String, SynapseError> {
    let bytes = task.await.map_err(|err| {
        SynapseError::Execution(format!("failed to collect process output: {err}"))
    })??;
    Ok(truncate_output(&bytes))
}

async fn read_stream<R>(mut stream: R) -> Result<Vec<u8>, std::io::Error>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    Ok(buf)
}

fn configure_command(
    command: &mut Command,
    memory_limit_mb: u32,
    #[cfg(target_os = "linux")] load_seccomp_in_child: bool,
    #[cfg(not(target_os = "linux"))] _load_seccomp_in_child: bool,
) -> Result<(), SynapseError> {
    #[cfg(unix)]
    {
        let memory_limit_bytes = memory_limit_bytes(memory_limit_mb)?;
        unsafe {
            command.pre_exec(move || {
                configure_child_process(
                    memory_limit_bytes,
                    #[cfg(target_os = "linux")]
                    load_seccomp_in_child,
                    #[cfg(not(target_os = "linux"))]
                    false,
                )
            });
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (command, memory_limit_mb);
    }

    Ok(())
}

#[cfg(unix)]
fn configure_child_process(
    memory_limit_bytes: libc::rlim_t,
    #[cfg(target_os = "linux")] load_seccomp_in_child: bool,
    #[cfg(not(target_os = "linux"))] _load_seccomp_in_child: bool,
) -> io::Result<()> {
    set_no_new_privileges()?;
    set_memory_limit(memory_limit_bytes)?;
    #[cfg(target_os = "linux")]
    if load_seccomp_in_child {
        seccomp::load_blacklist_profile()?;
    }
    unsafe {
        libc::umask(0o077);
    }
    Ok(())
}

#[cfg(unix)]
fn set_no_new_privileges() -> io::Result<()> {
    let result = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn set_memory_limit(memory_limit_bytes: libc::rlim_t) -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: memory_limit_bytes,
        rlim_max: memory_limit_bytes,
    };

    let result = unsafe { libc::setrlimit(libc::RLIMIT_AS, &limit) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn memory_limit_bytes(memory_limit_mb: u32) -> Result<libc::rlim_t, SynapseError> {
    let bytes = u64::from(memory_limit_mb)
        .checked_mul(1024 * 1024)
        .ok_or_else(|| SynapseError::InvalidInput("memory_limit_mb is too large".to_string()))?;
    libc::rlim_t::try_from(bytes)
        .map_err(|_| SynapseError::InvalidInput("memory_limit_mb is too large".to_string()))
}

fn sandbox_dir() -> PathBuf {
    temp_path(&SystemProviders, "synapse")
}

fn elapsed_ms(started: Instant) -> u64 {
    let millis = started.elapsed().as_millis();
    millis.min(u128::from(u64::MAX)) as u64
}

fn truncate_output(bytes: &[u8]) -> String {
    let truncated = bytes.len() > OUTPUT_LIMIT_BYTES;
    let slice = if truncated {
        &bytes[..OUTPUT_LIMIT_BYTES]
    } else {
        bytes
    };

    let mut output = String::from_utf8_lossy(slice).into_owned();
    if truncated {
        output.push_str("\n[output truncated]");
    }
    output
}

fn resolve_binary(binary: &str) -> Result<PathBuf, SynapseError> {
    let binary_path = Path::new(binary);
    if binary_path.is_absolute() && binary_path.exists() {
        return canonicalize_binary(binary_path);
    }

    let providers = SystemProviders;
    let Some(path) = find_command(&providers, binary) else {
        return Err(SynapseError::Execution(format!(
            "{binary} is not available in PATH"
        )));
    };

    canonicalize_binary(&path)
}

fn canonicalize_binary(path: &Path) -> Result<PathBuf, SynapseError> {
    std::fs::canonicalize(path).or_else(|_| Ok(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::{
        create_sandbox_dir_blocking, destroy_sandbox_dir_blocking, memory_limit_bytes,
        recreate_sandbox_dir_blocking, truncate_output, write_script, OUTPUT_LIMIT_BYTES,
    };
    use crate::SynapseError;
    use std::{
        env, fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir().join(format!("{prefix}-{nanos}"))
    }

    #[test]
    fn truncate_output_marks_large_streams() {
        let input = vec![b'a'; OUTPUT_LIMIT_BYTES + 1];
        let output = truncate_output(&input);

        assert!(output.ends_with("\n[output truncated]"));
        assert_eq!(
            output.len(),
            OUTPUT_LIMIT_BYTES + "\n[output truncated]".len()
        );
    }

    #[test]
    fn truncate_output_preserves_small_streams() {
        assert_eq!(truncate_output(b"hello"), "hello");
    }

    #[test]
    fn memory_limit_bytes_converts_megabytes() {
        assert_eq!(memory_limit_bytes(128).unwrap(), 128 * 1024 * 1024);
    }

    #[test]
    fn memory_limit_bytes_respects_platform_capacity() {
        let result = memory_limit_bytes(u32::MAX);

        if std::mem::size_of::<libc::rlim_t>() < std::mem::size_of::<u64>() {
            let error = result.unwrap_err();
            assert!(
                matches!(error, SynapseError::InvalidInput(message) if message == "memory_limit_mb is too large")
            );
        } else {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn recreate_sandbox_dir_blocking_removes_existing_contents() {
        let path = unique_path("synapse-runtime-reset");
        create_sandbox_dir_blocking(&path).unwrap();
        fs::write(path.join("stale.txt"), b"stale").unwrap();

        recreate_sandbox_dir_blocking(&path).unwrap();

        assert!(path.is_dir());
        assert!(!path.join("stale.txt").exists());

        let _ = destroy_sandbox_dir_blocking(&path);
    }

    #[tokio::test]
    async fn write_script_persists_code_to_disk() {
        let path = unique_path("synapse-runtime-script");
        create_sandbox_dir_blocking(&path).unwrap();
        let script = path.join("main.py");

        write_script(&script, "print(42)\n").await.unwrap();

        let contents = tokio::fs::read_to_string(&script).await.unwrap();
        assert_eq!(contents, "print(42)\n");

        let _ = destroy_sandbox_dir_blocking(&path);
    }
}
