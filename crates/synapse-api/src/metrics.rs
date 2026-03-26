use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use synapse_core::{ErrorCode, ExecuteResponse};

#[derive(Clone, Debug, Default)]
pub struct ExecutionMetrics {
    inner: Arc<ExecutionMetricsInner>,
}

#[derive(Debug, Default)]
struct ExecutionMetricsInner {
    success_total: AtomicU64,
    error_total: AtomicU64,
    invalid_input_total: AtomicU64,
    unsupported_language_total: AtomicU64,
    runtime_unavailable_total: AtomicU64,
    execution_failed_total: AtomicU64,
    queue_timeout_total: AtomicU64,
    capacity_rejected_total: AtomicU64,
    wall_timeout_total: AtomicU64,
    cpu_time_limit_exceeded_total: AtomicU64,
    memory_limit_exceeded_total: AtomicU64,
    sandbox_policy_blocked_total: AtomicU64,
    quota_exceeded_total: AtomicU64,
    rate_limited_total: AtomicU64,
    audit_failed_total: AtomicU64,
    io_error_total: AtomicU64,
    auth_required_total: AtomicU64,
    auth_invalid_total: AtomicU64,
    tenant_forbidden_total: AtomicU64,
    stdout_truncated_total: AtomicU64,
    stderr_truncated_total: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionMetricsSnapshot {
    pub success_total: u64,
    pub error_total: u64,
    pub invalid_input_total: u64,
    pub unsupported_language_total: u64,
    pub runtime_unavailable_total: u64,
    pub execution_failed_total: u64,
    pub queue_timeout_total: u64,
    pub capacity_rejected_total: u64,
    pub wall_timeout_total: u64,
    pub cpu_time_limit_exceeded_total: u64,
    pub memory_limit_exceeded_total: u64,
    pub sandbox_policy_blocked_total: u64,
    pub quota_exceeded_total: u64,
    pub rate_limited_total: u64,
    pub audit_failed_total: u64,
    pub io_error_total: u64,
    pub auth_required_total: u64,
    pub auth_invalid_total: u64,
    pub tenant_forbidden_total: u64,
    pub stdout_truncated_total: u64,
    pub stderr_truncated_total: u64,
}

impl ExecutionMetrics {
    pub fn record_response(&self, response: &ExecuteResponse) {
        if let Some(output) = &response.output {
            if output.stdout_truncated {
                self.inner
                    .stdout_truncated_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            if output.stderr_truncated {
                self.inner
                    .stderr_truncated_total
                    .fetch_add(1, Ordering::Relaxed);
            }
        }

        if let Some(error) = &response.error {
            self.record_error_code(error.code.clone());
        } else {
            self.inner.success_total.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_error_code(&self, code: ErrorCode) {
        self.inner.error_total.fetch_add(1, Ordering::Relaxed);
        match code {
            ErrorCode::InvalidInput => {
                self.inner
                    .invalid_input_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::UnsupportedLanguage => {
                self.inner
                    .unsupported_language_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::RuntimeUnavailable => {
                self.inner
                    .runtime_unavailable_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::ExecutionFailed => {
                self.inner
                    .execution_failed_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::QueueTimeout => {
                self.inner
                    .queue_timeout_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::CapacityRejected => {
                self.inner
                    .capacity_rejected_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::WallTimeout => {
                self.inner
                    .wall_timeout_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::CpuTimeLimitExceeded => {
                self.inner
                    .cpu_time_limit_exceeded_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::MemoryLimitExceeded => {
                self.inner
                    .memory_limit_exceeded_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::SandboxPolicyBlocked => {
                self.inner
                    .sandbox_policy_blocked_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::QuotaExceeded => {
                self.inner
                    .quota_exceeded_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::RateLimited => {
                self.inner
                    .rate_limited_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::AuditFailed => {
                self.inner
                    .audit_failed_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::IoError => {
                self.inner.io_error_total.fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::AuthRequired => {
                self.inner
                    .auth_required_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::AuthInvalid => {
                self.inner
                    .auth_invalid_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ErrorCode::TenantForbidden => {
                self.inner
                    .tenant_forbidden_total
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn snapshot(&self) -> ExecutionMetricsSnapshot {
        ExecutionMetricsSnapshot {
            success_total: self.inner.success_total.load(Ordering::Relaxed),
            error_total: self.inner.error_total.load(Ordering::Relaxed),
            invalid_input_total: self.inner.invalid_input_total.load(Ordering::Relaxed),
            unsupported_language_total: self
                .inner
                .unsupported_language_total
                .load(Ordering::Relaxed),
            runtime_unavailable_total: self.inner.runtime_unavailable_total.load(Ordering::Relaxed),
            execution_failed_total: self.inner.execution_failed_total.load(Ordering::Relaxed),
            queue_timeout_total: self.inner.queue_timeout_total.load(Ordering::Relaxed),
            capacity_rejected_total: self.inner.capacity_rejected_total.load(Ordering::Relaxed),
            wall_timeout_total: self.inner.wall_timeout_total.load(Ordering::Relaxed),
            cpu_time_limit_exceeded_total: self
                .inner
                .cpu_time_limit_exceeded_total
                .load(Ordering::Relaxed),
            memory_limit_exceeded_total: self
                .inner
                .memory_limit_exceeded_total
                .load(Ordering::Relaxed),
            sandbox_policy_blocked_total: self
                .inner
                .sandbox_policy_blocked_total
                .load(Ordering::Relaxed),
            quota_exceeded_total: self.inner.quota_exceeded_total.load(Ordering::Relaxed),
            rate_limited_total: self.inner.rate_limited_total.load(Ordering::Relaxed),
            audit_failed_total: self.inner.audit_failed_total.load(Ordering::Relaxed),
            io_error_total: self.inner.io_error_total.load(Ordering::Relaxed),
            auth_required_total: self.inner.auth_required_total.load(Ordering::Relaxed),
            auth_invalid_total: self.inner.auth_invalid_total.load(Ordering::Relaxed),
            tenant_forbidden_total: self.inner.tenant_forbidden_total.load(Ordering::Relaxed),
            stdout_truncated_total: self.inner.stdout_truncated_total.load(Ordering::Relaxed),
            stderr_truncated_total: self.inner.stderr_truncated_total.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutionMetrics;
    use synapse_core::{ErrorCode, ExecuteError, ExecuteResponse, OutputSummary};

    #[test]
    fn metrics_record_success_errors_and_truncation() {
        let metrics = ExecutionMetrics::default();

        metrics.record_response(&ExecuteResponse {
            stdout: "ok".to_string(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms: 1,
            request_id: None,
            tenant_id: None,
            runtime: None,
            limits: None,
            output: Some(OutputSummary {
                stdout_truncated: true,
                stderr_truncated: false,
            }),
            error: None,
            audit: None,
            sandbox_audit: Vec::new(),
        });
        metrics.record_response(&ExecuteResponse::error(
            ExecuteError {
                code: ErrorCode::WallTimeout,
                message: "execution timed out".to_string(),
            },
            0,
        ));

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.success_total, 1);
        assert_eq!(snapshot.error_total, 1);
        assert_eq!(snapshot.wall_timeout_total, 1);
        assert_eq!(snapshot.queue_timeout_total, 0);
        assert_eq!(snapshot.capacity_rejected_total, 0);
        assert_eq!(snapshot.stdout_truncated_total, 1);
        assert_eq!(snapshot.stderr_truncated_total, 0);
    }
}
