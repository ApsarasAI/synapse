use tracing::{info, instrument};

use crate::{
    new_request_id,
    sandbox::{DefaultSandboxEngine, SandboxEngine, SandboxExecution, SandboxInstance},
    ExecuteRequest, ExecuteResponse, LimitSummary, NetworkPolicy, RuntimeRegistry, SynapseError,
    SystemProviders,
};

#[instrument(skip(request), fields(language = %request.language, tenant_id = request.tenant_id.as_deref().unwrap_or("default")))]
pub async fn execute(request: ExecuteRequest) -> Result<ExecuteResponse, SynapseError> {
    let registry = RuntimeRegistry::default();
    execute_with_registry(&registry, request).await
}

pub async fn execute_with_registry(
    registry: &RuntimeRegistry,
    request: ExecuteRequest,
) -> Result<ExecuteResponse, SynapseError> {
    let engine = DefaultSandboxEngine;
    execute_with_engine_and_registry(&engine, registry, request).await
}

pub async fn execute_with_engine_and_registry(
    engine: &dyn SandboxEngine,
    registry: &RuntimeRegistry,
    mut request: ExecuteRequest,
) -> Result<ExecuteResponse, SynapseError> {
    validate_request(&request)?;
    validate_request_against_engine(engine, &request)?;
    let request_id = request
        .request_id
        .clone()
        .unwrap_or_else(|| new_request_id(&SystemProviders));
    request.request_id = Some(request_id.clone());

    let runtime = resolve_runtime(registry, &request)?;
    let result = execute_with_runtime(engine, &request, &runtime).await;
    info!(request_id, "execution finished in disposable sandbox");
    result
}

pub(crate) async fn execute_in_instance_with_registry(
    sandbox: &dyn SandboxInstance,
    engine: &dyn SandboxEngine,
    registry: &RuntimeRegistry,
    mut request: ExecuteRequest,
) -> Result<ExecuteResponse, SynapseError> {
    validate_request(&request)?;
    validate_request_against_engine(engine, &request)?;
    let request_id = request
        .request_id
        .clone()
        .unwrap_or_else(|| new_request_id(&SystemProviders));
    request.request_id = Some(request_id.clone());
    sandbox.reset().await?;

    let runtime = resolve_runtime(registry, &request)?;
    let result = execute_on_instance(sandbox, &request, &runtime).await;
    match sandbox.reset().await {
        Ok(()) => result,
        Err(error) => Err(error),
    }
}

async fn execute_with_runtime(
    engine: &dyn SandboxEngine,
    request: &ExecuteRequest,
    runtime: &crate::ResolvedRuntime,
) -> Result<ExecuteResponse, SynapseError> {
    let execution = sandbox_execution(request, runtime);
    let response = engine.execute_disposable(execution).await?;
    attach_request_metadata(response, request, runtime)
}

async fn execute_on_instance(
    sandbox: &dyn SandboxInstance,
    request: &ExecuteRequest,
    runtime: &crate::ResolvedRuntime,
) -> Result<ExecuteResponse, SynapseError> {
    let execution = sandbox_execution(request, runtime);
    let response = sandbox.execute(execution).await?;
    attach_request_metadata(response, request, runtime)
}

fn sandbox_execution<'a>(
    request: &'a ExecuteRequest,
    runtime: &'a crate::ResolvedRuntime,
) -> SandboxExecution<'a> {
    SandboxExecution {
        runtime,
        code: &request.code,
        wall_timeout_ms: request.timeout_ms,
        cpu_time_limit_ms: request.effective_cpu_time_limit_ms(),
        memory_limit_mb: request.memory_limit_mb,
        network_policy: &request.network_policy,
    }
}

fn attach_request_metadata(
    mut response: ExecuteResponse,
    request: &ExecuteRequest,
    runtime: &crate::ResolvedRuntime,
) -> Result<ExecuteResponse, SynapseError> {
    for event in &mut response.sandbox_audit {
        event.request_id = request
            .request_id
            .clone()
            .unwrap_or_else(|| new_request_id(&SystemProviders));
        event.tenant_id = request.tenant_id.clone();
    }

    Ok(response.with_request_metadata(
        request
            .request_id
            .clone()
            .unwrap_or_else(|| new_request_id(&SystemProviders)),
        request.tenant_id.as_deref(),
        Some(runtime.info().clone()),
        LimitSummary {
            wall_time_limit_ms: request.timeout_ms,
            cpu_time_limit_ms: request.effective_cpu_time_limit_ms(),
            memory_limit_mb: request.memory_limit_mb,
        },
    ))
}

fn validate_request_against_engine(
    engine: &dyn SandboxEngine,
    request: &ExecuteRequest,
) -> Result<(), SynapseError> {
    let capabilities = engine.capabilities();
    match &request.network_policy {
        NetworkPolicy::Disabled if capabilities.network_disabled => {}
        NetworkPolicy::Disabled => {
            return Err(SynapseError::SandboxPolicy(format!(
                "network disabled mode is not supported by the {} sandbox backend",
                engine.name()
            )));
        }
        NetworkPolicy::AllowList { .. } if capabilities.network_allow_list => {}
        NetworkPolicy::AllowList { .. } => {
            return Err(SynapseError::SandboxPolicy(format!(
                "network allow_list mode is not supported by the {} sandbox backend",
                engine.name()
            )));
        }
    }

    if request.effective_cpu_time_limit_ms() < request.timeout_ms && !capabilities.cpu_accounting {
        return Err(SynapseError::RuntimeUnavailable(format!(
            "cpu_time_limit_ms below timeout_ms requires CPU accounting support in the {} sandbox backend",
            engine.name()
        )));
    }

    Ok(())
}

