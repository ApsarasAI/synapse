use std::path::{Path, PathBuf};

use crate::{
    find_command, runtime, ExecuteRequest, ExecuteResponse, SynapseError, SystemProviders,
};

pub async fn execute(request: ExecuteRequest) -> Result<ExecuteResponse, SynapseError> {
    validate_request(&request)?;

    let sandbox = runtime::prepare_sandbox().await?;
    let result = execute_in_prepared(&sandbox, request).await;
    let _ = sandbox.destroy_blocking();
    result
}

pub async fn execute_in_prepared(
    sandbox: &runtime::PreparedSandbox,
    request: ExecuteRequest,
) -> Result<ExecuteResponse, SynapseError> {
    validate_request(&request)?;
    sandbox.reset().await?;

    let result = execute_in_sandbox(sandbox.path(), &request).await;
    match sandbox.reset().await {
        Ok(()) => result,
        Err(error) => Err(error),
    }
}

async fn execute_in_sandbox(
    sandbox_dir: &Path,
    request: &ExecuteRequest,
) -> Result<ExecuteResponse, SynapseError> {
    let binary = resolve_language(&request.language)?;
    runtime::execute_binary(
        &binary,
        &request.code,
        sandbox_dir,
        request.timeout_ms,
        request.memory_limit_mb,
    )
    .await
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

    if request.memory_limit_mb == 0 {
        return Err(SynapseError::InvalidInput(
            "memory_limit_mb must be greater than 0".to_string(),
        ));
    }

    Ok(())
}

fn resolve_language(language: &str) -> Result<PathBuf, SynapseError> {
    match language.trim().to_ascii_lowercase().as_str() {
        "python" | "python3" => resolve_binary("python3"),
        other => Err(SynapseError::UnsupportedLanguage(other.to_string())),
    }
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
    use super::{resolve_language, validate_request};
    use crate::{ExecuteRequest, SynapseError};

    fn request() -> ExecuteRequest {
        ExecuteRequest {
            language: "python".to_string(),
            code: "print('ok')\n".to_string(),
            timeout_ms: 5_000,
            memory_limit_mb: 128,
        }
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
    fn validate_request_rejects_zero_memory_limit() {
        let mut request = request();
        request.memory_limit_mb = 0;

        let error = validate_request(&request).unwrap_err();
        assert!(
            matches!(error, SynapseError::InvalidInput(message) if message == "memory_limit_mb must be greater than 0")
        );
    }

    #[test]
    fn resolve_language_accepts_python_aliases_case_insensitively() {
        let python = resolve_language("python").unwrap();
        let python3 = resolve_language("  PyThOn3  ").unwrap();

        assert_eq!(python, python3);
    }

    #[test]
    fn resolve_language_rejects_unknown_language() {
        let error = resolve_language("ruby").unwrap_err();
        assert!(matches!(error, SynapseError::UnsupportedLanguage(language) if language == "ruby"));
    }
}
