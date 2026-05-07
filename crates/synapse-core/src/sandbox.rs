use std::{fmt::Debug, future::Future, pin::Pin};

use crate::{ExecuteResponse, NetworkPolicy, ResolvedRuntime, SynapseError};

pub type SandboxFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, SynapseError>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SandboxCapabilities {
    pub network_disabled: bool,
    pub network_allow_list: bool,
    pub cpu_accounting: bool,
    pub memory_cgroup: bool,
    pub audit_capture: bool,
    pub warm_pooling: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct SandboxExecution<'a> {
    pub runtime: &'a ResolvedRuntime,
    pub code: &'a str,
    pub wall_timeout_ms: u64,
    pub cpu_time_limit_ms: u64,
    pub memory_limit_mb: u32,
    pub network_policy: &'a NetworkPolicy,
}

pub trait SandboxInstance: Debug + Send + Sync {
    fn reset<'a>(&'a self) -> SandboxFuture<'a, ()>;

    fn reset_blocking(&self) -> Result<(), SynapseError>;

    fn execute<'a>(&'a self, execution: SandboxExecution<'a>)
        -> SandboxFuture<'a, ExecuteResponse>;

    fn destroy_blocking(self: Box<Self>) -> Result<(), SynapseError>;
}

pub trait SandboxEngine: Debug + Send + Sync {
    fn name(&self) -> &'static str;

    fn capabilities(&self) -> SandboxCapabilities;

    fn prepare<'a>(&'a self) -> SandboxFuture<'a, Box<dyn SandboxInstance>>;

    fn prepare_blocking(&self) -> Result<Box<dyn SandboxInstance>, SynapseError> {
        Err(SynapseError::RuntimeUnavailable(format!(
            "blocking sandbox preparation is not supported by the {} engine",
            self.name()
        )))
    }

    fn execute_disposable<'a>(
        &'a self,
        execution: SandboxExecution<'a>,
    ) -> SandboxFuture<'a, ExecuteResponse> {
        Box::pin(async move {
            let sandbox = self.prepare().await?;
            let result = sandbox.execute(execution).await;
            let _ = sandbox.destroy_blocking();
            result
        })
    }
}

pub use crate::runtime::BubblewrapEngine as DefaultSandboxEngine;
