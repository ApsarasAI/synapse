<!-- Generated: 2026-03-25 | Files scanned: 24 | Token estimate: ~850 -->
# 后端架构

## 路由
- `GET /health` -> `health()` -> `"ok"`
- `GET /openapi.json` -> `openapi_spec()` -> generated OpenAPI document
- `GET /metrics` -> `metrics()` -> `render_metrics()` -> `SandboxPool::metrics()`
- `POST /execute` -> `execute_request()` -> `SandboxPool::execute()` -> `executor`

## 请求链
```
POST /execute
  -> raw body parse + content-type validation
  -> hydrate_request()
  -> validate inside service
  -> AppState.pool().execute(req)
  -> SandboxLease::execute(req)
  -> ExecuteResponse
  -> status mapping in server::status_for_execute_response()
```

## 服务 -> 核心映射
- `synapse-api/src/server.rs`
  - `AppState`
  - `router()`
  - `router_with_state()`
  - `serve()`
  - `execute_request()`
  - `metrics()`
  - `map_error()`
- `synapse-core/src/pool.rs`
  - `SandboxPool::new()`
  - `SandboxPool::acquire()`
  - `SandboxPool::execute()`
  - `SandboxPool::metrics()`
- `synapse-core/src/service.rs`
  - `execute()`
  - `execute_with_registry()`
  - `execute_with_engine_and_registry()`
- `synapse-core/src/sandbox.rs`
  - `SandboxEngine`
  - `SandboxInstance`
  - `SandboxExecution`

## 中间件链
- Bearer auth middleware protects `/execute`, `/metrics`, `/audits/*`, `/execute/stream`, and admin routes
- `TraceLayer` is attached at the router level
- request parsing errors for `/execute` are normalized into the same JSON error shape as executor failures

## 错误映射
- `InvalidInput` -> `400 BAD_REQUEST`
- `UnsupportedLanguage` -> `400 BAD_REQUEST`
- `QueueTimeout` / `WallTimeout` / `CpuTimeLimitExceeded` -> `408 REQUEST_TIMEOUT`
- `MemoryLimitExceeded` -> `413 PAYLOAD_TOO_LARGE`
- `RuntimeUnavailable` -> `424 FAILED_DEPENDENCY`
- `Execution` -> `500 INTERNAL_SERVER_ERROR`
- `Io` -> `500 INTERNAL_SERVER_ERROR`

## 关键文件
- `crates/synapse-api/src/server.rs`
- `crates/synapse-api/tests/api_endpoints.rs`
- `crates/synapse-api/tests/security_execute.rs`
- `crates/synapse-core/src/pool.rs`
- `crates/synapse-core/src/executor.rs`
