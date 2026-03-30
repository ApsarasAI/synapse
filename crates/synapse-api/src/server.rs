use std::{collections::BTreeMap, io::ErrorKind, net::SocketAddr};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Json, Path, Query, Request, State,
    },
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::IntoResponse,
    routing::{get, post},
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use synapse_core::{
    audit_event, new_request_id, validate_request_id, AuditEvent, AuditEventKind, ExecuteRequest,
    ExecuteResponse, RequestStatus, RequestSummary, RequestSummaryQuery, SynapseError,
    SystemProviders,
};
use tower_http::trace::TraceLayer;
use tracing::{error, info, instrument};

pub use crate::app::AppState;
use crate::{
    app::{default_state, AuthPrincipal},
    metrics::ExecutionLifecycle,
};

const TENANT_HEADER: &str = "x-synapse-tenant-id";
const REQUEST_ID_HEADER: &str = "x-synapse-request-id";
const DEFAULT_ADMIN_LIST_LIMIT: usize = 50;
const MAX_ADMIN_LIST_LIMIT: usize = 200;

#[derive(Debug, Deserialize)]
struct AdminRequestsQuery {
    request_id: Option<String>,
    tenant_id: Option<String>,
    status: Option<String>,
    error_code: Option<String>,
    language: Option<String>,
    from: Option<u64>,
    to: Option<u64>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct AdminOverviewResponse {
    service: AdminServiceStatus,
    metrics: AdminMetricsSnapshot,
    top_error_codes: Vec<AdminErrorCount>,
    recent_failures: Vec<RequestSummary>,
}

#[derive(Debug, Serialize)]
struct AdminServiceStatus {
    status: &'static str,
    version: &'static str,
    started_at_ms: u64,
}

#[derive(Debug, Serialize)]
struct AdminMetricsSnapshot {
    success_total: u64,
    error_total: u64,
    queued_total: u64,
    capacity_rejected_total: u64,
    queue_timeout_total: u64,
    runtime_unavailable_total: u64,
    wall_timeout_total: u64,
    cpu_time_limit_exceeded_total: u64,
    memory_limit_exceeded_total: u64,
    sandbox_policy_blocked_total: u64,
    rate_limited_total: u64,
    quota_exceeded_total: u64,
    stdout_truncated_total: u64,
    stderr_truncated_total: u64,
}

#[derive(Debug, Serialize)]
struct AdminErrorCount {
    code: String,
    count: u64,
}

#[derive(Debug, Serialize)]
struct AdminRequestsResponse {
    items: Vec<RequestSummary>,
}

#[derive(Debug, Serialize)]
struct AdminRequestAuditResponse {
    request_id: String,
    events: Vec<AuditEvent>,
}

#[derive(Debug, Serialize)]
struct AdminRuntimeResponse {
    active: Vec<AdminRuntimeEntry>,
    installed: Vec<AdminRuntimeEntry>,
}

#[derive(Debug, Serialize)]
struct AdminRuntimeEntry {
    language: String,
    version: String,
    command: String,
    path: String,
    active: bool,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct AdminCapacityResponse {
    scheduler: AdminSchedulerSnapshot,
    limits: AdminCapacityLimits,
}

#[derive(Debug, Serialize)]
struct AdminSchedulerSnapshot {
    active_total: usize,
    queued_total: usize,
    admitted_total: u64,
    rejected_total: u64,
    queue_timeout_total: u64,
    queue_wait_time_ms_total: u64,
}

#[derive(Debug, Serialize)]
struct AdminCapacityLimits {
    max_concurrency: usize,
    max_queue_depth: usize,
    max_queue_timeout_ms: u64,
    max_concurrent_executions_per_tenant: usize,
}

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
            "/admin/overview",
            get(admin_overview).route_layer(protected_layer.clone()),
        )
        .route(
            "/admin/requests",
            get(admin_list_requests).route_layer(protected_layer.clone()),
        )
        .route(
            "/admin/requests/:request_id",
            get(admin_get_request).route_layer(protected_layer.clone()),
        )
        .route(
            "/admin/requests/:request_id/audit",
            get(admin_get_request_audit).route_layer(protected_layer.clone()),
        )
        .route(
            "/admin/runtime",
            get(admin_runtime).route_layer(protected_layer.clone()),
        )
        .route(
            "/admin/capacity",
            get(admin_capacity).route_layer(protected_layer.clone()),
        )
        .route(
            "/execute",
            post(execute_request).route_layer(protected_layer.clone()),
        )
        .route(
            "/execute/stream",
            get(stream_execution_websocket).route_layer(protected_layer),
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

async fn admin_overview(
    State(state): State<AppState>,
    Extension(principal): Extension<AuthPrincipal>,
    Query(query): Query<AdminRequestsQuery>,
) -> impl IntoResponse {
    let summary_query = match admin_summary_query(&principal, query, true) {
        Ok(summary_query) => summary_query,
        Err(error) => {
            state.execution_metrics().record_error_code(error.code());
            return map_error(error).into_response();
        }
    };

    match state.request_summaries().list(&summary_query) {
        Ok(items) => {
            let snapshot = state.execution_metrics().snapshot();
            let mut error_counts = BTreeMap::new();
            for item in &items {
                if let Some(error_code) = item.error_code {
                    *error_counts.entry(error_code.to_string()).or_insert(0) += 1;
                }
            }
            let mut top_error_codes = error_counts
                .into_iter()
                .map(|(code, count)| AdminErrorCount { code, count })
                .collect::<Vec<_>>();
            top_error_codes.sort_by(|left, right| {
                right
                    .count
                    .cmp(&left.count)
                    .then_with(|| left.code.cmp(&right.code))
            });
            top_error_codes.truncate(5);

            let recent_failures = items
                .into_iter()
                .filter(|item| item.status == RequestStatus::Error)
                .take(10)
                .collect();

            Json(AdminOverviewResponse {
                service: AdminServiceStatus {
                    status: "ok",
                    version: env!("CARGO_PKG_VERSION"),
                    started_at_ms: state.started_at_ms(),
                },
                metrics: AdminMetricsSnapshot {
                    success_total: snapshot.success_total,
                    error_total: snapshot.error_total,
                    queued_total: snapshot.queued_total,
                    capacity_rejected_total: snapshot.capacity_rejected_total,
                    queue_timeout_total: snapshot.queue_timeout_total,
                    runtime_unavailable_total: snapshot.runtime_unavailable_total,
                    wall_timeout_total: snapshot.wall_timeout_total,
                    cpu_time_limit_exceeded_total: snapshot.cpu_time_limit_exceeded_total,
                    memory_limit_exceeded_total: snapshot.memory_limit_exceeded_total,
                    sandbox_policy_blocked_total: snapshot.sandbox_policy_blocked_total,
                    rate_limited_total: snapshot.rate_limited_total,
                    quota_exceeded_total: snapshot.quota_exceeded_total,
                    stdout_truncated_total: snapshot.stdout_truncated_total,
                    stderr_truncated_total: snapshot.stderr_truncated_total,
                },
                top_error_codes,
                recent_failures,
            })
            .into_response()
        }
        Err(error) => map_error(error).into_response(),
    }
}

async fn admin_list_requests(
    State(state): State<AppState>,
    Extension(principal): Extension<AuthPrincipal>,
    Query(query): Query<AdminRequestsQuery>,
) -> impl IntoResponse {
    let summary_query = match admin_summary_query(&principal, query, false) {
        Ok(summary_query) => summary_query,
        Err(error) => {
            state.execution_metrics().record_error_code(error.code());
            return map_error(error).into_response();
        }
    };

    match state.request_summaries().list(&summary_query) {
        Ok(items) => Json(AdminRequestsResponse { items }).into_response(),
        Err(error) => map_error(error).into_response(),
    }
}

async fn admin_get_request(
    State(state): State<AppState>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(request_id): Path<String>,
) -> impl IntoResponse {
    if let Err(error) = validate_request_id(&request_id) {
        return map_error(error).into_response();
    }

    match state.request_summaries().load(&request_id) {
        Ok(summary) if principal.allows_tenant(&summary.tenant_id) => {
            (StatusCode::OK, Json(summary)).into_response()
        }
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(SynapseError::Io(error)) if error.kind() == ErrorKind::NotFound => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(error) => map_error(error).into_response(),
    }
}

async fn admin_get_request_audit(
    State(state): State<AppState>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(request_id): Path<String>,
) -> impl IntoResponse {
    if let Err(error) = validate_request_id(&request_id) {
        return map_error(error).into_response();
    }

    match state.request_summaries().load(&request_id) {
        Ok(summary) if !principal.allows_tenant(&summary.tenant_id) => {
            StatusCode::NOT_FOUND.into_response()
        }
        Ok(_) => match state.audit_log().load(&request_id) {
            Ok(events) => (
                StatusCode::OK,
                Json(AdminRequestAuditResponse { request_id, events }),
            )
                .into_response(),
            Err(SynapseError::Io(error)) if error.kind() == ErrorKind::NotFound => {
                StatusCode::NOT_FOUND.into_response()
            }
            Err(error) => map_error(error).into_response(),
        },
        Err(SynapseError::Io(error)) if error.kind() == ErrorKind::NotFound => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(error) => map_error(error).into_response(),
    }
}

async fn admin_runtime(
    State(state): State<AppState>,
    _principal: Extension<AuthPrincipal>,
) -> impl IntoResponse {
    let installed = state
        .runtime_registry()
        .list()
        .into_iter()
        .map(|runtime| AdminRuntimeEntry {
            language: runtime.language,
            version: runtime.version,
            command: runtime.command,
            path: runtime.binary.display().to_string(),
            active: runtime.active,
            status: if runtime.healthy { "ok" } else { "corrupt" },
        })
        .collect::<Vec<_>>();
    let active = installed
        .iter()
        .filter(|runtime| runtime.active)
        .map(|runtime| AdminRuntimeEntry {
            language: runtime.language.clone(),
            version: runtime.version.clone(),
            command: runtime.command.clone(),
            path: runtime.path.clone(),
            active: runtime.active,
            status: runtime.status,
        })
        .collect();

    Json(AdminRuntimeResponse { active, installed }).into_response()
}

async fn admin_capacity(
    State(state): State<AppState>,
    _principal: Extension<AuthPrincipal>,
) -> impl IntoResponse {
    let metrics = state.scheduler().metrics();
    let config = state.scheduler().config();
    Json(AdminCapacityResponse {
        scheduler: AdminSchedulerSnapshot {
            active_total: metrics.active_total,
            queued_total: metrics.queued_total,
            admitted_total: metrics.admitted_total,
            rejected_total: metrics.rejected_total,
            queue_timeout_total: metrics.queue_timeout_total,
            queue_wait_time_ms_total: metrics.queue_wait_time_ms_total,
        },
        limits: AdminCapacityLimits {
            max_concurrency: config.max_concurrent_executions,
            max_queue_depth: config.max_queue_depth,
            max_queue_timeout_ms: config.max_queue_timeout_ms,
            max_concurrent_executions_per_tenant: config.max_concurrent_executions_per_tenant,
        },
    })
    .into_response()
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
    let request_language = req.language.clone();

    let mut audit = vec![audit_event(
        request_id.clone(),
        Some(&tenant_id),
        AuditEventKind::RequestReceived,
        "execution request received",
    )];
    let request_started_at_ms = audit[0].timestamp_ms;
    let mut queue_wait_ms = 0;

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
        persist_request_summary(
            &state,
            build_request_summary(
                &request_id,
                &tenant_id,
                &request_language,
                request_started_at_ms,
                queue_wait_ms,
                &response,
            ),
        );
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
        persist_request_summary(
            &state,
            build_request_summary(
                &request_id,
                &tenant_id,
                &request_language,
                request_started_at_ms,
                queue_wait_ms,
                &response,
            ),
        );
        return (status_for_error(&error), Json(response));
    }

    let _permit = match state.scheduler().acquire(&tenant_id).await {
        Ok(permit) => {
            state
                .execution_metrics()
                .record_lifecycle(ExecutionLifecycle::Admitted);
            if permit.was_queued() {
                state
                    .execution_metrics()
                    .record_lifecycle(ExecutionLifecycle::Queued);
            }
            let mut event = audit_event(
                request_id.clone(),
                Some(&tenant_id),
                AuditEventKind::QuotaAccepted,
                "execution capacity admitted",
            );
            event
                .fields
                .insert("lifecycle".to_string(), "admitted".to_string());
            event
                .fields
                .insert("queued".to_string(), permit.was_queued().to_string());
            event.fields.insert(
                "queue_wait_ms".to_string(),
                permit.wait_duration_ms().to_string(),
            );
            queue_wait_ms = permit.wait_duration_ms();
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
            persist_request_summary(
                &state,
                build_request_summary(
                    &request_id,
                    &tenant_id,
                    &request_language,
                    request_started_at_ms,
                    queue_wait_ms,
                    &response,
                ),
            );
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
                &state,
                &request_id,
                &tenant_id,
                &response,
            ));
            state
                .execution_metrics()
                .record_lifecycle(ExecutionLifecycle::Completed);
            response.audit = Some(synapse_core::AuditSummary {
                request_id: request_id.clone(),
                event_count: audit.len(),
            });
            state.execution_metrics().record_response(&response);
            persist_audit(&state, &request_id, &audit);
            persist_request_summary(
                &state,
                build_request_summary(
                    &request_id,
                    &tenant_id,
                    &request_language,
                    request_started_at_ms,
                    queue_wait_ms,
                    &response,
                ),
            );
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
            state
                .execution_metrics()
                .record_lifecycle(ExecutionLifecycle::Completed);
            state.execution_metrics().record_error_code(error.code());
            persist_audit(&state, &request_id, &audit);
            persist_request_summary(
                &state,
                build_request_summary(
                    &request_id,
                    &tenant_id,
                    &request_language,
                    request_started_at_ms,
                    queue_wait_ms,
                    &response,
                ),
            );
            error!(
                request_id = sanitize_log_value(&request_id),
                tenant_id = sanitize_log_value(&tenant_id),
                error_code = ?error.code(),
                error = %error,
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
        exit_fields.insert("error_code".to_string(), error.code.to_string());
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
            "synapse_execute_lifecycle_admitted_total {}\n",
            "synapse_execute_lifecycle_queued_total {}\n",
            "synapse_execute_lifecycle_started_total {}\n",
            "synapse_execute_lifecycle_runtime_resolved_total {}\n",
            "synapse_execute_lifecycle_limit_hit_total {}\n",
            "synapse_execute_lifecycle_completed_total {}\n",
            "synapse_execute_lifecycle_cleanup_done_total {}\n",
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
        execution_metrics.admitted_total,
        execution_metrics.queued_total,
        execution_metrics.started_total,
        execution_metrics.runtime_resolved_total,
        execution_metrics.limit_hit_total,
        execution_metrics.completed_total,
        execution_metrics.cleanup_done_total,
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

fn persist_request_summary(state: &AppState, summary: RequestSummary) {
    if let Err(error) = state.request_summaries().persist(&summary) {
        error!(
            request_id = sanitize_log_value(&summary.request_id),
            %error,
            "failed to persist request summary"
        );
    }
}

fn build_request_summary(
    request_id: &str,
    tenant_id: &str,
    language: &str,
    created_at_ms: u64,
    queue_wait_ms: u64,
    response: &ExecuteResponse,
) -> RequestSummary {
    RequestSummary {
        request_id: request_id.to_string(),
        tenant_id: tenant_id.to_string(),
        language: language.to_string(),
        status: if response.error.is_some() {
            RequestStatus::Error
        } else {
            RequestStatus::Success
        },
        error_code: response.error.as_ref().map(|error| error.code),
        created_at_ms,
        completed_at_ms: created_at_ms.saturating_add(response.duration_ms),
        duration_ms: response.duration_ms,
        queue_wait_ms,
        stdout_truncated: response
            .output
            .as_ref()
            .map(|output| output.stdout_truncated)
            .unwrap_or(false),
        stderr_truncated: response
            .output
            .as_ref()
            .map(|output| output.stderr_truncated)
            .unwrap_or(false),
        runtime_language: response
            .runtime
            .as_ref()
            .map(|runtime| runtime.language.clone()),
        runtime_version: response
            .runtime
            .as_ref()
            .map(|runtime| runtime.resolved_version.clone()),
    }
}

fn admin_summary_query(
    principal: &AuthPrincipal,
    query: AdminRequestsQuery,
    clamp_to_recent_failures: bool,
) -> Result<RequestSummaryQuery, SynapseError> {
    let requested_tenant = query.tenant_id.map(|tenant| tenant.trim().to_string());
    if let Some(tenant_id) = requested_tenant.as_deref() {
        if !principal.allows_tenant(tenant_id) {
            return Err(SynapseError::TenantForbidden(
                "token is not allowed to access the requested tenant".to_string(),
            ));
        }
    }

    let limit = query
        .limit
        .unwrap_or(DEFAULT_ADMIN_LIST_LIMIT)
        .clamp(1, MAX_ADMIN_LIST_LIMIT);
    let status = match query.status.as_deref() {
        Some("success") => Some(RequestStatus::Success),
        Some("error") => Some(RequestStatus::Error),
        Some(_) => {
            return Err(SynapseError::InvalidInput(
                "invalid status filter; expected success or error".to_string(),
            ));
        }
        None => None,
    };
    let error_code = match query.error_code {
        Some(code) => match code.parse::<synapse_core::ErrorCode>() {
            Ok(code) => Some(code),
            Err(()) => {
                return Err(SynapseError::InvalidInput(
                    "invalid error_code filter".to_string(),
                ));
            }
        },
        None => None,
    };
    let allowed_tenants = if principal.allows_all_tenants() {
        None
    } else {
        Some(principal.allowed_tenants())
    };

    let request_id = query
        .request_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(request_id) = request_id.as_deref() {
        validate_request_id(request_id)?;
    }

    Ok(RequestSummaryQuery {
        request_id,
        tenant_id: requested_tenant.filter(|tenant| !tenant.is_empty()),
        status: if clamp_to_recent_failures {
            Some(RequestStatus::Error)
        } else {
            status
        },
        error_code,
        language: query.language.map(|value| value.trim().to_string()),
        from_created_at_ms: query.from,
        to_created_at_ms: query.to,
        allowed_tenants,
        limit: Some(if clamp_to_recent_failures {
            limit.max(10)
        } else {
            limit
        }),
    })
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
    state
        .execution_metrics()
        .record_lifecycle(ExecutionLifecycle::Started);

    let mut sandbox_prepared = audit_event(
        request_id.to_string(),
        Some(tenant_id),
        AuditEventKind::SandboxPrepared,
        "sandbox prepared for execution",
    );
    sandbox_prepared
        .fields
        .insert("lifecycle".to_string(), "started".to_string());
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
        .insert("lifecycle".to_string(), "started".to_string());
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
        .insert("lifecycle".to_string(), "started".to_string());
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
        state
            .execution_metrics()
            .record_lifecycle(ExecutionLifecycle::RuntimeResolved);
        command_prepared
            .fields
            .insert("lifecycle".to_string(), "runtime_resolved".to_string());
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

    let mut execution_started = audit_event(
        request_id.to_string(),
        Some(tenant_id),
        AuditEventKind::ExecutionStarted,
        "sandbox execution started",
    );
    execution_started
        .fields
        .insert("lifecycle".to_string(), "started".to_string());

    vec![
        sandbox_prepared,
        sandbox_reset,
        command_prepared,
        execution_started,
    ]
}

fn finalize_execution_audit_events(
    state: &AppState,
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
            state
                .execution_metrics()
                .record_lifecycle(ExecutionLifecycle::LimitHit);
            let mut limit_exceeded = audit_event(
                request_id.to_string(),
                Some(tenant_id),
                AuditEventKind::LimitExceeded,
                error.message.clone(),
            );
            limit_exceeded
                .fields
                .insert("lifecycle".to_string(), "limit_hit".to_string());
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
        .insert("lifecycle".to_string(), "completed".to_string());
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

    state
        .execution_metrics()
        .record_lifecycle(ExecutionLifecycle::CleanupDone);
    let mut sandbox_reset = audit_event(
        request_id.to_string(),
        Some(tenant_id),
        AuditEventKind::SandboxReset,
        "sandbox reset after execution",
    );
    sandbox_reset
        .fields
        .insert("lifecycle".to_string(), "cleanup_done".to_string());
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
            ("error_code", error.code().to_string()),
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
