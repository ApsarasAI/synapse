use thiserror::Error;

use crate::types::{ErrorCode, ExecuteError};

#[derive(Debug, Error)]
pub enum SynapseError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("unsupported language: {0}")]
    UnsupportedLanguage(String),
    #[error("runtime unavailable: {0}")]
    RuntimeUnavailable(String),
    #[error("execution failed: {0}")]
    Execution(String),
    #[error("queue timeout: {0}")]
    QueueTimeout(String),
    #[error("capacity rejected: {0}")]
    CapacityRejected(String),
    #[error("execution timed out")]
    WallTimeout,
    #[error("cpu time limit exceeded")]
    CpuTimeLimitExceeded,
    #[error("memory limit exceeded")]
    MemoryLimitExceeded,
    #[error("sandbox policy blocked: {0}")]
    SandboxPolicy(String),
    #[error("tenant quota exceeded: {0}")]
    QuotaExceeded(String),
    #[error("tenant rate limited: {0}")]
    RateLimited(String),
    #[error("authentication required: {0}")]
    AuthRequired(String),
    #[error("authentication failed: {0}")]
    AuthInvalid(String),
    #[error("tenant access forbidden: {0}")]
    TenantForbidden(String),
    #[error("audit failed: {0}")]
    Audit(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

impl SynapseError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidInput(_) => ErrorCode::InvalidInput,
            Self::UnsupportedLanguage(_) => ErrorCode::UnsupportedLanguage,
            Self::RuntimeUnavailable(_) => ErrorCode::RuntimeUnavailable,
            Self::Execution(_) | Self::Internal(_) => ErrorCode::ExecutionFailed,
            Self::QueueTimeout(_) => ErrorCode::QueueTimeout,
            Self::CapacityRejected(_) => ErrorCode::CapacityRejected,
            Self::WallTimeout => ErrorCode::WallTimeout,
            Self::CpuTimeLimitExceeded => ErrorCode::CpuTimeLimitExceeded,
            Self::MemoryLimitExceeded => ErrorCode::MemoryLimitExceeded,
            Self::SandboxPolicy(_) => ErrorCode::SandboxPolicyBlocked,
            Self::QuotaExceeded(_) => ErrorCode::QuotaExceeded,
            Self::RateLimited(_) => ErrorCode::RateLimited,
            Self::AuthRequired(_) => ErrorCode::AuthRequired,
            Self::AuthInvalid(_) => ErrorCode::AuthInvalid,
            Self::TenantForbidden(_) => ErrorCode::TenantForbidden,
            Self::Audit(_) => ErrorCode::AuditFailed,
            Self::Io(_) => ErrorCode::IoError,
        }
    }

    pub fn to_execute_error(&self) -> ExecuteError {
        ExecuteError {
            code: self.code(),
            message: self.public_message(),
        }
    }

    fn public_message(&self) -> String {
        match self {
            Self::InvalidInput(_)
            | Self::UnsupportedLanguage(_)
            | Self::RuntimeUnavailable(_)
            | Self::QueueTimeout(_)
            | Self::CapacityRejected(_)
            | Self::SandboxPolicy(_)
            | Self::QuotaExceeded(_)
            | Self::RateLimited(_)
            | Self::AuthRequired(_)
            | Self::AuthInvalid(_)
            | Self::TenantForbidden(_) => self.to_string(),
            Self::WallTimeout => self.to_string(),
            Self::CpuTimeLimitExceeded => self.to_string(),
            Self::MemoryLimitExceeded => self.to_string(),
            Self::Execution(_) => "execution failed".to_string(),
            Self::Audit(_) => "audit failed".to_string(),
            Self::Io(_) => "io error".to_string(),
            Self::Internal(_) => "internal error".to_string(),
        }
    }
}
