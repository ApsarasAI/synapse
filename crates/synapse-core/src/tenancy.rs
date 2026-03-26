use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::Serialize;

use crate::{ExecuteRequest, SynapseError};

const DEFAULT_TENANT_ID: &str = "default";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TenantQuotaConfig {
    pub max_concurrent_executions_per_tenant: usize,
    pub max_requests_per_minute: usize,
    pub max_timeout_ms: u64,
    pub max_cpu_time_limit_ms: u64,
    pub max_memory_limit_mb: u32,
    pub max_queue_depth: usize,
    pub max_queue_timeout_ms: u64,
}

impl Default for TenantQuotaConfig {
    fn default() -> Self {
        Self {
            max_concurrent_executions_per_tenant: 2,
            max_requests_per_minute: 120,
            max_timeout_ms: 30_000,
            max_cpu_time_limit_ms: 30_000,
            max_memory_limit_mb: 512,
            max_queue_depth: 32,
            max_queue_timeout_ms: 5_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TenantQuotaManager {
    config: TenantQuotaConfig,
    request_windows: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
}

#[derive(Debug, Default)]
pub struct TenantPermit {
    _private: (),
}

impl TenantQuotaManager {
    pub fn new(config: TenantQuotaConfig) -> Self {
        Self {
            config,
            request_windows: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn config(&self) -> &TenantQuotaConfig {
        &self.config
    }

    pub fn normalize_tenant_id(&self, tenant_id: Option<&str>) -> String {
        tenant_id
            .map(str::trim)
            .filter(|tenant_id| !tenant_id.is_empty())
            .unwrap_or(DEFAULT_TENANT_ID)
            .to_string()
    }

    pub fn enforce_request_limits(&self, request: &ExecuteRequest) -> Result<(), SynapseError> {
        if request.timeout_ms > self.config.max_timeout_ms {
            return Err(SynapseError::QuotaExceeded(format!(
                "timeout_ms exceeds tenant ceiling of {}",
                self.config.max_timeout_ms
            )));
        }

        if request.effective_cpu_time_limit_ms() > self.config.max_cpu_time_limit_ms {
            return Err(SynapseError::QuotaExceeded(format!(
                "cpu_time_limit_ms exceeds tenant ceiling of {}",
                self.config.max_cpu_time_limit_ms
            )));
        }

        if request.memory_limit_mb > self.config.max_memory_limit_mb {
            return Err(SynapseError::QuotaExceeded(format!(
                "memory_limit_mb exceeds tenant ceiling of {}",
                self.config.max_memory_limit_mb
            )));
        }

        Ok(())
    }

    pub fn enforce_rate_limit(&self, tenant_id: &str) -> Result<(), SynapseError> {
        self.enforce_rate_limit_inner(tenant_id)
    }

    fn enforce_rate_limit_inner(&self, tenant_id: &str) -> Result<(), SynapseError> {
        let mut windows = self
            .request_windows
            .lock()
            .expect("tenant request windows mutex poisoned");
        let window = windows.entry(tenant_id.to_string()).or_default();
        let now = Instant::now();
        let horizon = now - Duration::from_secs(60);
        while matches!(window.front(), Some(entry) if *entry < horizon) {
            window.pop_front();
        }
        if window.len() >= self.config.max_requests_per_minute {
            return Err(SynapseError::RateLimited(format!(
                "tenant {tenant_id} exceeded {} requests per minute",
                self.config.max_requests_per_minute
            )));
        }
        window.push_back(now);
        Ok(())
    }
}

impl Default for TenantQuotaManager {
    fn default() -> Self {
        Self::new(TenantQuotaConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::{TenantQuotaConfig, TenantQuotaManager};
    use crate::{ExecuteRequest, NetworkPolicy};

    fn request() -> ExecuteRequest {
        ExecuteRequest {
            language: "python".to_string(),
            code: "print('ok')\n".to_string(),
            timeout_ms: 1_000,
            cpu_time_limit_ms: Some(500),
            memory_limit_mb: 128,
            runtime_version: None,
            tenant_id: None,
            request_id: None,
            network_policy: NetworkPolicy::Disabled,
        }
    }

    #[test]
    fn quotas_reject_oversized_timeout() {
        let manager = TenantQuotaManager::new(TenantQuotaConfig {
            max_timeout_ms: 100,
            ..TenantQuotaConfig::default()
        });
        let error = manager.enforce_request_limits(&request()).unwrap_err();
        assert!(error.to_string().contains("tenant ceiling"));
    }

    #[test]
    fn quotas_normalize_empty_tenant() {
        let manager = TenantQuotaManager::default();
        assert_eq!(manager.normalize_tenant_id(Some("  ")), "default");
    }

    #[test]
    fn rate_limits_reject_excess_requests() {
        let manager = TenantQuotaManager::new(TenantQuotaConfig {
            max_requests_per_minute: 1,
            ..TenantQuotaConfig::default()
        });

        manager.enforce_rate_limit("tenant-a").unwrap();
        let error = manager.enforce_rate_limit("tenant-a").unwrap_err();
        assert!(error.to_string().contains("requests per minute"));
    }
}
