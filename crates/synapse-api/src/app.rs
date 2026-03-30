use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use synapse_core::{
    AuditLog, ExecutionScheduler, ExecutionSchedulerConfig, Providers, RequestSummaryStore,
    RuntimeRegistry, SandboxPool, SynapseConfig, SynapseError, SystemProviders, TenantQuotaConfig,
    TenantQuotaManager,
};

use crate::metrics::ExecutionMetrics;

const API_TOKENS_ENV: &str = "SYNAPSE_API_TOKENS";

#[derive(Clone, Debug)]
pub struct AppState {
    pool: SandboxPool,
    audit_log: AuditLog,
    request_summaries: RequestSummaryStore,
    tenant_quotas: TenantQuotaManager,
    scheduler: ExecutionScheduler,
    execution_metrics: ExecutionMetrics,
    runtime_registry: RuntimeRegistry,
    auth: ApiAuthConfig,
    started_at_ms: u64,
}

impl AppState {
    pub fn new(pool: SandboxPool, audit_log: AuditLog, tenant_quotas: TenantQuotaManager) -> Self {
        Self::new_with_services_and_auth(
            pool,
            audit_log,
            RequestSummaryStore::default(),
            tenant_quotas,
            RuntimeRegistry::default(),
            ApiAuthConfig::disabled(),
        )
    }

    pub fn new_with_runtime_registry(
        pool: SandboxPool,
        audit_log: AuditLog,
        tenant_quotas: TenantQuotaManager,
        runtime_registry: RuntimeRegistry,
    ) -> Self {
        Self::new_with_services_and_auth(
            pool,
            audit_log,
            RequestSummaryStore::default(),
            tenant_quotas,
            runtime_registry,
            ApiAuthConfig::disabled(),
        )
    }

    pub fn new_with_auth(
        pool: SandboxPool,
        audit_log: AuditLog,
        tenant_quotas: TenantQuotaManager,
        runtime_registry: RuntimeRegistry,
        auth: ApiAuthConfig,
    ) -> Self {
        Self::new_with_services_and_auth(
            pool,
            audit_log,
            RequestSummaryStore::default(),
            tenant_quotas,
            runtime_registry,
            auth,
        )
    }

    pub fn new_with_services_and_auth(
        pool: SandboxPool,
        audit_log: AuditLog,
        request_summaries: RequestSummaryStore,
        tenant_quotas: TenantQuotaManager,
        runtime_registry: RuntimeRegistry,
        auth: ApiAuthConfig,
    ) -> Self {
        let scheduler = ExecutionScheduler::new(ExecutionSchedulerConfig::new(
            pool.metrics().configured_size,
            tenant_quotas.config().max_queue_depth,
            tenant_quotas.config().max_queue_timeout_ms,
            tenant_quotas.config().max_concurrent_executions_per_tenant,
        ));
        Self {
            pool,
            audit_log,
            request_summaries,
            tenant_quotas,
            scheduler,
            execution_metrics: ExecutionMetrics::default(),
            runtime_registry,
            auth,
            started_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        }
    }

    pub fn pool(&self) -> &SandboxPool {
        &self.pool
    }

    pub fn audit_log(&self) -> &AuditLog {
        &self.audit_log
    }

    pub fn tenant_quotas(&self) -> &TenantQuotaManager {
        &self.tenant_quotas
    }

    pub fn request_summaries(&self) -> &RequestSummaryStore {
        &self.request_summaries
    }

    pub fn scheduler(&self) -> &ExecutionScheduler {
        &self.scheduler
    }

    pub fn execution_metrics(&self) -> &ExecutionMetrics {
        &self.execution_metrics
    }

    pub fn runtime_registry(&self) -> &RuntimeRegistry {
        &self.runtime_registry
    }

    pub fn auth(&self) -> &ApiAuthConfig {
        &self.auth
    }

    pub fn started_at_ms(&self) -> u64 {
        self.started_at_ms
    }
}

pub fn default_state() -> AppState {
    let config = SynapseConfig::from_providers(&SystemProviders);
    let runtime_registry = RuntimeRegistry::default();
    AppState::new_with_services_and_auth(
        SandboxPool::new_with_runtime_registry(config.pool_size, runtime_registry.clone()),
        AuditLog::from_providers(&SystemProviders),
        RequestSummaryStore::from_providers(&SystemProviders),
        TenantQuotaManager::new(TenantQuotaConfig {
            max_concurrent_executions_per_tenant: config.tenant_max_concurrency,
            max_requests_per_minute: config.tenant_max_requests_per_minute,
            max_timeout_ms: config.tenant_max_timeout_ms,
            max_cpu_time_limit_ms: config.tenant_max_cpu_time_limit_ms,
            max_memory_limit_mb: config.tenant_max_memory_limit_mb,
            max_queue_depth: config.max_queue_depth,
            max_queue_timeout_ms: config.max_queue_timeout_ms,
        }),
        runtime_registry,
        ApiAuthConfig::from_providers(&SystemProviders),
    )
}

