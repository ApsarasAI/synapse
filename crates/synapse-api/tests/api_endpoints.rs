use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    env, fs,
    net::SocketAddr,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use synapse_api::{
    app::ApiAuthConfig,
    server::{router, router_with_state, AppState},
};
use synapse_core::{
    find_command, AuditLog, RuntimeRegistry, SandboxPool, SystemProviders, TenantQuotaConfig,
    TenantQuotaManager,
};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tower::util::ServiceExt;
use url::Url;

#[tokio::test]
async fn health_returns_ok() {
    let response = router()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body, "ok");
}

#[tokio::test]
async fn execute_returns_python_output() {
    if !python3_available().await {
        return;
    }

    let Some(app) = runtime_test_app(1) else {
        return;
    };
    let response = app
        .oneshot(json_request(
            "/execute",
            json!({
                "language": "python",
                "code": "print('hello from api')\n",
                "timeout_ms": 5_000,
                "memory_limit_mb": 128
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["stdout"], "hello from api\n");
    assert_eq!(body["stderr"], "");
    assert_eq!(body["exit_code"], 0);
}

#[tokio::test]
async fn execute_rejects_invalid_input() {
    let app = AppState::new_with_auth(
        SandboxPool::new(1),
        AuditLog::default(),
        TenantQuotaManager::default(),
        RuntimeRegistry::default(),
        ApiAuthConfig::disabled(),
    );
    let response = router_with_state(app)
        .oneshot(json_request(
            "/execute",
            json!({
                "language": "python",
                "code": "   ",
                "timeout_ms": 5_000,
                "memory_limit_mb": 128
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["exit_code"], -1);
    assert_eq!(body["stderr"], "invalid input: code cannot be empty");
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn execute_rejects_unsupported_network_allow_list_policy() {
    let app = AppState::new_with_auth(
        SandboxPool::new(1),
        AuditLog::default(),
        TenantQuotaManager::default(),
        RuntimeRegistry::default(),
        ApiAuthConfig::disabled(),
    );
    let response = router_with_state(app)
        .oneshot(json_request(
            "/execute",
            json!({
                "language": "python",
                "code": "print('blocked')\n",
                "network_policy": {
                    "mode": "allow_list",
                    "hosts": ["example.com:443"]
                }
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "sandbox_policy_blocked");
}

#[tokio::test]
async fn execute_times_out_through_http() {
    if !python3_available().await {
        return;
    }

    let Some(app) = runtime_test_app(1) else {
        return;
    };
    let response = app
        .oneshot(json_request(
            "/execute",
            json!({
                "language": "python",
                "code": "while True:\n    pass\n",
                "timeout_ms": 50,
                "memory_limit_mb": 128
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["exit_code"], -1);
    assert!(body["stderr"]
        .as_str()
        .unwrap_or_default()
        .contains("execution timed out"));
    assert_eq!(body["error"]["code"], "wall_timeout");
}

#[tokio::test]
async fn execute_reports_output_truncation_metadata() {
    if !python3_available().await {
        return;
    }

    let Some(app) = runtime_test_app(1) else {
        return;
    };
    let response = app
        .oneshot(json_request(
            "/execute",
            json!({
                "language": "python",
                "code": "print('a' * (1024 * 1024 + 256))\n",
                "timeout_ms": 5_000,
                "memory_limit_mb": 256
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["exit_code"], 0);
    assert_eq!(body["output"]["stdout_truncated"], true);
    assert_eq!(body["output"]["stderr_truncated"], false);
    assert!(body["stdout"]
        .as_str()
        .unwrap_or_default()
        .ends_with("[output truncated]"));
}

#[tokio::test]
async fn metrics_reflect_execute_requests() {
    if !python3_available().await {
        return;
    }

    let Some(state) = runtime_test_state(2, TenantQuotaManager::default()) else {
        return;
    };
    let app = router_with_state(state);

    let response = app
        .clone()
        .oneshot(json_request(
            "/execute",
            json!({
                "language": "python",
                "code": "print('metrics')\n",
                "timeout_ms": 5_000,
                "memory_limit_mb": 128
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("synapse_pool_configured_size 2"));
    assert!(text.contains("synapse_execute_requests_total 1"));
    assert!(text.contains("synapse_execute_completed_total 1"));
    assert!(text.contains("synapse_tenant_max_concurrency"));
}

#[tokio::test]
async fn metrics_track_invalid_input_failures() {
    let state = AppState::new(
        SandboxPool::new(1),
        AuditLog::default(),
        TenantQuotaManager::default(),
    );
    let app = router_with_state(state);

    let response = app
        .clone()
        .oneshot(json_request(
            "/execute",
            json!({
                "language": "python",
                "code": " ",
                "timeout_ms": 5_000,
                "memory_limit_mb": 128
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("synapse_execute_error_total 1"));
    assert!(text.contains("synapse_execute_invalid_input_total 1"));
}

#[tokio::test]
async fn metrics_track_audit_persist_failures() {
    if !python3_available().await {
        return;
    }

    let Some(state) = runtime_test_state(1, TenantQuotaManager::default()) else {
        return;
    };
    let app = router_with_state(state);
    let request_id = unique_request_id("duplicate-audit");

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/execute")
                    .header("content-type", "application/json")
                    .header("x-synapse-request-id", &request_id)
                    .body(Body::from(
                        json!({
                            "language": "python",
                            "code": "print('audit metric')\n",
                            "timeout_ms": 5_000,
                            "memory_limit_mb": 128
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("synapse_execute_audit_failed_total 1"));
}

#[tokio::test]
async fn metrics_track_timeout_and_truncation_dimensions() {
    if !python3_available().await {
        return;
    }

    let Some(state) = runtime_test_state(1, TenantQuotaManager::default()) else {
        return;
    };
    let app = router_with_state(state);

    let timeout_response = app
        .clone()
        .oneshot(json_request(
            "/execute",
            json!({
                "language": "python",
                "code": "while True:\n    pass\n",
                "timeout_ms": 50,
                "memory_limit_mb": 128
            }),
        ))
        .await
        .unwrap();
    assert_eq!(timeout_response.status(), StatusCode::OK);

    let truncation_response = app
        .clone()
        .oneshot(json_request(
            "/execute",
            json!({
                "language": "python",
                "code": "print('a' * (1024 * 1024 + 256))\n",
                "timeout_ms": 5_000,
                "memory_limit_mb": 256
            }),
        ))
        .await
        .unwrap();
    assert_eq!(truncation_response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("synapse_execute_success_total 1"));
    assert!(text.contains("synapse_execute_error_total 1"));
    assert!(text.contains("synapse_execute_wall_timeout_total 1"));
    assert!(text.contains("synapse_execute_lifecycle_admitted_total 2"));
    assert!(text.contains("synapse_execute_lifecycle_completed_total 2"));
    assert!(text.contains("synapse_execute_lifecycle_cleanup_done_total 2"));
    assert!(text.contains("synapse_execute_stdout_truncated_total 1"));
}

#[tokio::test]
async fn execute_reports_queue_timeout_when_capacity_is_held() {
    if !python3_available().await {
        return;
    }

    let Some(state) = runtime_test_state(
        1,
        TenantQuotaManager::new(TenantQuotaConfig {
            max_concurrent_executions_per_tenant: 1,
            max_requests_per_minute: 120,
            max_timeout_ms: 30_000,
            max_cpu_time_limit_ms: 30_000,
            max_memory_limit_mb: 512,
            max_queue_depth: 2,
            max_queue_timeout_ms: 50,
        }),
    ) else {
        return;
    };
    let app = router_with_state(state);

    let active = tokio::spawn({
        let app = app.clone();
        async move {
            app.oneshot(json_request(
                "/execute",
                json!({
                    "language": "python",
                    "tenant_id": "tenant-a",
                    "code": "import time\ntime.sleep(0.2)\nprint('held')\n",
                    "timeout_ms": 5_000,
                    "memory_limit_mb": 128
                }),
            ))
            .await
            .unwrap()
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let response = app
        .clone()
        .oneshot(json_request(
            "/execute",
            json!({
                "language": "python",
                "tenant_id": "tenant-b",
                "code": "print('queued')\n",
                "timeout_ms": 5_000,
                "memory_limit_mb": 128
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "queue_timeout");

    assert_eq!(active.await.unwrap().status(), StatusCode::OK);
}

#[tokio::test]
async fn execute_reports_capacity_rejected_when_queue_is_full() {
    if !python3_available().await {
        return;
    }

    let Some(state) = runtime_test_state(
        1,
        TenantQuotaManager::new(TenantQuotaConfig {
            max_concurrent_executions_per_tenant: 1,
            max_requests_per_minute: 120,
            max_timeout_ms: 30_000,
            max_cpu_time_limit_ms: 30_000,
            max_memory_limit_mb: 512,
            max_queue_depth: 1,
            max_queue_timeout_ms: 2_000,
        }),
    ) else {
        return;
    };
    let app = router_with_state(state);

    let active = tokio::spawn({
        let app = app.clone();
        async move {
            app.oneshot(json_request(
                "/execute",
                json!({
                    "language": "python",
                    "tenant_id": "tenant-a",
                    "code": "import time\ntime.sleep(0.2)\nprint('active')\n",
                    "timeout_ms": 5_000,
                    "memory_limit_mb": 128
                }),
            ))
            .await
            .unwrap()
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let queued = tokio::spawn({
        let app = app.clone();
        async move {
            app.oneshot(json_request(
                "/execute",
                json!({
                    "language": "python",
                    "tenant_id": "tenant-b",
                    "code": "print('queued')\n",
                    "timeout_ms": 5_000,
                    "memory_limit_mb": 128
                }),
            ))
            .await
            .unwrap()
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let rejected = app
        .clone()
        .oneshot(json_request(
            "/execute",
            json!({
                "language": "python",
                "tenant_id": "tenant-c",
                "code": "print('rejected')\n",
                "timeout_ms": 5_000,
                "memory_limit_mb": 128
            }),
        ))
        .await
        .unwrap();

    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = json_body(rejected).await;
    assert_eq!(body["error"]["code"], "capacity_rejected");

    assert_eq!(active.await.unwrap().status(), StatusCode::OK);
    assert_eq!(queued.await.unwrap().status(), StatusCode::OK);
}

#[tokio::test]
async fn audits_capture_command_and_completion_details() {
    if !python3_available().await {
        return;
    }

    let Some(state) = runtime_test_state(1, TenantQuotaManager::default()) else {
        return;
    };
    let app = router_with_state(state);

    let request_id = unique_request_id("audit-command-details");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/execute")
                .header("content-type", "application/json")
                .header("x-synapse-request-id", &request_id)
                .body(Body::from(
                    json!({
                        "language": "python",
                        "code": "print('audit')\n",
                        "timeout_ms": 5_000,
                        "memory_limit_mb": 128
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let audit_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/audits/{request_id}"))
                .header("x-synapse-tenant-id", "default")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(audit_response.status(), StatusCode::OK);
    let body = to_bytes(audit_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let events: Value = serde_json::from_slice(&body).unwrap();
    let items = events.as_array().unwrap();

    assert!(items.iter().any(|event| {
        event["kind"] == "command_prepared"
            && event["fields"]["command"] == "python3"
            && event["fields"]["runtime_language"] == "python"
    }));
    assert!(items
        .iter()
        .any(|event| event["fields"]["lifecycle"] == "runtime_resolved"));
    assert!(items.iter().any(|event| {
        event["kind"] == "execution_finished"
            && event["fields"]["exit_code"] == "0"
            && event["fields"]["stdout_truncated"] == "false"
            && event["fields"]["lifecycle"] == "completed"
    }));
    assert!(items.iter().any(|event| {
        event["kind"] == "sandbox_reset" && event["fields"]["lifecycle"] == "cleanup_done"
    }));
}

#[tokio::test]
async fn audits_capture_limit_exceeded_details() {
    if !python3_available().await {
        return;
    }

    let Some(state) = runtime_test_state(1, TenantQuotaManager::default()) else {
        return;
    };
    let app = router_with_state(state);

    let request_id = unique_request_id("audit-limit-details");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/execute")
                .header("content-type", "application/json")
                .header("x-synapse-request-id", &request_id)
                .body(Body::from(
                    json!({
                        "language": "python",
                        "code": "while True:\n    pass\n",
                        "timeout_ms": 50,
                        "memory_limit_mb": 128
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let audit_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/audits/{request_id}"))
                .header("x-synapse-tenant-id", "default")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(audit_response.status(), StatusCode::OK);
    let body = to_bytes(audit_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let events: Value = serde_json::from_slice(&body).unwrap();
    let items = events.as_array().unwrap();

    assert!(items.iter().any(|event| {
        event["kind"] == "limit_exceeded"
            && event["fields"]["error_code"] == "WallTimeout"
            && event["fields"]["wall_time_limit_ms"] == "50"
    }));
}

#[tokio::test]
async fn audits_capture_network_attempt_details() {
    if !python3_available().await {
        return;
    }

    let Some(state) = runtime_test_state(1, TenantQuotaManager::default()) else {
        return;
    };
    let app = router_with_state(state);

    let request_id = unique_request_id("audit-network-details");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/execute")
                .header("content-type", "application/json")
                .header("x-synapse-request-id", &request_id)
                .body(Body::from(
                    json!({
                        "language": "python",
                        "code": "import socket\ntry:\n    socket.create_connection(('1.1.1.1', 80), timeout=1)\nexcept Exception:\n    pass\n",
                        "timeout_ms": 5_000,
                        "memory_limit_mb": 128
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let audit_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/audits/{request_id}"))
                .header("x-synapse-tenant-id", "default")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(audit_response.status(), StatusCode::OK);
    let body = to_bytes(audit_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let events: Value = serde_json::from_slice(&body).unwrap();
    let items = events.as_array().unwrap();

    assert!(items.iter().any(|event| {
        event["kind"] == "network_attempt"
            && event["fields"]["target"]
                .as_str()
                .unwrap_or_default()
                .contains("AF_INET")
    }));
}

#[tokio::test]
async fn audits_capture_process_spawn_attempt_details() {
    if !python3_available().await {
        return;
    }

    let Some(state) = runtime_test_state(1, TenantQuotaManager::default()) else {
        return;
    };
    let app = router_with_state(state);

    let request_id = unique_request_id("audit-process-details");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/execute")
                .header("content-type", "application/json")
                .header("x-synapse-request-id", &request_id)
                .body(Body::from(
                    json!({
                        "language": "python",
                        "code": "import subprocess\ntry:\n    subprocess.run(['/bin/sh', '-c', 'true'], check=False)\nexcept Exception:\n    pass\n",
                        "timeout_ms": 5_000,
                        "memory_limit_mb": 128
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let audit_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/audits/{request_id}"))
                .header("x-synapse-tenant-id", "default")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(audit_response.status(), StatusCode::OK);
    let body = to_bytes(audit_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let events: Value = serde_json::from_slice(&body).unwrap();
    let items = events.as_array().unwrap();

    assert!(items
        .iter()
        .any(|event| { event["kind"] == "process_spawn" }));
}

#[tokio::test]
async fn stream_websocket_emits_lifecycle_events_and_closes() {
    if !python3_available().await {
        return;
    }

    let Some(state) = runtime_test_state(1, TenantQuotaManager::default()) else {
        return;
    };
    let (addr, _server) = spawn_test_server(router_with_state(state)).await;
    let events = connect_and_collect_stream_events(
        addr,
        json!({
            "language": "python",
            "code": "print('stream ok')\n",
            "timeout_ms": 5_000,
            "memory_limit_mb": 128,
            "tenant_id": "tenant-stream"
        }),
    )
    .await;

    assert_eq!(events[0]["event"], "started");
    assert_eq!(events[0]["fields"]["tenant_id"], "tenant-stream");
    assert!(events[0]["fields"]["request_id"]
        .as_str()
        .unwrap()
        .starts_with("synapse-request-"));
    assert!(events
        .iter()
        .any(|event| { event["event"] == "stdout" && event["fields"]["data"] == "stream ok\n" }));
    assert!(events.iter().any(|event| {
        event["event"] == "completed"
            && event["fields"]["exit_code"] == "0"
            && event["fields"]["stdout_truncated"] == "false"
            && event["fields"]["tenant_id"] == "tenant-stream"
    }));
}

#[tokio::test]
async fn stream_websocket_reports_timeout_errors() {
    if !python3_available().await {
        return;
    }

    let Some(state) = runtime_test_state(1, TenantQuotaManager::default()) else {
        return;
    };
    let (addr, _server) = spawn_test_server(router_with_state(state)).await;
    let events = connect_and_collect_stream_events(
        addr,
        json!({
            "language": "python",
            "code": "while True:\n    pass\n",
            "timeout_ms": 50,
            "memory_limit_mb": 128
        }),
    )
    .await;

    assert!(events.iter().any(|event| {
        event["event"] == "stderr"
            && event["fields"]["data"]
                .as_str()
                .unwrap_or_default()
                .contains("execution timed out")
    }));
    assert!(events.iter().any(|event| {
        event["event"] == "completed"
            && event["fields"]["error_code"] == "WallTimeout"
            && event["fields"]["exit_code"] == "-1"
    }));
}

#[tokio::test]
async fn audit_lookup_returns_not_found_for_missing_record() {
    let app = AppState::new_with_auth(
        SandboxPool::new(1),
        AuditLog::default(),
        TenantQuotaManager::default(),
        RuntimeRegistry::default(),
        ApiAuthConfig::disabled(),
    );
    let response = router_with_state(app)
        .oneshot(
            Request::builder()
                .uri("/audits/missing_record")
                .header("x-synapse-tenant-id", "default")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn audit_lookup_rejects_invalid_request_id() {
    let app = AppState::new_with_auth(
        SandboxPool::new(1),
        AuditLog::default(),
        TenantQuotaManager::default(),
        RuntimeRegistry::default(),
        ApiAuthConfig::disabled(),
    );
    let response = router_with_state(app)
        .oneshot(
            Request::builder()
                .uri("/audits/bad$request")
                .header("x-synapse-tenant-id", "default")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn protected_routes_require_authorization_token() {
    let app = router_with_state(auth_test_state(1).unwrap());
    let response = app
        .oneshot(json_request(
            "/execute",
            json!({
                "language": "python",
                "code": "print('auth')\n",
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "auth_required");
}

#[tokio::test]
async fn protected_routes_reject_invalid_token() {
    let app = router_with_state(auth_test_state(1).unwrap());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/execute")
                .header("authorization", "Bearer wrong-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "language": "python",
                        "code": "print('auth')\n",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "auth_invalid");
}

#[tokio::test]
async fn execute_rejects_tenant_mismatch_for_authorized_token() {
    let app = router_with_state(auth_test_state(1).unwrap());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/execute")
                .header("authorization", "Bearer tenant-a-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "language": "python",
                        "code": "print('auth')\n",
                        "tenant_id": "tenant-b",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "tenant_forbidden");
}

#[tokio::test]
async fn audit_lookup_requires_matching_tenant() {
    if !python3_available().await {
        return;
    }

    let Some(state) = runtime_test_state(1, TenantQuotaManager::default()) else {
        return;
    };
    let app = router_with_state(state);
    let request_id = unique_request_id("tenant-audit");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/execute")
                .header("content-type", "application/json")
                .header("x-synapse-request-id", &request_id)
                .header("x-synapse-tenant-id", "tenant-a")
                .body(Body::from(
                    json!({
                        "language": "python",
                        "code": "print('tenant audit')\n",
                        "timeout_ms": 5_000,
                        "memory_limit_mb": 128
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let forbidden = app
        .oneshot(
            Request::builder()
                .uri(format!("/audits/{request_id}"))
                .header("x-synapse-tenant-id", "tenant-b")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn audit_lookup_rejects_cross_tenant_token_access() {
    if !python3_available().await {
        return;
    }

    let Some(state) = auth_test_state(1) else {
        return;
    };
    let app = router_with_state(state);
    let request_id = unique_request_id("tenant-audit-auth");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/execute")
                .header("authorization", "Bearer tenant-a-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "language": "python",
                        "code": "print('tenant audit auth')\n",
                        "tenant_id": "tenant-a",
                        "request_id": request_id,
                        "timeout_ms": 5_000,
                        "memory_limit_mb": 128
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let forbidden = app
        .oneshot(
            Request::builder()
                .uri(format!("/audits/{request_id}"))
                .header("authorization", "Bearer tenant-a-token")
                .header("x-synapse-tenant-id", "tenant-b")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    let body = json_body(forbidden).await;
    assert_eq!(body["error"]["code"], "tenant_forbidden");
}

fn json_request(uri: &str, payload: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
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

fn runtime_test_app(pool_size: usize) -> Option<axum::Router> {
    runtime_test_state(pool_size, TenantQuotaManager::default()).map(router_with_state)
}

fn runtime_test_state(pool_size: usize, quotas: TenantQuotaManager) -> Option<AppState> {
    let registry = provision_python_runtime_registry()?;
    Some(AppState::new_with_runtime_registry(
        SandboxPool::new_with_runtime_registry(pool_size, registry.clone()),
        AuditLog::default(),
        quotas,
        registry,
    ))
}

fn auth_test_state(pool_size: usize) -> Option<AppState> {
    let registry = provision_python_runtime_registry()?;
    Some(AppState::new_with_auth(
        SandboxPool::new_with_runtime_registry(pool_size, registry.clone()),
        AuditLog::default(),
        TenantQuotaManager::default(),
        registry,
        ApiAuthConfig::from_static_tokens(&[
            ("tenant-a-token", &["tenant-a"]),
            ("ops-token", &["*"]),
        ]),
    ))
}

fn provision_python_runtime_registry() -> Option<RuntimeRegistry> {
    let python = find_command(&SystemProviders, "python3")?;
    let root = unique_runtime_root("synapse-api-runtime");
    let registry = RuntimeRegistry::from_root(&root);
    registry.install("python", "system", &python).ok()?;
    registry.activate("python", "system").ok()?;
    Some(registry)
}

fn unique_runtime_root(prefix: &str) -> PathBuf {
    let path = env::temp_dir().join(format!(
        "{prefix}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&path);
    path
}

async fn spawn_test_server(app: axum::Router) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, handle)
}

async fn connect_and_collect_stream_events(addr: SocketAddr, payload: Value) -> Vec<Value> {
    let url = Url::parse(&format!("ws://{addr}/execute/stream")).unwrap();
    let (mut socket, _) = connect_async(url.as_str()).await.unwrap();
    socket
        .send(Message::Text(payload.to_string()))
        .await
        .unwrap();
    let mut events = Vec::new();

    while let Some(message) = socket.next().await {
        match message.unwrap() {
            Message::Text(text) => events.push(serde_json::from_str(&text).unwrap()),
            Message::Close(_) => break,
            _ => {}
        }
    }

    events
}

fn unique_request_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}-{nanos}")
}
