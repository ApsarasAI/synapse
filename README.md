# Synapse

[简体中文](README.zh-CN.md)

Synapse is a Linux-first sandbox for running short-lived AI agent code with controlled isolation, audit capture, and a small HTTP/CLI surface.

This repository is the developer preview release. The goal is simple: a new contributor should be able to validate the host, import a runtime, start the service, run one request, inspect one audit record, and understand where the project is headed in about 30 minutes.

## Why Synapse

- Run untrusted or semi-trusted Python snippets inside a constrained sandbox.
- Keep startup fast with pooled sandboxes and a small Rust workspace.
- Expose the minimum useful control plane: `doctor`, runtime management, `/execute`, `/audits/:request_id`, and `/metrics`.
- Make host requirements explicit instead of hiding them behind "works on my machine" assumptions.

## Use Cases

- Local agent-tool execution during product prototyping.
- Backend services that need auditable code execution for internal developer workflows.
- Security and systems experiments around sandbox policy, runtime packaging, and execution observability.

## Status And Compatibility

- Versioning: Synapse follows SemVer.
- Stability: the project is currently a `0.x` developer preview. Breaking changes can still happen between minor releases.
- Stable enough to depend on:
  - `synapse doctor`
  - `synapse runtime list|verify|install|install-bundle|import-host|activate`
  - `GET /health`
- Still expected to change in `0.x`:
  - `POST /execute` request and response shape
  - `GET /audits/:request_id` event fields
  - `GET /metrics` metric names and dimensions
  - streaming execution behavior
- Platform support:
  - Linux: supported for the secure sandbox path.
  - macOS / Windows: not supported for the release target. Build may work, but secure sandbox guarantees do not.

## System Requirements

Synapse's secure execution path currently assumes a Linux host with:

- `bwrap` / bubblewrap available in `PATH`
- `strace` available in `PATH`
- cgroup v2 with writable `cpu`, `memory`, and `pids` controllers
- Python 3 available if you use `synapse runtime import-host`
- Rust stable toolchain if you build from source

Validate prerequisites:

```bash
cargo run -p synapse-cli -- doctor
```

Typical failure modes:

- `sandbox` fails: install `bubblewrap` and enable unprivileged user namespaces.
- `strace` fails: install `strace`.
- `cgroupv2` fails: mount a writable cgroup v2 hierarchy and expose `cpu`, `memory`, and `pids`.
- `runtime` fails: import or install a managed runtime before starting the service.

## Installation

Preferred install paths:

1. Rust developers

```bash
cargo install --path crates/synapse-cli --locked
synapse --help
```

2. Non-Rust users

- Download the Linux binary from GitHub Releases.
- Verify the published SHA-256 checksum.
- Run `./synapse --help` and `./synapse doctor`.

Release artifacts are intended to include:

- `synapse-linux-x86_64.tar.gz`
- `synapse-linux-x86_64.sha256`
- release notes with a change summary

See [docs/release-process.md](docs/release-process.md) for the expected release workflow.

## Quick Start

If you only want the shortest path from source checkout to first request:

1. Import a managed runtime.

```bash
cargo run -p synapse-cli -- runtime import-host --language python --version system --command python3 --activate
```

2. Run host diagnostics.

```bash
cargo run -p synapse-cli -- doctor
```

3. Start the API.

```bash
cargo run -p synapse-cli -- serve --listen 127.0.0.1:8080
```

4. Verify health.

```bash
curl http://127.0.0.1:8080/health
```

Expected response:

```text
ok
```

5. Execute one request.

```bash
curl \
  -X POST http://127.0.0.1:8080/execute \
  -H 'content-type: application/json' \
  -H 'x-synapse-request-id: hello-demo' \
  -d '{
    "language": "python",
    "code": "print(\"hello from synapse\")\n",
    "timeout_ms": 5000,
    "memory_limit_mb": 128
  }'
```

Expected response shape:

```json
{
  "stdout": "hello from synapse\n",
  "stderr": "",
  "exit_code": 0,
  "duration_ms": 12,
  "request_id": "hello-demo",
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
    "request_id": "hello-demo",
    "event_count": 6
  }
}
```

6. Fetch the corresponding audit record.

```bash
curl http://127.0.0.1:8080/audits/hello-demo
```

7. Inspect metrics.

```bash
curl http://127.0.0.1:8080/metrics | rg '^synapse_'
```

For a fuller walkthrough, see [docs/quickstart/10-minute-quickstart.md](docs/quickstart/10-minute-quickstart.md).

## CLI Surface

The README examples are kept aligned with the current CLI help:

```bash
synapse --help
synapse serve --help
synapse runtime --help
```

Primary commands:

- `synapse serve --listen 127.0.0.1:8080`
- `synapse doctor`
- `synapse runtime import-host --activate --language python --version system --command python3`
- `synapse runtime verify --language python`
- `synapse runtime list`

## API Surface

Public developer-preview endpoints:

- `GET /health`
- `POST /execute`
- `GET /audits/:request_id`
- `GET /metrics`

Authentication behavior:

- If `SYNAPSE_API_TOKENS` is unset, API auth is disabled.
- If `SYNAPSE_API_TOKENS` is set, `/execute`, `/audits/:request_id`, `/metrics`, and `/execute/stream` require `Authorization: Bearer <token>`.

See [docs/api-reference.md](docs/api-reference.md) for request, response, and error examples.

## Troubleshooting

`synapse doctor` fails on `cgroupv2`

- Synapse can still expose `/health`, but secure execution is not ready for production-like validation.
- Make cgroup v2 writable and ensure `cpu`, `memory`, and `pids` controllers are available.

`/execute` returns `runtime_unavailable`

- Import or install a runtime, then re-run `synapse runtime verify --language python`.

`/execute` returns `sandbox_policy_blocked`

- Current developer preview only supports `network_policy.mode = "disabled"`.

`/execute` returns `queue_timeout` or `capacity_rejected`

- Increase pool and tenant queue settings, or reduce concurrent requests.

`/audits/:request_id` returns `404`

- Confirm the same `request_id` was used for the execute request and that the tenant context matches.

## Contributing

Start with [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

Useful entry points:

- [docs/quickstart/10-minute-quickstart.md](docs/quickstart/10-minute-quickstart.md)
- [docs/api-reference.md](docs/api-reference.md)
- [docs/release-process.md](docs/release-process.md)
- [docs/README.md](docs/README.md)