fn validate_request(request: &ExecuteRequest) -> Result<(), SynapseError> {
    if request.code.trim().is_empty() {
        return Err(SynapseError::InvalidInput(
            "code cannot be empty".to_string(),
        ));
    }

    if request.timeout_ms == 0 {
        return Err(SynapseError::InvalidInput(
            "timeout_ms must be greater than 0".to_string(),
        ));
    }

    if request.effective_cpu_time_limit_ms() == 0 {
        return Err(SynapseError::InvalidInput(
            "cpu_time_limit_ms must be greater than 0".to_string(),
        ));
    }

    if request.memory_limit_mb == 0 {
        return Err(SynapseError::InvalidInput(
            "memory_limit_mb must be greater than 0".to_string(),
        ));
    }

    match &request.network_policy {
        NetworkPolicy::Disabled => {}
        NetworkPolicy::AllowList { hosts } if hosts.is_empty() => {
            return Err(SynapseError::InvalidInput(
                "network allow_list policy requires at least one host".to_string(),
            ));
        }
        NetworkPolicy::AllowList { .. } => {}
    }

    Ok(())
}

fn resolve_runtime(
    registry: &RuntimeRegistry,
    request: &ExecuteRequest,
) -> Result<crate::ResolvedRuntime, SynapseError> {
    registry.resolve(&request.language, request.runtime_version.as_deref())
}

#[cfg(test)]
mod tests {
    use super::{resolve_runtime, validate_request, validate_request_against_engine};
    use crate::{
        sandbox::DefaultSandboxEngine, ExecuteRequest, NetworkPolicy, RuntimeRegistry, SynapseError,
    };
    use std::{env, fs, path::PathBuf};

    fn request() -> ExecuteRequest {
        ExecuteRequest {
            language: "python".to_string(),
            code: "print('ok')\n".to_string(),
            timeout_ms: 5_000,
            cpu_time_limit_ms: Some(3_000),
            memory_limit_mb: 128,
            runtime_version: None,
            tenant_id: None,
            request_id: None,
            network_policy: NetworkPolicy::Disabled,
        }
    }

    fn unique_root(prefix: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&path);
        path
    }

    fn fake_runtime_binary(root: &PathBuf, name: &str) -> PathBuf {
        fs::create_dir_all(root).unwrap();
        let path = root.join(name);
        fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).unwrap();
        }
        path
    }

    fn registry_with_active_python() -> RuntimeRegistry {
        let root = unique_root("synapse-service-runtime");
        let registry = RuntimeRegistry::from_root(&root);
        let binary = fake_runtime_binary(&root.join("src"), "python3");
        registry.install("python", "3.12.5", &binary).unwrap();
        registry.activate("python", "3.12.5").unwrap();
        registry
    }

    #[test]
    fn validate_request_rejects_empty_code() {
        let mut request = request();
        request.code = "   ".to_string();

        let error = validate_request(&request).unwrap_err();
        assert!(
            matches!(error, SynapseError::InvalidInput(message) if message == "code cannot be empty")
        );
    }

    #[test]
    fn validate_request_rejects_zero_timeout() {
        let mut request = request();
        request.timeout_ms = 0;

        let error = validate_request(&request).unwrap_err();
        assert!(
            matches!(error, SynapseError::InvalidInput(message) if message == "timeout_ms must be greater than 0")
        );
    }

    #[test]
    fn validate_request_rejects_zero_cpu_budget() {
        let mut request = request();
        request.cpu_time_limit_ms = Some(0);

        let error = validate_request(&request).unwrap_err();
        assert!(
            matches!(error, SynapseError::InvalidInput(message) if message == "cpu_time_limit_ms must be greater than 0")
        );
    }

    #[test]
    fn validate_request_rejects_zero_memory_limit() {
        let mut request = request();
        request.memory_limit_mb = 0;

        let error = validate_request(&request).unwrap_err();
        assert!(
            matches!(error, SynapseError::InvalidInput(message) if message == "memory_limit_mb must be greater than 0")
        );
    }

    #[test]
    fn validate_request_rejects_empty_network_allow_list() {
        let mut request = request();
        request.network_policy = NetworkPolicy::AllowList { hosts: Vec::new() };

        let error = validate_request(&request).unwrap_err();
        assert!(
            matches!(error, SynapseError::InvalidInput(message) if message == "network allow_list policy requires at least one host")
        );
    }

    #[test]
    fn engine_capabilities_reject_unsupported_network_allow_list() {
        let mut request = request();
        request.network_policy = NetworkPolicy::AllowList {
            hosts: vec!["example.com:443".to_string()],
        };

        validate_request(&request).unwrap();
        let error = validate_request_against_engine(&DefaultSandboxEngine, &request).unwrap_err();
        assert!(
            matches!(error, SynapseError::SandboxPolicy(message) if message.contains("allow_list mode"))
        );
    }

    #[test]
    fn resolve_runtime_accepts_python_aliases_case_insensitively() {
        let registry = registry_with_active_python();
        let python = resolve_runtime(&registry, &request()).unwrap();
        let mut alias_request = request();
        alias_request.language = "  PyThOn3  ".to_string();
        let python3 = resolve_runtime(&registry, &alias_request).unwrap();

        assert_eq!(python.info().language, python3.info().language);
        let _ = fs::remove_dir_all(registry.root());
    }

    #[test]
    fn resolve_runtime_rejects_unknown_language() {
        let registry = RuntimeRegistry::default();
        let mut request = request();
        request.language = "ruby".to_string();
        let error = resolve_runtime(&registry, &request).unwrap_err();
        assert!(matches!(error, SynapseError::UnsupportedLanguage(language) if language == "ruby"));
    }
}
