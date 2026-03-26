# TODO

- Status summary (based on current codebase review):
  - Fully implemented: 3
  - Partially implemented: 6
  - Not implemented: 0
  - Current status labels:
    - `Not implemented`: no production code for the TODO item yet
    - `Partially implemented`: some code/tests/CLI/API support exists, but the TODO item is not complete
    - `Implemented`: all listed next steps are effectively done

- [P0 / Critical] Build OverlayFS-based layered runtimes. `[Implemented]`
  Why: this is the highest-value infrastructure gap for startup latency, concurrency density, and future multi-language support.
  Current assessment:
  - Linux execution now mounts `/workspace` through `bubblewrap` overlay arguments with a shared runtime lower layer plus per-sandbox `upper/` and `work/` directories.
  - Pooled sandbox reset clears and recreates the writable layers between executions, and disposable sandboxes destroy them at the end of the request lifecycle.
  - Integration coverage now exercises write isolation across sequential executions and verifies that only the writable layer directories remain after reset.
  Next steps:
  1. Define the read-only runtime layer layout and writable upper/work layer lifecycle.
  2. Mount sandbox filesystems with OverlayFS instead of the current single writable directory approach.
  3. Add cleanup and reset logic for per-execution writable layers.
  4. Add runtime and integration tests covering isolation, reset, and performance-sensitive setup paths.

- [P1 / High] Split `timeout_ms` into separate wall-clock and CPU-time budgets. `[Partially implemented]`
  Why: the current runtime uses one value for both wall timeout and cgroup CPU accounting, which is functional but conflates two different control goals.
  Current assessment:
  - `ExecuteRequest` already has `cpu_time_limit_ms`.
  - Backward compatibility exists via defaulting `cpu_time_limit_ms` to `timeout_ms`.
  - The runtime enforces wall timeout and CPU budget separately.
  - Validation and some runtime tests exist, but API/runtime coverage for divergent wall-time vs CPU-time scenarios is still incomplete.
  Next steps:
  1. Extend `ExecuteRequest` and config with a distinct `cpu_time_limit_ms` field.
  2. Keep backward compatibility by defaulting `cpu_time_limit_ms` from `timeout_ms` during rollout.
  3. Update the runtime to enforce wall timeout and CPU budget independently.
  4. Add API and runtime tests covering divergent wall-time vs CPU-time scenarios.

- [P1 / High] Add runtime and language management. `[Partially implemented]`
  Why: the project is still effectively a Python-only MVP and needs explicit runtime/version management before it can behave like a real execution platform.
  Current assessment:
  - There is now a managed runtime store with per-version `manifest.json`, integrity hash validation, and active-version pointers.
  - The CLI now supports `runtime list`, `runtime verify`, `runtime install`, `runtime install-bundle`, `runtime import-host`, and `runtime activate`.
  - Execution resolves runtimes through the managed registry instead of hard-coding Python lookup directly in the execution path, and missing active runtimes now fail explicitly instead of silently importing from the host.
  - The system still only supports Python. It can now install an offline runtime bundle directory without consulting host PATH, but the explicit `runtime import-host` flow still depends on a host `python3`, so this is not yet a full multi-language or independently provisioned runtime platform.
  Next steps:
  1. Define runtime metadata and version selection for supported languages.
  2. Add CLI and/or config support for installing, listing, and selecting runtimes.
  3. Separate language resolution from hard-coded Python-only execution.
  4. Add tests for runtime discovery, selection, and missing-runtime failures.

- [P1 / High] Improve observability. `[Partially implemented]`
  Why: current visibility is limited to basic metrics, which is not enough for debugging, operations, or future multi-tenant usage.
  Current assessment:
  - The API and service layers already use tracing instrumentation.
  - The server exposes pool, quota, and execution-outcome metrics, including structured failure and truncation counters.
  - Request-correlated IDs and execution completion/failure logs exist.
  - Observability is still basic: sandbox lifecycle logs are incomplete, and tracing/metrics validation is still limited to smoke checks.
  Next steps:
  1. Add structured tracing around request handling and execution lifecycle events.
  2. Expand execution metrics with clearer failure and limit-exceeded dimensions.
  3. Add request-correlated logs for sandbox creation, execution, reset, and cleanup.
  4. Add tests or smoke checks for emitted metrics and tracing coverage where practical.

