use std::{collections::BTreeMap, io::ErrorKind, net::SocketAddr};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Json, Path, Request, State,
    },
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::IntoResponse,
    routing::{get, post},
    Extension, Router,
};
use synapse_core::{
    audit_event, new_request_id, validate_request_id, AuditEvent, AuditEventKind, ExecuteRequest,
    ExecuteResponse, SynapseError, SystemProviders,
};
use tower_http::trace::TraceLayer;
use tracing::{error, info, instrument};

pub use crate::app::AppState;
use crate::app::{default_state, AuthPrincipal};

const TENANT_HEADER: &str = "x-synapse-tenant-id";
const REQUEST_ID_HEADER: &str = "x-synapse-request-id";

pub fn router() -> Router {
    router_with_state(default_state())
}

pub fn router_with_state(state: AppState) -> Router {
    let protected_layer = middleware::from_fn_with_state(state.clone(), require_bearer_auth);
    Router::new()
        .route("/health", get(health))
        .route(
            "/metrics",
            get(metrics).route_layer(protected_layer.clone()),
        )
        .route(
            "/audits/:request_id",
            get(get_audit_log).route_layer(protected_layer.clone()),
        )
        .route(
            "/execute",
            post(execute_request).route_layer(protected_layer.clone()),
        )
        .route(
            "/execute/stream",
            get(stream_execution_websocket)
                .post(stream_execution_legacy)
                .route_layer(protected_layer),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn serve(listen: SocketAddr) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, router()).await
}

async fn health() -> &'static str {
    "ok"
}

async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    render_metrics(&state)
}

async fn require_bearer_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> impl IntoResponse {
    match state
        .auth()
        .authenticate_bearer(header_value(request.headers(), "authorization"))
    {
        Ok(principal) => {
            request.extensions_mut().insert(principal);
            next.run(request).await
        }
        Err(error) => {
            state.execution_metrics().record_error_code(error.code());
            map_error(error).into_response()
        }
    }
}

