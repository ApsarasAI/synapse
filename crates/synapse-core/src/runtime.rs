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
    time::sleep,
};

#[cfg(target_os = "linux")]
use crate::cgroups::ExecutionCgroup;
#[cfg(target_os = "linux")]
use crate::seccomp::{self, ExportedSeccompFilter};
use crate::{
    find_command,
    sandbox::{
        SandboxCapabilities, SandboxEngine, SandboxExecution, SandboxFuture, SandboxInstance,
    },
    syscall_audit::collect_trace_audit_events,
    temp_path, ExecuteResponse, SynapseError, SystemProviders,
};

const OUTPUT_LIMIT_BYTES: usize = 1024 * 1024;
const MINIMAL_PATH: &str = "/usr/bin:/bin";
const SANDBOX_WORKDIR: &str = "/workspace";
const SANDBOX_SCRIPT_PATH: &str = "/workspace/main.py";
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

pub type DefaultSandboxEngine = BubblewrapEngine;

#[derive(Clone, Debug, Default)]
pub struct BubblewrapEngine;

#[derive(Clone, Debug)]
pub(crate) struct BubblewrapSandboxInstance {
    root: PathBuf,
    upper: PathBuf,
}

#[derive(Debug)]
struct OutputCapture {
    content: String,
    truncated: bool,
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

impl BubblewrapEngine {
    #[cfg(target_os = "linux")]
    pub fn probe(&self) -> Result<String, SynapseError> {
        match detect_linux_sandbox_strategy()? {
            SandboxStrategy::Bubblewrap { bwrap } => Ok(format!(
                "bubblewrap overlay sandbox available ({})",
                bwrap.display()
            )),
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn probe(&self) -> Result<String, SynapseError> {
        Ok("direct process sandbox available for non-Linux development".to_string())
    }
}

impl SandboxEngine for BubblewrapEngine {
    fn name(&self) -> &'static str {
        "bubblewrap"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            network_disabled: true,
            network_allow_list: false,
            cpu_accounting: cfg!(target_os = "linux"),
            memory_cgroup: cfg!(target_os = "linux"),
            audit_capture: cfg!(target_os = "linux"),
            warm_pooling: true,
        }
    }

    fn prepare<'a>(&'a self) -> SandboxFuture<'a, Box<dyn SandboxInstance>> {
        Box::pin(async move {
            let root = sandbox_dir();
            create_sandbox_layout(&root).await?;
            Ok(Box::new(BubblewrapSandboxInstance::new(root)) as Box<dyn SandboxInstance>)
        })
    }

    fn prepare_blocking(&self) -> Result<Box<dyn SandboxInstance>, SynapseError> {
        let root = sandbox_dir();
        create_sandbox_layout_blocking(&root)?;
        Ok(Box::new(BubblewrapSandboxInstance::new(root)))
    }
}

impl BubblewrapSandboxInstance {
    fn new(root: PathBuf) -> Self {
        Self {
            upper: root.join("upper"),
            root,
        }
    }
}

impl SandboxInstance for BubblewrapSandboxInstance {
    fn reset<'a>(&'a self) -> SandboxFuture<'a, ()> {
        Box::pin(async move {
            recreate_sandbox_layout(&self.root)
                .await
                .map_err(Into::into)
        })
    }

    fn reset_blocking(&self) -> Result<(), SynapseError> {
        recreate_sandbox_layout_blocking(&self.root).map_err(Into::into)
    }

    fn execute<'a>(
        &'a self,
        execution: SandboxExecution<'a>,
    ) -> SandboxFuture<'a, ExecuteResponse> {
        Box::pin(async move {
            let artifact = execution.runtime.artifact();
            execute_binary(
                artifact.binary(),
                artifact.workspace_lowerdir(),
                execution.code,
                &self.upper,
                execution.wall_timeout_ms,
                execution.cpu_time_limit_ms,
                execution.memory_limit_mb,
            )
            .await
        })
    }

    fn destroy_blocking(self: Box<Self>) -> Result<(), SynapseError> {
        destroy_sandbox_layout_blocking(&self.root).map_err(Into::into)
    }
}

