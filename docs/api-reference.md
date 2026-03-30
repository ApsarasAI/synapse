# API Reference v1

Synapse v1 exposes a small HTTP and websocket surface for enterprise agent execution.

## v1 Contract

The v1 integration surface is:

- `GET /health`
- `POST /execute`
- `GET /audits/:request_id`
- `GET /metrics`
- `GET /execute/stream`
- `GET /admin/overview`
- `GET /admin/requests`
- `GET /admin/requests/:request_id`
- `GET /admin/requests/:request_id/audit`
- `GET /admin/runtime`
- `GET /admin/capacity`

The contract frozen for v1 is:

- request and response field names documented in this file
- request correlation with `request_id` and `tenant_id`
- documented error code strings
- documented HTTP status mapping for synchronous calls
- websocket stream event names `started`, `stdout`, `stderr`, `completed`, `error`

Out of scope for v1:

- backward compatibility for undocumented fields
- browser or desktop execution APIs
- multi-language runtime APIs
- hosted SaaS-specific control plane APIs

## Authentication

- If `SYNAPSE_API_TOKENS` is unset, auth is disabled.
- If `SYNAPSE_API_TOKENS` is set, these routes require `Authorization: Bearer <token>`:
  - `POST /execute`
  - `GET /audits/:request_id`
  - `GET /metrics`
  - `GET /execute/stream`
  - `GET /admin/overview`
  - `GET /admin/requests`
  - `GET /admin/requests/:request_id`
  - `GET /admin/requests/:request_id/audit`
  - `GET /admin/runtime`
  - `GET /admin/capacity`

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

Notes:

- `network_policy.mode = "allow_list"` is currently rejected with `sandbox_policy_blocked`
- empty or blank `tenant_id` values normalize to `default`
- if both payload and header provide `tenant_id`, the payload value wins after validation

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

## GET /execute/stream

Stream one execution over websocket. The server accepts:

- websocket upgrade on `GET /execute/stream`, then one initial JSON message

Example:

```python
import asyncio
from synapse_sdk import SynapseClient, SynapseClientConfig

async def main() -> None:
    client = SynapseClient(SynapseClientConfig(base_url="http://127.0.0.1:8080"))
    async for event in client.execute_stream(
        "print('stream ok')\n",
        request_id="stream-demo",
    ):
        print(event)

asyncio.run(main())
```

Event shapes:

Each websocket message is a JSON object with this envelope:

```json
{
  "event": "stdout",
  "fields": {
    "data": "stream ok\n"
  }
}
```

Event payloads inside `fields`:

- `started`: `request_id`, `tenant_id`
- `stdout`: `data`
- `stderr`: `data`
- `completed`: `request_id`, `tenant_id`, `exit_code`, `duration_ms`; may also include `stdout_truncated`, `stderr_truncated`, `error_code`, `error`
- `error`: `error_code`, `error`; emitted when the request payload is invalid before execution starts

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

Metric names listed above are part of the v1 operational contract for release-gate and PoC validation.

## Admin APIs

The `/admin/*` surface is intended for a read-only operations console.

Common behavior:

- all admin routes use the same bearer auth rules as other protected APIs
- tenant-scoped tokens only see records for their allowed tenants
- wildcard tokens such as `*` may view global state
- request ids must pass the same validation rules as `/execute`

### GET /admin/overview

Return dashboard data for the operations console.

Response includes:

- `service`: current service status, version, and process start timestamp
- `metrics`: key execution, queue, quota, runtime, and truncation counters
- `alerts`: lightweight alert hints derived from current counters
- `top_error_codes`: aggregated error-code counts from recent failed requests
- `recent_failures`: recent failed request summaries

Representative example:

```json
{
  "service": {
    "status": "ok",
    "version": "0.1.0",
    "started_at_ms": 1711785600000
  },
  "metrics": {
    "success_total": 120,
    "error_total": 9,
    "queued_total": 5,
    "capacity_rejected_total": 1,
    "queue_timeout_total": 0,
    "runtime_unavailable_total": 1,
    "wall_timeout_total": 2,
    "cpu_time_limit_exceeded_total": 0,
    "memory_limit_exceeded_total": 0,
    "sandbox_policy_blocked_total": 0,
    "rate_limited_total": 0,
    "quota_exceeded_total": 0,
    "stdout_truncated_total": 0,
    "stderr_truncated_total": 0
  },
  "alerts": [
    {
      "severity": "critical",
      "code": "runtime_unavailable",
      "message": "runtime verification or activation requires attention",
      "count": 1
    }
  ],
  "top_error_codes": [
    {
      "code": "wall_timeout",
      "count": 2
    }
  ],
  "recent_failures": []
}
```

### GET /admin/requests

Return request summaries for recent executions.

Supported query parameters:

- `request_id`
- `tenant_id`
- `status=success|error`
- `error_code`
- `language`
- `from`
- `to`
- `limit` with a server-enforced max of `200`

Each item includes:

- `request_id`
- `tenant_id`
- `language`
- `status`
- `error_code`
- `created_at_ms`
- `completed_at_ms`
- `duration_ms`
- `queue_wait_ms`
- `stdout_truncated`
- `stderr_truncated`
- `runtime_language`
- `runtime_version`

### GET /admin/requests/:request_id

Return a single request summary.

Behavior notes:

- `404` if the request summary does not exist
- `404` if the record exists but is not visible to the current tenant scope

### GET /admin/requests/:request_id/audit

Return an envelope with `request_id` and `events` for the matching audit trail.

Behavior notes:

- `404` if the summary or audit log does not exist
- `404` if the record exists but is outside the token's tenant scope

### GET /admin/runtime

Return runtime inventory for the control plane.

Response includes:

- `active`: active runtimes only
- `installed`: all installed runtimes with `ok` or `corrupt` status

### GET /admin/capacity

Return scheduler capacity state for the control plane.

Response includes:

- `scheduler.active_total`
- `scheduler.queued_total`
- `scheduler.admitted_total`
- `scheduler.rejected_total`
- `scheduler.queue_timeout_total`
- `scheduler.queue_wait_time_ms_total`
- `limits.max_concurrency`
- `limits.max_queue_depth`
- `limits.max_queue_timeout_ms`
- `limits.max_concurrent_executions_per_tenant`
