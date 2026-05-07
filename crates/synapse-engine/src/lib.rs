use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fmt::Debug,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use thiserror::Error;

#[cfg(target_os = "linux")]
pub mod cgroups;
pub mod runtime;
#[cfg(target_os = "linux")]
pub mod seccomp;
pub mod syscall_audit;

pub use runtime::BubblewrapEngine as DefaultSandboxEngine;
pub use runtime::{probe_linux_sandbox_support, BubblewrapEngine};

pub type SandboxFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, SandboxError>> + Send + 'a>>;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("runtime unavailable: {0}")]
    RuntimeUnavailable(String),
    #[error("execution failed: {0}")]
    Execution(String),
    #[error("execution timed out")]
    WallTimeout,
    #[error("cpu time limit exceeded")]
    CpuTimeLimitExceeded,
    #[error("memory limit exceeded")]
    MemoryLimitExceeded,
    #[error("sandbox policy blocked: {0}")]
    SandboxPolicy(String),
    #[error("audit failed: {0}")]
    Audit(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxErrorKind {
    WallTimeout,
    CpuTimeLimitExceeded,
    MemoryLimitExceeded,
    SandboxPolicyBlocked,
    RuntimeUnavailable,
    ExecutionFailed,
    AuditFailed,
    IoError,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SandboxCapabilities {
    pub network_disabled: bool,
    pub network_allow_list: bool,
    pub cpu_accounting: bool,
    pub memory_cgroup: bool,
    pub audit_capture: bool,
    pub warm_pooling: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineNetworkPolicy<'a> {
    Disabled,
    AllowList { hosts: &'a [String] },
}

#[derive(Clone, Debug)]
pub struct EngineRuntimeArtifact {
    binary: PathBuf,
    workspace_lowerdir: PathBuf,
    command: String,
}

impl EngineRuntimeArtifact {
    pub fn new(
        binary: impl Into<PathBuf>,
        workspace_lowerdir: impl Into<PathBuf>,
        command: impl Into<String>,
    ) -> Self {
        Self {
            binary: binary.into(),
            workspace_lowerdir: workspace_lowerdir.into(),
            command: command.into(),
        }
    }

    pub fn binary(&self) -> &Path {
        &self.binary
    }

    pub fn workspace_lowerdir(&self) -> &Path {
        &self.workspace_lowerdir
    }

    pub fn command(&self) -> &str {
        &self.command
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SandboxExecution<'a> {
    pub runtime: &'a EngineRuntimeArtifact,
    pub code: &'a str,
    pub wall_timeout_ms: u64,
    pub cpu_time_limit_ms: u64,
    pub memory_limit_mb: u32,
    pub network_policy: EngineNetworkPolicy<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxOutputSummary {
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub output: SandboxOutputSummary,
    pub error: Option<SandboxErrorKind>,
    pub sandbox_audit: Vec<SandboxAuditEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxAuditKind {
    FileAccess,
    NetworkAttempt,
    ProcessSpawn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxAuditEvent {
    pub kind: SandboxAuditKind,
    pub message: String,
    pub fields: BTreeMap<String, String>,
}

pub fn sandbox_audit_event(
    kind: SandboxAuditKind,
    message: impl Into<String>,
    fields: BTreeMap<String, String>,
) -> SandboxAuditEvent {
    SandboxAuditEvent {
        kind,
        message: message.into(),
        fields,
    }
}

pub trait SandboxInstance: Debug + Send + Sync {
    fn reset<'a>(&'a self) -> SandboxFuture<'a, ()>;

    fn reset_blocking(&self) -> Result<(), SandboxError>;

    fn execute<'a>(&'a self, execution: SandboxExecution<'a>) -> SandboxFuture<'a, SandboxOutput>;

    fn destroy_blocking(self: Box<Self>) -> Result<(), SandboxError>;
}

pub trait SandboxEngine: Debug + Send + Sync {
    fn name(&self) -> &'static str;

    fn capabilities(&self) -> SandboxCapabilities;

    fn prepare<'a>(&'a self) -> SandboxFuture<'a, Box<dyn SandboxInstance>>;

    fn prepare_blocking(&self) -> Result<Box<dyn SandboxInstance>, SandboxError> {
        Err(SandboxError::RuntimeUnavailable(format!(
            "blocking sandbox preparation is not supported by the {} engine",
            self.name()
        )))
    }

    fn execute_disposable<'a>(
        &'a self,
        execution: SandboxExecution<'a>,
    ) -> SandboxFuture<'a, SandboxOutput> {
        Box::pin(async move {
            let sandbox = self.prepare().await?;
            let result = sandbox.execute(execution).await;
            let _ = sandbox.destroy_blocking();
            result
        })
    }
}

pub trait Providers: Send + Sync {
    fn env_var(&self, key: &str) -> Option<String>;
    fn env_var_os(&self, key: &str) -> Option<OsString>;
    fn temp_dir(&self) -> PathBuf;
    fn process_id(&self) -> u32;
    fn now_unix_nanos(&self) -> u128;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProviders;

impl Providers for SystemProviders {
    fn env_var(&self, key: &str) -> Option<String> {
        env::var(key).ok()
    }

    fn env_var_os(&self, key: &str) -> Option<OsString> {
        env::var_os(key)
    }

    fn temp_dir(&self) -> PathBuf {
        env::temp_dir()
    }

    fn process_id(&self) -> u32 {
        process::id()
    }

    fn now_unix_nanos(&self) -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    }
}

pub fn find_command(providers: &dyn Providers, command: &str) -> Option<PathBuf> {
    let path = providers.env_var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(command))
        .find(|candidate| candidate.is_file())
}

pub fn temp_path(providers: &dyn Providers, prefix: &str) -> PathBuf {
    let pid = providers.process_id();
    let nanos = providers.now_unix_nanos();
    providers.temp_dir().join(format!("{prefix}-{pid}-{nanos}"))
}