pub(crate) async fn execute_binary(
    binary: &Path,
    workspace_lowerdir: &Path,
    code: &str,
    sandbox_dir: &Path,
    wall_timeout_ms: u64,
    cpu_time_limit_ms: u64,
    memory_limit_mb: u32,
) -> Result<ExecuteResponse, SynapseError> {
    let script_path = sandbox_dir.join("main.py");
    write_script(&script_path, code).await?;
    run_process(
        binary,
        workspace_lowerdir,
        &script_path,
        sandbox_dir,
        wall_timeout_ms,
        cpu_time_limit_ms,
        memory_limit_mb,
    )
    .await
}

async fn create_sandbox_layout(path: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(path.join("upper")).await?;
    fs::create_dir_all(path.join("work")).await?;

    #[cfg(unix)]
    {
        fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
        fs::set_permissions(path.join("upper"), std::fs::Permissions::from_mode(0o700)).await?;
        fs::set_permissions(path.join("work"), std::fs::Permissions::from_mode(0o700)).await?;
    }

    Ok(())
}

fn create_sandbox_layout_blocking(path: &Path) -> Result<(), std::io::Error> {
    stdfs::create_dir_all(path.join("upper"))?;
    stdfs::create_dir_all(path.join("work"))?;

    #[cfg(unix)]
    {
        stdfs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        stdfs::set_permissions(path.join("upper"), std::fs::Permissions::from_mode(0o700))?;
        stdfs::set_permissions(path.join("work"), std::fs::Permissions::from_mode(0o700))?;
    }

    Ok(())
}

async fn recreate_sandbox_layout(path: &Path) -> Result<(), std::io::Error> {
    if fs::try_exists(path).await? {
        fs::remove_dir_all(path).await?;
    }
    create_sandbox_layout(path).await
}

fn recreate_sandbox_layout_blocking(path: &Path) -> Result<(), std::io::Error> {
    if path.exists() {
        stdfs::remove_dir_all(path)?;
    }
    create_sandbox_layout_blocking(path)
}

