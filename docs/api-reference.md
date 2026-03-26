# API Reference

Synapse exposes a small HTTP surface intended for developer-preview integrations.

## Stability

- `GET /health` is the most stable endpoint.
- `POST /execute`, `GET /audits/:request_id`, and `GET /metrics` may evolve during `0.x`.
- Field names and metric dimensions should be treated as preview-level unless documented otherwise in a release note.

## Authentication

- If `SYNAPSE_API_TOKENS` is unset, auth is disabled.
- If `SYNAPSE_API_TOKENS` is set, these routes require `Authorization: Bearer <token>`:
  - `POST /execute`
  - `GET /audits/:request_id`
  - `GET /metrics`
  - `GET|POST /execute/stream`

Tenant selection:

- `x-synapse-tenant-id` header is optional.
- Unset or blank tenant ids normalize to `default`.

Request correlation:

- `x-synapse-request-id` is optional for `/execute`.
- If omitted, Synapse generates one.
- Request ids must use ASCII letters, digits, `-`, or `_`, and be at most 128 characters.

## GET /health

Returns a plain-text liveness response.

Example:

```bash
curl http://127.0.0.1:8080/health
```

Response:

```text
ok
```

## POST /execute

Execute a code snippet in the configured sandbox.

Example request:

```bash
curl \
  -X POST http://127.0.0.1:8080/execute \
  -H 'content-type: application/json' \
  -H 'x-synapse-request-id: api-demo' \
  -d '{
    "language": "python",
    "code": "print(\"api demo\")\n",
    "timeout_ms": 5000,
    "memory_limit_mb": 128
  }'
```

Request body:

```json
{
  "language": "python",
  "code": "print(\"api demo\")\n",
  "timeout_ms": 5000,
  "cpu_time_limit_ms": 5000,
  "memory_limit_mb": 128,
  "runtime_version": "system",
  "tenant_id": "default",
  "request_id": "api-demo",
  "network_policy": {
    "mode": "disabled"
  }
}
```

Important request fields:

- `language`: currently intended for managed runtimes such as `python`
- `code`: required, non-empty
- `timeout_ms`: defaults to `5000`
- `cpu_time_limit_ms`: optional, defaults to `timeout_ms`
- `memory_limit_mb`: defaults to `128`
- `runtime_version`: optional, uses active runtime when omitted
- `tenant_id`: optional, defaults to `default`
- `request_id`: optional, may also come from `x-synapse-request-id`
- `network_policy`: defaults to `{"mode":"disabled"}`

Successful response example:

```json
{
  "stdout": "api demo\n",
  "stderr": "",
  "exit_code": 0,
  "duration_ms": 11,
  "request_id": "api-demo",
  "tenant_id": "default",
  "runtime": {
    "language": "python",
    "resolved_version": "system",
    "command": "python3"
  },
  "limits": {
    "wall_time_limit_ms": 5000,
    "cpu_time_limit_ms": 5000,
    "memory_limit_mb": 128
  },
  "output": {
    "stdout_truncated": false,
    "stderr_truncated": false
  },
  "audit": {
    "request_id": "api-demo",
    "event_count": 6
  }
}
```

Error response example:

```json
{
  "stdout": "",
  "stderr": "invalid input: code cannot be empty",
  "exit_code": -1,
  "duration_ms": 0,
  "error": {
    "code": "invalid_input",
    "message": "invalid input: code cannot be empty"
  }
}
```

Common error codes:

- `invalid_input`
- `runtime_unavailable`
- `queue_timeout`
- `capacity_rejected`
- `wall_timeout`
- `cpu_time_limit_exceeded`
- `memory_limit_exceeded`
- `sandbox_policy_blocked`
- `quota_exceeded`
- `rate_limited`
- `auth_required`
- `auth_invalid`
- `tenant_forbidden`
- `audit_failed`
- `io_error`
- `execution_failed`

Observed HTTP status mapping:

- `400`: `invalid_input`, `unsupported_language`
- `401`: `auth_required`, `auth_invalid`
- `403`: `sandbox_policy_blocked`, `tenant_forbidden`
- `408`: `queue_timeout`, `wall_timeout`, `cpu_time_limit_exceeded`
- `413`: `memory_limit_exceeded`
- `424`: `runtime_unavailable`
- `429`: `quota_exceeded`, `rate_limited`
- `503`: `capacity_rejected`
- `500`: `audit_failed`, `io_error`, `execution_failed`

## GET /audits/:request_id

Return the persisted audit trail for a request id.

Example:

```bash
curl http://127.0.0.1:8080/audits/api-demo
```

Response shape:

```json
[
  {
    "request_id": "api-demo",
    "tenant_id": "default",
    "kind": "request_received",
    "message": "execution request received",
    "fields": {}
  }
]
```

Behavior notes:

- `404` if the request id does not exist
- `404` if the record exists but is not visible to the active tenant context
- request ids must pass the same validation rules as `/execute`

## GET /metrics

Return Prometheus-style text metrics.

Example:

```bash
curl http://127.0.0.1:8080/metrics | rg '^synapse_'
```

Representative metrics:

- `synapse_pool_configured_size`
- `synapse_pool_available`
- `synapse_execute_requests_total`
- `synapse_execute_completed_total`
- `synapse_execute_error_total`
- `synapse_execute_runtime_unavailable_total`
- `synapse_execute_audit_failed_total`
- `synapse_tenant_max_concurrency`

Metric names and dimensions are still preview-level in `0.x`.