async fn get_audit_log(
    State(state): State<AppState>,
    Extension(principal): Extension<AuthPrincipal>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> impl IntoResponse {
    if let Err(error) = validate_request_id(&request_id) {
        return map_error(error).into_response();
    }

    let tenant_id = state
        .tenant_quotas()
        .normalize_tenant_id(header_value(&headers, TENANT_HEADER));
    if let Err(error) = state.auth().authorize_tenant(&principal, &tenant_id) {
        state.execution_metrics().record_error_code(error.code());
        return map_error(error).into_response();
    }
    match state.audit_log().load(&request_id) {
        Ok(events) if audit_visible_to_tenant(&events, &tenant_id) => {
            (StatusCode::OK, Json(events)).into_response()
        }
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(SynapseError::Io(error)) if error.kind() == ErrorKind::NotFound => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(error) => map_error(error).into_response(),
    }
}

#[instrument(skip(state, headers, req), fields(language = %req.language))]
async fn execute_request(
    State(state): State<AppState>,
    Extension(principal): Extension<AuthPrincipal>,
    headers: HeaderMap,
    Json(mut req): Json<ExecuteRequest>,
) -> (StatusCode, Json<ExecuteResponse>) {
    if let Err(error) = hydrate_request(&mut req, &headers, &state, &principal) {
        let status = status_for_error(&error);
        state.execution_metrics().record_error_code(error.code());
        return (
            status,
            Json(ExecuteResponse::error(error.to_execute_error(), 0)),
        );
    }
    let request_id = req
        .request_id
        .clone()
        .unwrap_or_else(|| new_request_id(&SystemProviders));
    let tenant_id = state
        .tenant_quotas()
        .normalize_tenant_id(req.tenant_id.as_deref());
    req.request_id = Some(request_id.clone());
    req.tenant_id = Some(tenant_id.clone());

    let mut audit = vec![audit_event(
        request_id.clone(),
        Some(&tenant_id),
        AuditEventKind::RequestReceived,
        "execution request received",
    )];

    if let Err(error) = state.tenant_quotas().enforce_request_limits(&req) {
        audit.push(audit_with_error(
            &request_id,
            &tenant_id,
            AuditEventKind::QuotaRejected,
            &error,
        ));
        let mut response = ExecuteResponse::error(error.to_execute_error(), 0);
        response.request_id = Some(request_id.clone());
        response.tenant_id = Some(tenant_id.clone());
        response.audit = Some(synapse_core::AuditSummary {
            request_id: request_id.clone(),
            event_count: audit.len(),
        });
        state.execution_metrics().record_error_code(error.code());
        persist_audit(&state, &request_id, &audit);
        return (status_for_error(&error), Json(response));
    }

    if let Err(error) = state.tenant_quotas().enforce_rate_limit(&tenant_id) {
        audit.push(audit_with_error(
            &request_id,
            &tenant_id,
            AuditEventKind::QuotaRejected,
            &error,
        ));
        let mut response = ExecuteResponse::error(error.to_execute_error(), 0);
        response.request_id = Some(request_id.clone());
        response.tenant_id = Some(tenant_id.clone());
        response.audit = Some(synapse_core::AuditSummary {
            request_id: request_id.clone(),
            event_count: audit.len(),
        });
        state.execution_metrics().record_error_code(error.code());
        persist_audit(&state, &request_id, &audit);
        return (status_for_error(&error), Json(response));
    }

    let _permit = match state.scheduler().acquire(&tenant_id).await {
        Ok(permit) => {
            let mut event = audit_event(
                request_id.clone(),
                Some(&tenant_id),
                AuditEventKind::QuotaAccepted,
                "execution capacity admitted",
            );
            event
                .fields
                .insert("queued".to_string(), permit.was_queued().to_string());
            event.fields.insert(
                "queue_wait_ms".to_string(),
                permit.wait_duration_ms().to_string(),
            );
            audit.push(event);
            permit
        }
        Err(error) => {
            audit.push(audit_with_error(
                &request_id,
                &tenant_id,
                AuditEventKind::QuotaRejected,
                &error,
            ));
            let mut response = ExecuteResponse::error(error.to_execute_error(), 0);
            response.request_id = Some(request_id.clone());
            response.tenant_id = Some(tenant_id.clone());
            response.audit = Some(synapse_core::AuditSummary {
                request_id: request_id.clone(),
                event_count: audit.len(),
            });
            state.execution_metrics().record_error_code(error.code());
            persist_audit(&state, &request_id, &audit);
            return (status_for_error(&error), Json(response));
        }
    };

    audit.extend(prepare_execution_audit_events(
        &state,
        &request_id,
        &tenant_id,
        &req,
    ));

    match state.pool().execute(req).await {
        Ok(mut response) => {
            audit.append(&mut response.sandbox_audit);
            audit.extend(finalize_execution_audit_events(
                &request_id,
                &tenant_id,
                &response,
            ));
            response.audit = Some(synapse_core::AuditSummary {
                request_id: request_id.clone(),
                event_count: audit.len(),
            });
            state.execution_metrics().record_response(&response);
            persist_audit(&state, &request_id, &audit);
            log_execution_outcome(&request_id, &tenant_id, &response);
            (StatusCode::OK, Json(response))
        }
        Err(error) => {
            audit.push(audit_with_error(
                &request_id,
                &tenant_id,
                AuditEventKind::PolicyBlocked,
                &error,
            ));
            let mut response = ExecuteResponse::error(error.to_execute_error(), 0);
            response.request_id = Some(request_id.clone());
            response.tenant_id = Some(tenant_id.clone());
            response.audit = Some(synapse_core::AuditSummary {
                request_id: request_id.clone(),
                event_count: audit.len(),
            });
            state.execution_metrics().record_error_code(error.code());
            persist_audit(&state, &request_id, &audit);
            error!(
                request_id = sanitize_log_value(&request_id),
                tenant_id = sanitize_log_value(&tenant_id),
                error_code = ?error.code(),
                "execution request failed"
            );
            (status_for_error(&error), Json(response))
        }
    }
}

async fn stream_execution_websocket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Extension(principal): Extension<AuthPrincipal>,
    headers: HeaderMap,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_stream_websocket(socket, state, principal, headers))
}