fn destroy_sandbox_layout_blocking(path: &Path) -> Result<(), std::io::Error> {
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
    workspace_lowerdir: &Path,
    script_path: &Path,
    sandbox_dir: &Path,
    wall_timeout_ms: u64,
    cpu_time_limit_ms: u64,
    memory_limit_mb: u32,
) -> Result<ExecuteResponse, SynapseError> {
    let started = Instant::now();
    let strategy = sandbox_strategy()?;
    #[cfg(target_os = "linux")]
    let execution_cgroup = ExecutionCgroup::try_create(&SystemProviders, memory_limit_mb)?;
    #[cfg(target_os = "linux")]
    require_cpu_limit_support(
        cpu_time_limit_ms,
        wall_timeout_ms,
        execution_cgroup.is_some(),
    )?;
    #[cfg(target_os = "linux")]
    let seccomp_plan = prepare_seccomp(&strategy, sandbox_dir)?;
    let mut command = build_command(
        &strategy,
        binary,
        workspace_lowerdir,
        script_path,
        sandbox_dir,
        #[cfg(target_os = "linux")]
        seccomp_plan.bubblewrap_fd(),
        #[cfg(not(target_os = "linux"))]
        None,
    );
    #[cfg(target_os = "linux")]
    let trace_prefix = sandbox_dir
        .parent()
        .unwrap_or(sandbox_dir)
        .join(format!("trace-{}", std::process::id()));
    #[cfg(target_os = "linux")]
    wrap_with_strace(&mut command, &trace_prefix)?;
    command
        .stdin(StdStdio::null())
        .stdout(StdStdio::piped())
        .stderr(StdStdio::piped())
        .env_clear()
        .env("PATH", MINIMAL_PATH)
        .env("LANG", "C.UTF-8")
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONUNBUFFERED", "1")
        .process_group(0)
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
    if let Some(execution_cgroup) = execution_cgroup.as_ref() {
        let pid = child.id().ok_or_else(|| {
            SynapseError::Execution("failed to read spawned process id".to_string())
        })?;
        if let Err(error) = execution_cgroup.attach(pid) {
            kill_process_group(pid);
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
    }
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

    let wait_result = wait_for_process(
        &mut child,
        wall_timeout_ms,
        cpu_time_limit_ms,
        #[cfg(target_os = "linux")]
        execution_cgroup.as_ref(),
    )
    .await?;
    let duration_ms = elapsed_ms(started);

    match wait_result {
        ProcessOutcome::Exited(status) => {
            let stdout = collect_output(stdout_task).await?;
            let stderr = collect_output(stderr_task).await?;
            let output = output_summary(&stdout, &stderr);
            #[cfg(target_os = "linux")]
            let sandbox_audit = collect_trace_audit_events("", None, &trace_prefix);
            #[cfg(not(target_os = "linux"))]
            let sandbox_audit = Vec::new();
            let memory_limit_exceeded = memory_limit_exceeded(
                &stderr.content,
                #[cfg(target_os = "linux")]
                execution_cgroup.as_ref(),
            )?;

            Ok(ExecuteResponse {
                stdout: stdout.content,
                stderr: if memory_limit_exceeded {
                    append_limit_message(stderr.content, "memory limit exceeded")
                } else {
                    stderr.content
                },
                exit_code: if memory_limit_exceeded {
                    -1
                } else {
                    status.code().unwrap_or(-1)
                },
                duration_ms,
                request_id: None,
                tenant_id: None,
                runtime: None,
                limits: None,
                output: Some(output),
                error: if memory_limit_exceeded {
                    Some(SynapseError::MemoryLimitExceeded.to_execute_error())
                } else {
                    None
                },
                audit: None,
                sandbox_audit,
            })
        }
        ProcessOutcome::WallTimeout => {
            if let Some(pid) = child.id() {
                kill_process_group(pid);
            }
            let _ = child.kill().await;
            let _ = child.wait().await;

            let stdout = collect_output(stdout_task).await?;
            let stderr = collect_output(stderr_task).await?;
            let output = output_summary(&stdout, &stderr);
            #[cfg(target_os = "linux")]
            let sandbox_audit = collect_trace_audit_events("", None, &trace_prefix);
            #[cfg(not(target_os = "linux"))]
            let sandbox_audit = Vec::new();

            Ok(ExecuteResponse {
                stdout: stdout.content,
                stderr: append_limit_message(stderr.content, "execution timed out"),
                exit_code: -1,
                duration_ms,
                request_id: None,
                tenant_id: None,
                runtime: None,
                limits: None,
                output: Some(output),
                error: Some(SynapseError::WallTimeout.to_execute_error()),
                audit: None,
                sandbox_audit,
            })
        }
        ProcessOutcome::CpuTimeLimitExceeded => {
            if let Some(pid) = child.id() {
                kill_process_group(pid);
            }
            let _ = child.kill().await;
            let _ = child.wait().await;

            let stdout = collect_output(stdout_task).await?;
            let stderr = collect_output(stderr_task).await?;
            let output = output_summary(&stdout, &stderr);
            #[cfg(target_os = "linux")]
            let sandbox_audit = collect_trace_audit_events("", None, &trace_prefix);
            #[cfg(not(target_os = "linux"))]
            let sandbox_audit = Vec::new();

            Ok(ExecuteResponse {
                stdout: stdout.content,
                stderr: append_limit_message(stderr.content, "cpu time limit exceeded"),
                exit_code: -1,
                duration_ms,
                request_id: None,
                tenant_id: None,
                runtime: None,
                limits: None,
                output: Some(output),
                error: Some(SynapseError::CpuTimeLimitExceeded.to_execute_error()),
                audit: None,
                sandbox_audit,
            })
        }
    }
}

#[cfg(target_os = "linux")]
fn require_cpu_limit_support(
    cpu_time_limit_ms: u64,
    wall_timeout_ms: u64,
    cgroup_available: bool,
) -> Result<(), SynapseError> {
    if cgroup_available || cpu_time_limit_ms >= wall_timeout_ms {
        return Ok(());
    }

    Err(SynapseError::RuntimeUnavailable(
        "cpu_time_limit_ms below timeout_ms requires cgroups v2 support".to_string(),
    ))
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
    workspace_lowerdir: &Path,
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
                .args(bubblewrap_args(
                    binary,
                    workspace_lowerdir,
                    sandbox_dir,
                    seccomp_fd,
                ))
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
pub fn probe_linux_sandbox_support() -> Result<String, SynapseError> {
    BubblewrapEngine.probe()
}

#[cfg(target_os = "linux")]
fn wrap_with_strace(command: &mut Command, trace_prefix: &Path) -> Result<(), SynapseError> {
    let strace = resolve_binary("strace").map_err(|_| {
        SynapseError::Audit("strace is required for sandbox audit capture".to_string())
    })?;
    let program = command.as_std().get_program().to_os_string();
    let args: Vec<_> = command
        .as_std()
        .get_args()
        .map(|arg| arg.to_os_string())
        .collect();
    let current_dir = command.as_std().get_current_dir().map(PathBuf::from);
    let std_command = command.as_std_mut();
    *std_command = StdCommand::new(strace);
    std_command.args([
        OsString::from("-ff"),
        OsString::from("-qq"),
        OsString::from("-s"),
        OsString::from("256"),
        OsString::from("-e"),
        OsString::from("trace=%file,%network,%process"),
        OsString::from("-o"),
        trace_prefix.as_os_str().to_os_string(),
        program,
    ]);
    std_command.args(args);
    if let Some(current_dir) = current_dir {
        std_command.current_dir(current_dir);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn detect_linux_sandbox_strategy() -> Result<SandboxStrategy, SynapseError> {
    if let Ok(bwrap) = resolve_binary("bwrap") {
        if bubblewrap_supported(&bwrap) {
            return Ok(SandboxStrategy::Bubblewrap { bwrap });
        }
    }

    Err(SynapseError::RuntimeUnavailable(
        "bubblewrap with overlay support is required for secure execution on Linux".to_string(),
    ))
}

#[cfg(target_os = "linux")]
fn bubblewrap_args(
    binary: &Path,
    workspace_lowerdir: &Path,
    sandbox_dir: &Path,
    seccomp_fd: Option<RawFd>,
) -> Vec<OsString> {
    let mut args = bubblewrap_base_args();
    if let Some(fd) = seccomp_fd {
        args.push(OsString::from("--seccomp"));
        args.push(OsString::from(fd.to_string()));
    }
    let work_dir = sandbox_dir.parent().unwrap_or(sandbox_dir).join("work");
    args.extend([
        OsString::from("--overlay-src"),
        workspace_lowerdir.as_os_str().to_os_string(),
        OsString::from("--overlay"),
        sandbox_dir.as_os_str().to_os_string(),
        work_dir.as_os_str().to_os_string(),
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
        sandbox_runtime_binary(binary),
        OsString::from(SANDBOX_SCRIPT_PATH),
    ]);
    args
}

#[cfg(target_os = "linux")]
fn sandbox_runtime_binary(binary: &Path) -> OsString {
    let name = binary
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("python3");
    OsString::from(format!("{SANDBOX_WORKDIR}/{name}"))
}

#[cfg(target_os = "linux")]
fn bubblewrap_supported(bwrap: &Path) -> bool {
    let probe_root = temp_path(&SystemProviders, "synapse-bwrap-probe");
    let lowerdir = probe_root.join("lower");
    let upperdir = probe_root.join("upper");
    let workdir = probe_root.join("work");
    let marker = lowerdir.join("marker.txt");
    let result = (|| -> Result<bool, std::io::Error> {
        stdfs::create_dir_all(&lowerdir)?;
        stdfs::create_dir_all(&upperdir)?;
        stdfs::create_dir_all(&workdir)?;
        stdfs::write(&marker, b"ok")?;

        let status = StdCommand::new(bwrap)
            .args(bubblewrap_probe_args(&lowerdir, &upperdir, &workdir))
            .stdin(StdStdio::null())
            .stdout(StdStdio::null())
            .stderr(StdStdio::null())
            .status()?;
        Ok(status.success())
    })();
    let _ = stdfs::remove_dir_all(&probe_root);
    result.unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn bubblewrap_probe_args(lowerdir: &Path, upperdir: &Path, workdir: &Path) -> Vec<OsString> {
    let shell = resolve_binary("sh").unwrap_or_else(|_| PathBuf::from("/bin/sh"));
    let mut args = bubblewrap_base_args();
    args.extend([
        OsString::from("--overlay-src"),
        lowerdir.as_os_str().to_os_string(),
        OsString::from("--overlay"),
        upperdir.as_os_str().to_os_string(),
        workdir.as_os_str().to_os_string(),
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
        shell.as_os_str().to_os_string(),
        OsString::from("-lc"),
        OsString::from(
            "test -f /workspace/marker.txt && echo probe > /workspace/write.txt && test -f /workspace/write.txt",
        ),
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

#[derive(Debug)]
enum ProcessOutcome {
    Exited(std::process::ExitStatus),
    WallTimeout,
    CpuTimeLimitExceeded,
}

async fn wait_for_process(
    child: &mut tokio::process::Child,
    wall_timeout_ms: u64,
    cpu_time_limit_ms: u64,
    #[cfg(target_os = "linux")] execution_cgroup: Option<&ExecutionCgroup>,
) -> Result<ProcessOutcome, SynapseError> {
    let started = Instant::now();
    let timeout = Duration::from_millis(wall_timeout_ms);
    #[cfg(target_os = "linux")]
    let starting_cpu_usage_usec = match execution_cgroup {
        Some(execution_cgroup) => Some(execution_cgroup.cpu_usage_usec()?),
        None => None,
    };

    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(ProcessOutcome::Exited(status));
        }

        if started.elapsed() >= timeout {
            return Ok(ProcessOutcome::WallTimeout);
        }

        #[cfg(target_os = "linux")]
        if let (Some(execution_cgroup), Some(starting_cpu_usage_usec)) =
            (execution_cgroup, starting_cpu_usage_usec)
        {
            let current_cpu_usage_usec = execution_cgroup.cpu_usage_usec()?;
            if current_cpu_usage_usec.saturating_sub(starting_cpu_usage_usec)
                >= cpu_time_limit_usec(cpu_time_limit_ms)
            {
                return Ok(ProcessOutcome::CpuTimeLimitExceeded);
            }
        }

        sleep(process_poll_interval(started, timeout)).await;
    }
}

fn process_poll_interval(started: Instant, timeout: Duration) -> Duration {
    let remaining = timeout.saturating_sub(started.elapsed());
    remaining.min(PROCESS_POLL_INTERVAL)
}

fn append_limit_message(mut stderr: String, message: &str) -> String {
    if !stderr.is_empty() {
        stderr.push('\n');
    }
    stderr.push_str(message);
    stderr
}

#[cfg(target_os = "linux")]
fn cpu_time_limit_usec(timeout_ms: u64) -> u64 {
    timeout_ms.saturating_mul(1_000)
}

/// Kill the entire process group by sending SIGKILL to -pid.
/// When `process_group(0)` is set on the command, the child and all
/// its descendants share the same process group. Killing the group
/// ensures strace-wrapped processes don't become orphans.
#[cfg(unix)]
fn kill_process_group(pid: u32) {
    // Negative PID means send signal to the process group
    let pgid = -(pid as i32);
    unsafe {
        libc::kill(pgid, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_process_group(_pid: u32) {
    // On non-Unix systems, fall back to nothing (child.kill() will be used)
}
async fn collect_output(
    task: tokio::task::JoinHandle<Result<Vec<u8>, std::io::Error>>,
) -> Result<OutputCapture, SynapseError> {
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

fn truncate_output(bytes: &[u8]) -> OutputCapture {
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
    OutputCapture {
        content: output,
        truncated,
    }
}

fn output_summary(stdout: &OutputCapture, stderr: &OutputCapture) -> crate::OutputSummary {
    crate::OutputSummary {
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    }
}

fn memory_limit_exceeded(
    stderr: &str,
    #[cfg(target_os = "linux")] execution_cgroup: Option<&ExecutionCgroup>,
) -> Result<bool, SynapseError> {
    if stderr.contains("MemoryError") {
        return Ok(true);
    }

    #[cfg(target_os = "linux")]
    if let Some(execution_cgroup) = execution_cgroup {
        let events = execution_cgroup.memory_events()?;
        if events.oom > 0 || events.oom_kill > 0 {
            return Ok(true);
        }
    }

    Ok(false)
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
        append_limit_message, create_sandbox_layout_blocking, destroy_sandbox_layout_blocking,
        memory_limit_bytes, memory_limit_exceeded, output_summary,
        recreate_sandbox_layout_blocking, truncate_output, write_script, OutputCapture,
        OUTPUT_LIMIT_BYTES,
    };
    #[cfg(target_os = "linux")]
    use super::{cpu_time_limit_usec, require_cpu_limit_support};
    use crate::{OutputSummary, SynapseError};
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

        assert!(output.content.ends_with("\n[output truncated]"));
        assert!(output.truncated);
        assert_eq!(
            output.content.len(),
            OUTPUT_LIMIT_BYTES + "\n[output truncated]".len()
        );
    }

    #[test]
    fn truncate_output_preserves_small_streams() {
        let output = truncate_output(b"hello");
        assert_eq!(output.content, "hello");
        assert!(!output.truncated);
    }

    #[test]
    fn output_summary_reports_truncation_per_stream() {
        let summary = output_summary(
            &OutputCapture {
                content: "stdout".to_string(),
                truncated: true,
            },
            &OutputCapture {
                content: "stderr".to_string(),
                truncated: false,
            },
        );

        assert_eq!(
            summary,
            OutputSummary {
                stdout_truncated: true,
                stderr_truncated: false,
            }
        );
    }

    #[test]
    fn memory_limit_exceeded_detects_memory_error_traceback() {
        assert!(memory_limit_exceeded(
            "Traceback (most recent call last):\nMemoryError\n",
            #[cfg(target_os = "linux")]
            None,
        )
        .unwrap());
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

    #[cfg(target_os = "linux")]
    #[test]
    fn cpu_limits_require_cgroup_support_when_more_strict_than_wall_timeout() {
        let error = require_cpu_limit_support(50, 500, false).unwrap_err();
        assert!(matches!(error, SynapseError::RuntimeUnavailable(_)));
    }

    #[test]
    fn recreate_sandbox_layout_blocking_removes_existing_contents() {
        let path = unique_path("synapse-runtime-reset");
        create_sandbox_layout_blocking(&path).unwrap();
        fs::write(path.join("stale.txt"), b"stale").unwrap();

        recreate_sandbox_layout_blocking(&path).unwrap();

        assert!(path.is_dir());
        assert!(!path.join("stale.txt").exists());

        let _ = destroy_sandbox_layout_blocking(&path);
    }

    #[tokio::test]
    async fn write_script_persists_code_to_disk() {
        let path = unique_path("synapse-runtime-script");
        create_sandbox_layout_blocking(&path).unwrap();
        let script = path.join("main.py");

        write_script(&script, "print(42)\n").await.unwrap();

        let contents = tokio::fs::read_to_string(&script).await.unwrap();
        assert_eq!(contents, "print(42)\n");

        let _ = destroy_sandbox_layout_blocking(&path);
    }

    #[test]
    fn append_limit_message_appends_after_existing_stderr() {
        assert_eq!(
            append_limit_message("traceback".to_string(), "execution timed out"),
            "traceback\nexecution timed out"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cpu_time_limit_usec_matches_timeout_budget() {
        assert_eq!(cpu_time_limit_usec(250), 250_000);
    }
}
