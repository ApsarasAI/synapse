use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{runtimes::RuntimeInfo, AuditEvent};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecuteRequest {
    pub language: String,
    pub code: String,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub cpu_time_limit_ms: Option<u64>,
    #[serde(default = "default_memory_limit_mb")]
    pub memory_limit_mb: u32,
    #[serde(default)]
    pub runtime_version: Option<String>,
    #[serde(default)]
    pub tenant_id: Option<String>,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub network_policy: NetworkPolicy,
}

impl ExecuteRequest {
    pub fn effective_cpu_time_limit_ms(&self) -> u64 {
        self.cpu_time_limit_ms.unwrap_or(self.timeout_ms)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidInput,
    UnsupportedLanguage,
    RuntimeUnavailable,
    ExecutionFailed,
    QueueTimeout,
    CapacityRejected,
    WallTimeout,
    CpuTimeLimitExceeded,
    MemoryLimitExceeded,
    SandboxPolicyBlocked,
    QuotaExceeded,
    RateLimited,
    AuditFailed,
    IoError,
    AuthRequired,
    AuthInvalid,
    TenantForbidden,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::InvalidInput => "invalid_input",
            Self::UnsupportedLanguage => "unsupported_language",
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::ExecutionFailed => "execution_failed",
            Self::QueueTimeout => "queue_timeout",
            Self::CapacityRejected => "capacity_rejected",
            Self::WallTimeout => "wall_timeout",
            Self::CpuTimeLimitExceeded => "cpu_time_limit_exceeded",
            Self::MemoryLimitExceeded => "memory_limit_exceeded",
            Self::SandboxPolicyBlocked => "sandbox_policy_blocked",
            Self::QuotaExceeded => "quota_exceeded",
            Self::RateLimited => "rate_limited",
            Self::AuditFailed => "audit_failed",
            Self::IoError => "io_error",
            Self::AuthRequired => "auth_required",
            Self::AuthInvalid => "auth_invalid",
            Self::TenantForbidden => "tenant_forbidden",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum NetworkPolicy {
    #[default]
    Disabled,
    AllowList {
        hosts: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecuteError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LimitSummary {
    pub wall_time_limit_ms: u64,
    pub cpu_time_limit_ms: u64,
    pub memory_limit_mb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputSummary {
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditSummary {
    pub request_id: String,
    pub event_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecuteResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<LimitSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<OutputSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ExecuteError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit: Option<AuditSummary>,
    #[serde(skip_serializing)]
    pub sandbox_audit: Vec<AuditEvent>,
}

impl ExecuteResponse {
    pub fn error(error: ExecuteError, duration_ms: u64) -> Self {
        Self {
            stdout: String::new(),
            stderr: error.message.clone(),
            exit_code: -1,
            duration_ms,
            request_id: None,
            tenant_id: None,
            runtime: None,
            limits: None,
            output: None,
            error: Some(error),
            audit: None,
            sandbox_audit: Vec::new(),
        }
    }

    pub fn with_request_metadata(
        mut self,
        request_id: impl Into<String>,
        tenant_id: Option<&str>,
        runtime: Option<RuntimeInfo>,
        limits: LimitSummary,
    ) -> Self {
        self.request_id = Some(request_id.into());
        self.tenant_id = tenant_id.map(str::to_string);
        self.runtime = runtime;
        self.limits = Some(limits);
        self
    }
}

const fn default_timeout_ms() -> u64 {
    5_000
}

const fn default_memory_limit_mb() -> u32 {
    128
}