async fn stream_execution_legacy(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Extension(principal): Extension<AuthPrincipal>,
    headers: HeaderMap,
    Json(req): Json<ExecuteRequest>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_stream(socket, state, principal, headers, req))
}

async fn handle_stream(
    mut socket: WebSocket,
    state: AppState,
    principal: AuthPrincipal,
    headers: HeaderMap,
    mut req: ExecuteRequest,
) {
    if let Err(error) = hydrate_request(&mut req, &headers, &state, &principal) {
        let _ = send_stream_error(&mut socket, &error).await;
        let _ = socket.close().await;
        return;
    }
    let request_id = req
        .request_id
        .clone()
        .unwrap_or_else(|| new_request_id(&SystemProviders));
    req.request_id = Some(request_id.clone());
    let tenant_id = state
        .tenant_quotas()
        .normalize_tenant_id(req.tenant_id.as_deref());
    req.tenant_id = Some(tenant_id.clone());

    let _ = send_stream_event(
        &mut socket,
        "started",
        json_object([
            ("request_id", request_id.clone()),
            ("tenant_id", tenant_id.clone()),
        ]),
    )
    .await;

    let response = execute_request(State(state), Extension(principal), headers, Json(req))
        .await
        .1
         .0;

    if !response.stdout.is_empty() {
        let _ = send_stream_event(
            &mut socket,
            "stdout",
            json_object([("data", response.stdout.clone())]),
        )
        .await;
    }
    if !response.stderr.is_empty() {
        let _ = send_stream_event(
            &mut socket,
            "stderr",
            json_object([("data", response.stderr.clone())]),
        )
        .await;
    }
    let mut exit_fields = BTreeMap::new();
    exit_fields.insert("request_id".to_string(), request_id);
    exit_fields.insert("tenant_id".to_string(), tenant_id);
    exit_fields.insert("exit_code".to_string(), response.exit_code.to_string());
    exit_fields.insert("duration_ms".to_string(), response.duration_ms.to_string());
    if let Some(output) = &response.output {
        exit_fields.insert(
            "stdout_truncated".to_string(),
            output.stdout_truncated.to_string(),
        );
        exit_fields.insert(
            "stderr_truncated".to_string(),
            output.stderr_truncated.to_string(),
        );
    }
    if let Some(error) = response.error {
        exit_fields.insert("error_code".to_string(), format!("{:?}", error.code));
        exit_fields.insert("error".to_string(), error.message);
    }
    let _ = send_stream_event(&mut socket, "completed", exit_fields).await;
    let _ = socket.close().await;
}

async fn handle_stream_websocket(
    socket: WebSocket,
    state: AppState,
    principal: AuthPrincipal,
    headers: HeaderMap,
) {
    let mut socket = socket;
    let req = match receive_stream_request(&mut socket).await {
        Ok(req) => req,
        Err(error) => {
            let _ = send_stream_error(&mut socket, &error).await;
            let _ = socket.close().await;
            return;
        }
    };

    handle_stream(socket, state, principal, headers, req).await;
}

async fn receive_stream_request(socket: &mut WebSocket) -> Result<ExecuteRequest, SynapseError> {
    let message = socket.recv().await.ok_or_else(|| {
        SynapseError::InvalidInput("missing initial stream request payload".to_string())
    })?;
    let message = message
        .map_err(|error| SynapseError::InvalidInput(format!("invalid websocket frame: {error}")))?;

    match message {
        Message::Text(text) => serde_json::from_str(&text).map_err(|error| {
            SynapseError::InvalidInput(format!("invalid stream request payload: {error}"))
        }),
        Message::Binary(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            SynapseError::InvalidInput(format!("invalid stream request payload: {error}"))
        }),
        _ => Err(SynapseError::InvalidInput(
            "expected initial text or binary message with JSON request payload".to_string(),
        )),
    }
}

