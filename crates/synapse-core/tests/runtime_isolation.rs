use synapse_core::{
    DefaultSandboxEngine, ExecuteRequest, NetworkPolicy, RuntimeRegistry, SandboxEngine,
    SandboxExecution,
};

fn request(code: &str) -> ExecuteRequest {
    ExecuteRequest {
        language: "python".to_string(),
        code: code.to_string(),
        timeout_ms: 5_000,
        cpu_time_limit_ms: Some(5_000),
        memory_limit_mb: 128,
        runtime_version: None,
        tenant_id: Some("tenant-isolation".to_string()),
        request_id: None,
        network_policy: NetworkPolicy::Disabled,
    }
}

async fn python3_available() -> bool {
    tokio::process::Command::new("python3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .is_ok()
}

#[tokio::test]
async fn sandbox_reset_clears_overlay_state_between_runs() {
    if !python3_available().await {
        return;
    }

    let registry = RuntimeRegistry::default();
    let Ok(runtime) = registry.resolve("python", None) else {
        return;
    };
    let engine = DefaultSandboxEngine;
    let sandbox = engine.prepare().await.unwrap();
    let first_request = request(
        "from pathlib import Path\nPath('/workspace/state.txt').write_text('persisted')\nprint(Path('/workspace/state.txt').read_text())\n",
    );
    let first = sandbox
        .execute(SandboxExecution {
            runtime: &runtime,
            code: &first_request.code,
            wall_timeout_ms: first_request.timeout_ms,
            cpu_time_limit_ms: first_request.effective_cpu_time_limit_ms(),
            memory_limit_mb: first_request.memory_limit_mb,
            network_policy: &first_request.network_policy,
        })
        .await
        .unwrap();
    assert_eq!(first.stdout, "persisted\n");

    sandbox.reset().await.unwrap();
    let second_request =
        request("from pathlib import Path\nprint(Path('/workspace/state.txt').exists())\n");
    let second = sandbox
        .execute(SandboxExecution {
            runtime: &runtime,
            code: &second_request.code,
            wall_timeout_ms: second_request.timeout_ms,
            cpu_time_limit_ms: second_request.effective_cpu_time_limit_ms(),
            memory_limit_mb: second_request.memory_limit_mb,
            network_policy: &second_request.network_policy,
        })
        .await
        .unwrap();
    assert_eq!(second.stdout, "False\n");

    sandbox.destroy_blocking().unwrap();
}