- [P0 / Critical] Add audit logging. `[Implemented]`
  Why: this is a core product differentiator in `product.md` and a prerequisite for enterprise trust, compliance review, and security operations.
  Current assessment:
  - The codebase now has an audit event model, persisted audit logs, and an audit retrieval endpoint.
  - Request receipt, quota outcomes, command preparation, execution start/finish, limit-exceeded outcomes, file access, network attempts, and process spawn attempts are all captured with structured fields.
  - Syscall-derived sandbox events are collected via `strace` and appended to the same request-scoped audit record surfaced by `/audits/:request_id`.
  Next steps:
  1. Define the structured audit event model for file access, command execution, network attempts, and policy blocks.
  2. Capture and persist audit events during sandbox execution with low overhead.
  3. Expose audit data through logs and/or enterprise-facing retrieval paths.
  4. Add tests for representative audit scenarios and verify no sensitive host data leaks into events.

- [P0 / Critical] Add multi-tenant quotas and fairness controls. `[Implemented]`
  Why: without tenant isolation and quota enforcement, the product cannot safely evolve into a SaaS or enterprise shared platform.
  Current assessment:
  - Tenant identity handling exists at the API boundary.
  - Per-tenant concurrency, rate, timeout, CPU, and memory ceilings are enforced.
  - The API now admits work through a global execution scheduler instead of allowing unbounded overflow when the pool is saturated.
  - Shared capacity now has tenant-aware queuing, queue timeout, and capacity rejection semantics, with queue metrics exposed through `/metrics`.
  - Scheduler unit tests and API contention tests now cover queue timeout, capacity rejection, and round-robin fairness across tenants.
  Next steps:
  1. Add tenant identity handling at the API boundary.
  2. Enforce per-tenant concurrency, rate, timeout, and memory ceilings.
  3. Add tenant-level queuing and fair scheduling over the shared sandbox pool.
  4. Add tests for contention, starvation prevention, and over-quota behavior.

- [P1 / High] Stabilize the error model and API contract. `[Partially implemented]`
  Why: the product needs precise, machine-readable failure semantics such as timeout, OOM, syscall blocked, pool exhausted, and output truncated.
  Current assessment:
  - The API already returns structured errors with stable error codes.
  - Runtime and quota failures are mapped onto product-level error categories.
  - Memory-limit hits, output truncation, queue timeout, and capacity rejection are now surfaced structurally.
  - The contract still lacks narrower internal/platform failure distinctions beyond the current `execution_failed` bucket.
  Next steps:
  1. Extend the response schema with structured error typing and details.
  2. Map runtime and pool failures onto stable product-level error codes.
  3. Keep backward compatibility where needed during rollout.
  4. Add API tests for each documented error category.

- [P1 / High] Add performance benchmarks, load tests, and regression gates. `[Partially implemented]`
  Why: startup latency and throughput are part of the product promise, so they need automated validation rather than ad hoc measurement.
  Current assessment:
  - Criterion benchmarks already exist for pool acquire, sandbox creation, and a basic execute path.
  - There is now a single repo-level `scripts/p0_gate.sh` entrypoint that runs formatting, clippy, and the full workspace test suite as the release/security gate for the current kernel baseline.
  - There are still no repeatable HTTP load tests and no explicit performance regression thresholds.
  Next steps:
  1. Add criterion benchmarks for sandbox creation, pool acquire, execution, and recycle paths.
  2. Add repeatable HTTP load tests for target concurrency and latency envelopes.
  3. Define regression thresholds aligned with product goals.
  4. Wire benchmark and load-test checks into CI or release gates where practical.

- [P1 / High] Add SDK and streaming integration support. `[Partially implemented]`
  Why: the target users are AI application developers, and the product needs first-class integration surfaces beyond raw REST calls.
  Current assessment:
  - A streaming execution endpoint now exists.
  - The streaming endpoint now supports standard WebSocket `GET` handshakes in addition to the earlier request-body form, and integration tests cover started/stdout|stderr/completed/close lifecycle semantics.
  - There is still no Python SDK and no documented integration guidance for agent/tool usage.
  Next steps:
  1. Add a Python SDK that wraps execution requests, retries, and connection handling.
  2. Add a streaming execution interface, likely WebSocket-based, for incremental stdout/stderr delivery.
  3. Document integration patterns for agent/tool use cases.
  4. Add integration tests for SDK behavior and stream lifecycle semantics.
