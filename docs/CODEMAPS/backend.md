<!-- Generated: 2026-03-25 | Files scanned: 24 | Token estimate: ~850 -->
# 后端架构

## 路由
- `GET /health` -> `health()` -> `"ok"`
- `GET /metrics` -> `metrics()` -> `render_metrics()` -> `SandboxPool::metrics()`
- `POST /execute` -> `execute_request()` -> `SandboxPool::execute()` -> `executor`

## 请求链
```
POST /execute
  -> axum Json<ExecuteRequest>
  -> validate inside executor
  -> AppState.pool().execute(req)
  -> SandboxLease::execute(req)
  -> ExecuteResponse
  -> status mapping in server::map_error()
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
- `synapse-core/src/executor.rs`
  - `execute()`
  - `execute_in_prepared()`
  - `prepare_sandbox()`
  - `prepare_sandbox_blocking()`
  - `PreparedSandbox::{reset,destroy_blocking}`

## 中间件链
- 当前无显式 axum/tower middleware
- `tower-http` 已在 workspace 依赖中，但代码未接入 `TraceLayer` 或 `CorsLayer`
- 现状是直接路由 + 状态注入 + 错误映射

## 错误映射
- `InvalidInput` -> `400 BAD_REQUEST`
- `UnsupportedLanguage` -> `400 BAD_REQUEST`
- `Execution` -> `500 INTERNAL_SERVER_ERROR`
- `Io` -> `500 INTERNAL_SERVER_ERROR`

## 关键文件
- `crates/synapse-api/src/server.rs`
- `crates/synapse-api/tests/api_endpoints.rs`
- `crates/synapse-api/tests/security_execute.rs`
- `crates/synapse-core/src/pool.rs`
- `crates/synapse-core/src/executor.rs`