fn hydrate_request(
    req: &mut ExecuteRequest,
    headers: &HeaderMap,
    state: &AppState,
    principal: &AuthPrincipal,
) -> Result<(), SynapseError> {
    if req.request_id.is_none() {
        if let Some(request_id) = header_value(headers, REQUEST_ID_HEADER) {
            validate_request_id(request_id)?;
            req.request_id = Some(request_id.to_string());
        } else {
            req.request_id = Some(new_request_id(&SystemProviders));
        }
    } else if let Some(request_id) = req.request_id.as_deref() {
        validate_request_id(request_id)?;
    }

    req.tenant_id = Some(
        state.tenant_quotas().normalize_tenant_id(
            req.tenant_id
                .as_deref()
                .or_else(|| header_value(headers, TENANT_HEADER)),
        ),
    );
    if let Some(tenant_id) = req.tenant_id.as_deref() {
        state.auth().authorize_tenant(principal, tenant_id)?;
    }

    Ok(())
}

fn header_value<'a>(headers: &'a HeaderMap, key: &str) -> Option<&'a str> {
    headers.get(key).and_then(|value| value.to_str().ok())
}

fn render_metrics(state: &AppState) -> String {
    let metrics = state.pool().metrics();
    let tenant_config = state.tenant_quotas().config();
    let scheduler_metrics = state.scheduler().metrics();
    let scheduler_config = state.scheduler().config();
    let execution_metrics = state.execution_metrics().snapshot();
    format!(
        concat!(
            "synapse_pool_configured_size {}\n",
            "synapse_pool_available {}\n",
            "synapse_pool_pooled_total {}\n",
            "synapse_pool_active {}\n",
            "synapse_pool_overflow_active {}\n",
            "synapse_pool_overflow_total {}\n",
            "synapse_pool_poisoned_total {}\n",
            "synapse_execute_requests_total {}\n",
            "synapse_execute_completed_total {}\n",
            "synapse_execute_failed_total {}\n",
            "synapse_execute_timeouts_total {}\n",
            "synapse_tenant_max_concurrency {}\n",
            "synapse_tenant_max_requests_per_minute {}\n",
            "synapse_tenant_max_timeout_ms {}\n",
            "synapse_tenant_max_cpu_time_limit_ms {}\n",
            "synapse_tenant_max_memory_limit_mb {}\n",
            "synapse_queue_max_depth {}\n",
            "synapse_queue_timeout_ms {}\n",
            "synapse_capacity_max_concurrency {}\n",
            "synapse_queue_active {}\n",
            "synapse_queue_depth {}\n",
            "synapse_queue_admitted_total {}\n",
            "synapse_queue_rejected_total {}\n",
            "synapse_queue_timeout_total {}\n",
            "synapse_queue_wait_time_ms_total {}\n",
            "synapse_execute_success_total {}\n",
            "synapse_execute_error_total {}\n",
            "synapse_execute_invalid_input_total {}\n",
            "synapse_execute_unsupported_language_total {}\n",
            "synapse_execute_runtime_unavailable_total {}\n",
            "synapse_execute_execution_failed_total {}\n",
            "synapse_execute_queue_timeout_total {}\n",
            "synapse_execute_capacity_rejected_total {}\n",
            "synapse_execute_wall_timeout_total {}\n",
            "synapse_execute_cpu_time_limit_exceeded_total {}\n",
            "synapse_execute_memory_limit_exceeded_total {}\n",
            "synapse_execute_sandbox_policy_blocked_total {}\n",
            "synapse_execute_quota_exceeded_total {}\n",
            "synapse_execute_rate_limited_total {}\n",
            "synapse_execute_audit_failed_total {}\n",
            "synapse_execute_io_error_total {}\n",
            "synapse_execute_auth_required_total {}\n",
            "synapse_execute_auth_invalid_total {}\n",
            "synapse_execute_tenant_forbidden_total {}\n",
            "synapse_execute_stdout_truncated_total {}\n",
            "synapse_execute_stderr_truncated_total {}\n",
        ),
        metrics.configured_size,
        metrics.available,
        metrics.pooled_total,
        metrics.active,
        metrics.overflow_active,
        metrics.overflow_total,
        metrics.poisoned_total,
        metrics.requests_total,
        metrics.completed_total,
        metrics.failed_total,
        metrics.timeouts_total,
        tenant_config.max_concurrent_executions_per_tenant,
        tenant_config.max_requests_per_minute,
        tenant_config.max_timeout_ms,
        tenant_config.max_cpu_time_limit_ms,
        tenant_config.max_memory_limit_mb,
        tenant_config.max_queue_depth,
        tenant_config.max_queue_timeout_ms,
        scheduler_config.max_concurrent_executions,
        scheduler_metrics.active_total,
        scheduler_metrics.queued_total,
        scheduler_metrics.admitted_total,
        scheduler_metrics.rejected_total,
        scheduler_metrics.queue_timeout_total,
        scheduler_metrics.queue_wait_time_ms_total,
        execution_metrics.success_total,
        execution_metrics.error_total,
        execution_metrics.invalid_input_total,
        execution_metrics.unsupported_language_total,
        execution_metrics.runtime_unavailable_total,
        execution_metrics.execution_failed_total,
        execution_metrics.queue_timeout_total,
        execution_metrics.capacity_rejected_total,
        execution_metrics.wall_timeout_total,
        execution_metrics.cpu_time_limit_exceeded_total,
        execution_metrics.memory_limit_exceeded_total,
        execution_metrics.sandbox_policy_blocked_total,
        execution_metrics.quota_exceeded_total,
        execution_metrics.rate_limited_total,
        execution_metrics.audit_failed_total,
        execution_metrics.io_error_total,
        execution_metrics.auth_required_total,
        execution_metrics.auth_invalid_total,
        execution_metrics.tenant_forbidden_total,
        execution_metrics.stdout_truncated_total,
        execution_metrics.stderr_truncated_total,
    )
}

