# 10-Minute Quickstart

This guide walks through the minimum supported Linux workflow for the developer preview: validate the host, import a runtime, start the server, run one request, and verify audit and metrics output.

## 1. Check Host Dependencies

Required for the secure Linux sandbox path:

- `bwrap`
- `strace`
- cgroup v2 with `cpu`, `memory`, and `pids`
- `python3`

Run:

```bash
cargo run -p synapse-cli -- doctor
```

If `runtime` is the only failing check, continue to the next step and import a runtime. If `sandbox`, `strace`, or `cgroupv2` fail, fix the host first.

## 2. Import A Runtime

The fastest developer-preview path is importing `python3` from the host into the managed runtime store:

```bash
cargo run -p synapse-cli -- runtime import-host --language python --version system --command python3 --activate
```

Verify:

```bash
cargo run -p synapse-cli -- runtime verify --language python
```

You should see a `verified` line for `python:system`.

## 3. Start The Server

```bash
cargo run -p synapse-cli -- serve --listen 127.0.0.1:8080
```

Open a second terminal for the rest of the steps.

## 4. Verify Health

```bash
curl http://127.0.0.1:8080/health
```

Expected:

```text
ok
```

## 5. Execute One Request

```bash
curl \
  -X POST http://127.0.0.1:8080/execute \
  -H 'content-type: application/json' \
  -H 'x-synapse-request-id: quickstart-demo' \
  -d '{
    "language": "python",
    "code": "print(\"quickstart ok\")\n",
    "timeout_ms": 5000,
    "memory_limit_mb": 128
  }'
```

Expected fields:

- `stdout` contains `quickstart ok`
- `exit_code` is `0`
- `request_id` is `quickstart-demo`
- `tenant_id` is `default`
- `audit.event_count` is present

## 6. Read The Audit Record

```bash
curl http://127.0.0.1:8080/audits/quickstart-demo
```

You should receive a JSON array of audit events. Typical events include:

- request received
- quota admitted
- runtime resolved
- execution started
- execution finished
- sandbox reset

## 7. Check Metrics

```bash
curl http://127.0.0.1:8080/metrics | rg '^synapse_'
```

Look for at least:

- `synapse_execute_requests_total`
- `synapse_execute_completed_total`
- `synapse_pool_configured_size`

## 8. Optional Auth Check

If you set `SYNAPSE_API_TOKENS`, the protected endpoints require bearer auth. Example token document:

```bash
export SYNAPSE_API_TOKENS='[
  {"token":"dev-token","tenants":["default"]}
]'
```

Then call:

```bash
curl \
  -H 'Authorization: Bearer dev-token' \
  http://127.0.0.1:8080/metrics
```

## Failure Notes

`doctor` fails on `cgroupv2`

- The server can still answer `/health`, but secure execution prerequisites are incomplete.

`/execute` fails with `runtime_unavailable`

- Re-run `runtime import-host` or install a bundle into the managed store.

`/execute` fails with `sandbox_policy_blocked`

- `network_policy.mode = "allow_list"` is not supported in the current preview.

`/audits/:request_id` returns `404`

- Confirm you used the same request id and are querying the same tenant context.
