use std::{collections::BTreeSet, fs};

use synapse_core::{execute_in_prepared, prepare_sandbox, ExecuteRequest, NetworkPolicy};

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
async fn prepared_sandbox_reset_clears_overlay_state_between_runs() {
    if !python3_available().await {
        return;
    }

    let sandbox = prepare_sandbox().await.unwrap();
    let first = execute_in_prepared(
        &sandbox,
        request(
            "from pathlib import Path\nPath('/workspace/state.txt').write_text('persisted')\nprint(Path('/workspace/state.txt').read_text())\n",
        ),
    )
    .await
    .unwrap();
    assert_eq!(first.stdout, "persisted\n");

    let second = execute_in_prepared(
        &sandbox,
        request("from pathlib import Path\nprint(Path('/workspace/state.txt').exists())\n"),
    )
    .await
    .unwrap();
    assert_eq!(second.stdout, "False\n");

    let entries: BTreeSet<_> = fs::read_dir(sandbox.root_path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries,
        BTreeSet::from(["upper".to_string(), "work".to_string()])
    );

    sandbox.destroy_blocking().unwrap();
}