#[derive(Clone, Debug, Default)]
pub struct ApiAuthConfig {
    tokens: Arc<Vec<ApiToken>>,
    load_error: Option<Arc<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthPrincipal {
    allowed_tenants: BTreeSet<String>,
}

impl AuthPrincipal {
    pub fn allows_all_tenants(&self) -> bool {
        self.allowed_tenants.contains("*")
    }

    pub fn allows_tenant(&self, tenant_id: &str) -> bool {
        self.allows_all_tenants() || self.allowed_tenants.contains(tenant_id)
    }

    pub fn allowed_tenants(&self) -> Vec<String> {
        self.allowed_tenants.iter().cloned().collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ApiToken {
    secret: String,
    allowed_tenants: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
struct ApiTokenDocument {
    token: String,
    #[serde(default)]
    tenants: Vec<String>,
}

impl ApiAuthConfig {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn from_providers(providers: &dyn Providers) -> Self {
        let Some(raw) = providers.env_var(API_TOKENS_ENV) else {
            return Self::default();
        };

        match serde_json::from_str(&raw) {
            Ok(parsed) => Self::from_documents(parsed),
            Err(error) => Self {
                tokens: Arc::new(Vec::new()),
                load_error: Some(Arc::new(format!(
                    "failed to parse {API_TOKENS_ENV}: {error}"
                ))),
            },
        }
    }

    pub fn from_static_tokens(tokens: &[(&str, &[&str])]) -> Self {
        Self::from_documents(
            tokens
                .iter()
                .map(|(token, tenants)| ApiTokenDocument {
                    token: (*token).to_string(),
                    tenants: tenants.iter().map(|tenant| (*tenant).to_string()).collect(),
                })
                .collect(),
        )
    }

    fn from_documents(parsed: Vec<ApiTokenDocument>) -> Self {
        let tokens = parsed
            .into_iter()
            .filter_map(|token| {
                let secret = token.token.trim().to_string();
                if secret.is_empty() {
                    return None;
                }

                let allowed_tenants = token
                    .tenants
                    .into_iter()
                    .map(|tenant| tenant.trim().to_string())
                    .filter(|tenant| !tenant.is_empty())
                    .collect::<BTreeSet<_>>();

                Some(ApiToken {
                    secret,
                    allowed_tenants,
                })
            })
            .collect();

        Self {
            tokens: Arc::new(tokens),
            load_error: None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        !self.tokens.is_empty()
    }

    pub fn authenticate_bearer(
        &self,
        authorization: Option<&str>,
    ) -> Result<AuthPrincipal, SynapseError> {
        if let Some(error) = &self.load_error {
            return Err(SynapseError::Internal((**error).clone()));
        }

        if !self.is_enabled() {
            return Ok(AuthPrincipal {
                allowed_tenants: BTreeSet::from(["*".to_string()]),
            });
        }

        let header = authorization.ok_or_else(|| {
            SynapseError::AuthRequired("missing Authorization: Bearer token".to_string())
        })?;
        let token = header
            .strip_prefix("Bearer ")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                SynapseError::AuthInvalid(
                    "invalid Authorization header; expected Bearer token".to_string(),
                )
            })?;

        let configured = self
            .tokens
            .iter()
            .find(|candidate| candidate.secret == token)
            .ok_or_else(|| SynapseError::AuthInvalid("invalid bearer token".to_string()))?;

        Ok(AuthPrincipal {
            allowed_tenants: configured.allowed_tenants.clone(),
        })
    }

    pub fn authorize_tenant(
        &self,
        principal: &AuthPrincipal,
        tenant_id: &str,
    ) -> Result<(), SynapseError> {
        if let Some(error) = &self.load_error {
            return Err(SynapseError::Internal((**error).clone()));
        }

        if !self.is_enabled()
            || principal.allowed_tenants.contains("*")
            || principal.allowed_tenants.contains(tenant_id)
        {
            return Ok(());
        }

        Err(SynapseError::TenantForbidden(format!(
            "token is not allowed to access tenant {tenant_id}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::ApiAuthConfig;
    use std::{collections::HashMap, ffi::OsString, path::PathBuf};
    use synapse_core::{Providers, SynapseError};

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
            PathBuf::from("/tmp")
        }

        fn process_id(&self) -> u32 {
            1
        }

        fn now_unix_nanos(&self) -> u128 {
            1
        }
    }

    #[test]
    fn invalid_token_config_fails_closed() {
        let mut providers = FakeProviders::default();
        providers
            .env
            .insert("SYNAPSE_API_TOKENS".to_string(), "{bad json".to_string());

        let config = ApiAuthConfig::from_providers(&providers);
        let error = config.authenticate_bearer(None).unwrap_err();
        assert!(
            matches!(error, SynapseError::Internal(message) if message.contains("failed to parse SYNAPSE_API_TOKENS"))
        );
    }
}