fn log_execution_outcome(request_id: &str, tenant_id: &str, response: &ExecuteResponse) {
    let error_code = response
        .error
        .as_ref()
        .map(|error| format!("{:?}", error.code))
        .unwrap_or_else(|| "none".to_string());
    let stdout_truncated = response
        .output
        .as_ref()
        .map(|output| output.stdout_truncated)
        .unwrap_or(false);
    let stderr_truncated = response
        .output
        .as_ref()
        .map(|output| output.stderr_truncated)
        .unwrap_or(false);
    info!(
        request_id = sanitize_log_value(request_id),
        tenant_id = sanitize_log_value(tenant_id),
        exit_code = response.exit_code,
        duration_ms = response.duration_ms,
        error_code,
        has_error = response.error.is_some(),
        stdout_truncated,
        stderr_truncated,
        "execution request completed"
    );
}

fn persist_audit(state: &AppState, request_id: &str, events: &[AuditEvent]) {
    if let Err(error) = state.audit_log().persist(request_id, events) {
        state
            .execution_metrics()
            .record_error_code(synapse_core::ErrorCode::AuditFailed);
        error!(
            request_id = sanitize_log_value(request_id),
            %error,
            "failed to persist audit log"
        );
    }
}

fn audit_with_error(
    request_id: &str,
    tenant_id: &str,
    kind: AuditEventKind,
    error: &SynapseError,
) -> AuditEvent {
    let mut event = audit_event(
        request_id.to_string(),
        Some(tenant_id),
        kind,
        error.to_string(),
    );
    event
        .fields
        .insert("error_code".to_string(), format!("{:?}", error.code()));
    event
}

fn prepare_execution_audit_events(
    state: &AppState,
    request_id: &str,
    tenant_id: &str,
    request: &ExecuteRequest,
) -> Vec<AuditEvent> {
    let resolved_runtime = state
        .runtime_registry()
        .resolve(&request.language, request.runtime_version.as_deref())
        .ok();

    let mut sandbox_prepared = audit_event(
        request_id.to_string(),
        Some(tenant_id),
        AuditEventKind::SandboxPrepared,
        "sandbox prepared for execution",
    );
    sandbox_prepared
        .fields
        .insert("language".to_string(), request.language.clone());
    sandbox_prepared.fields.insert(
        "memory_limit_mb".to_string(),
        request.memory_limit_mb.to_string(),
    );

    let mut sandbox_reset = audit_event(
        request_id.to_string(),
        Some(tenant_id),
        AuditEventKind::SandboxReset,
        "sandbox reset before execution",
    );
    sandbox_reset
        .fields
        .insert("request_id".to_string(), request_id.to_string());

    let mut command_prepared = audit_event(
        request_id.to_string(),
        Some(tenant_id),
        AuditEventKind::CommandPrepared,
        "sandbox command prepared",
    );
    command_prepared
        .fields
        .insert("language".to_string(), request.language.clone());
    command_prepared.fields.insert(
        "wall_time_limit_ms".to_string(),
        request.timeout_ms.to_string(),
    );
    command_prepared.fields.insert(
        "cpu_time_limit_ms".to_string(),
        request.effective_cpu_time_limit_ms().to_string(),
    );
    command_prepared.fields.insert(
        "memory_limit_mb".to_string(),
        request.memory_limit_mb.to_string(),
    );
    if let Some(runtime) = resolved_runtime {
        command_prepared
            .fields
            .insert("runtime_language".to_string(), runtime.info.language);
        command_prepared
            .fields
            .insert("runtime_version".to_string(), runtime.info.resolved_version);
        command_prepared
            .fields
            .insert("command".to_string(), runtime.info.command);
    }

    let execution_started = audit_event(
        request_id.to_string(),
        Some(tenant_id),
        AuditEventKind::ExecutionStarted,
        "sandbox execution started",
    );

    vec![
        sandbox_prepared,
        sandbox_reset,
        command_prepared,
        execution_started,
    ]
}

fn finalize_execution_audit_events(
    request_id: &str,
    tenant_id: &str,
    response: &ExecuteResponse,
) -> Vec<AuditEvent> {
    let mut events = Vec::new();

    if let Some(error) = &response.error {
        if matches!(
            error.code,
            synapse_core::ErrorCode::WallTimeout
                | synapse_core::ErrorCode::CpuTimeLimitExceeded
                | synapse_core::ErrorCode::MemoryLimitExceeded
        ) {
            let mut limit_exceeded = audit_event(
                request_id.to_string(),
                Some(tenant_id),
                AuditEventKind::LimitExceeded,
                error.message.clone(),
            );
            limit_exceeded
                .fields
                .insert("error_code".to_string(), format!("{:?}", error.code));
            if let Some(limits) = &response.limits {
                limit_exceeded.fields.insert(
                    "wall_time_limit_ms".to_string(),
                    limits.wall_time_limit_ms.to_string(),
                );
                limit_exceeded.fields.insert(
                    "cpu_time_limit_ms".to_string(),
                    limits.cpu_time_limit_ms.to_string(),
                );
                limit_exceeded.fields.insert(
                    "memory_limit_mb".to_string(),
                    limits.memory_limit_mb.to_string(),
                );
            }
            events.push(limit_exceeded);
        }
    }

    let mut execution_finished = audit_event(
        request_id.to_string(),
        Some(tenant_id),
        AuditEventKind::ExecutionFinished,
        "sandbox execution completed",
    );
    execution_finished
        .fields
        .insert("exit_code".to_string(), response.exit_code.to_string());
    execution_finished
        .fields
        .insert("duration_ms".to_string(), response.duration_ms.to_string());
    if let Some(error) = &response.error {
        execution_finished
            .fields
            .insert("error_code".to_string(), format!("{:?}", error.code));
    }
    if let Some(output) = &response.output {
        execution_finished.fields.insert(
            "stdout_truncated".to_string(),
            output.stdout_truncated.to_string(),
        );
        execution_finished.fields.insert(
            "stderr_truncated".to_string(),
            output.stderr_truncated.to_string(),
        );
    }
    if let Some(runtime) = &response.runtime {
        execution_finished
            .fields
            .insert("runtime_language".to_string(), runtime.language.clone());
        execution_finished.fields.insert(
            "runtime_version".to_string(),
            runtime.resolved_version.clone(),
        );
    }
    events.push(execution_finished);

    let sandbox_reset = audit_event(
        request_id.to_string(),
        Some(tenant_id),
        AuditEventKind::SandboxReset,
        "sandbox reset after execution",
    );
    events.push(sandbox_reset);

    events
}

fn map_error(error: SynapseError) -> (StatusCode, Json<ExecuteResponse>) {
    (
        status_for_error(&error),
        Json(ExecuteResponse::error(error.to_execute_error(), 0)),
    )
}

fn audit_visible_to_tenant(events: &[AuditEvent], tenant_id: &str) -> bool {
    let mut saw_tenant = false;
    for event in events {
        if let Some(event_tenant_id) = event.tenant_id.as_deref() {
            saw_tenant = true;
            if event_tenant_id != tenant_id {
                return false;
            }
        }
    }

    !saw_tenant || tenant_id == "default"
}

fn status_for_error(error: &SynapseError) -> StatusCode {
    match error {
        SynapseError::InvalidInput(_) | SynapseError::UnsupportedLanguage(_) => {
            StatusCode::BAD_REQUEST
        }
        SynapseError::QuotaExceeded(_) | SynapseError::RateLimited(_) => {
            StatusCode::TOO_MANY_REQUESTS
        }
        SynapseError::RuntimeUnavailable(_) => StatusCode::FAILED_DEPENDENCY,
        SynapseError::QueueTimeout(_) => StatusCode::REQUEST_TIMEOUT,
        SynapseError::CapacityRejected(_) => StatusCode::SERVICE_UNAVAILABLE,
        SynapseError::WallTimeout | SynapseError::CpuTimeLimitExceeded => {
            StatusCode::REQUEST_TIMEOUT
        }
        SynapseError::MemoryLimitExceeded => StatusCode::PAYLOAD_TOO_LARGE,
        SynapseError::SandboxPolicy(_) => StatusCode::FORBIDDEN,
        SynapseError::AuthRequired(_) | SynapseError::AuthInvalid(_) => StatusCode::UNAUTHORIZED,
        SynapseError::TenantForbidden(_) => StatusCode::FORBIDDEN,
        SynapseError::Execution(_)
        | SynapseError::Audit(_)
        | SynapseError::Internal(_)
        | SynapseError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn send_stream_event(
    socket: &mut WebSocket,
    event: &str,
    fields: BTreeMap<String, String>,
) -> Result<(), axum::Error> {
    let payload = serde_json::json!({
        "event": event,
        "fields": fields,
    });
    socket.send(Message::Text(payload.to_string())).await
}

async fn send_stream_error(
    socket: &mut WebSocket,
    error: &SynapseError,
) -> Result<(), axum::Error> {
    send_stream_event(
        socket,
        "error",
        json_object([
            ("error_code", format!("{:?}", error.code())),
            ("error", error.to_execute_error().message),
        ]),
    )
    .await
}

fn sanitize_log_value(value: &str) -> String {
    let mut sanitized = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>();
    sanitized.truncate(128);
    sanitized
}

fn json_object<const N: usize>(
    items: [(impl Into<String>, impl Into<String>); N],
) -> BTreeMap<String, String> {
    items
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect()
}
