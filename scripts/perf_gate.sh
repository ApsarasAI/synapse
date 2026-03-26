#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

criterion_max_mean_ns="${SYNAPSE_CRITERION_MAX_MEAN_NS:-500000000}"
http_p95_limit_ms="${SYNAPSE_HTTP_P95_LIMIT_MS:-1000}"
http_error_rate_limit="${SYNAPSE_HTTP_ERROR_RATE_LIMIT:-0}"
http_rps_min="${SYNAPSE_HTTP_RPS_MIN:-1}"

cargo bench -p synapse-core --bench mvp -- --sample-size 10 --measurement-time 1

python3 - "$criterion_max_mean_ns" <<'PY'
import json
import pathlib
import sys

limit = int(sys.argv[1])
root = pathlib.Path("target/criterion")
checks = []
for bench in ("pool_acquire", "sandbox_create", "execute_fast_path", "sandbox_recycle"):
    path = root / bench / "new" / "estimates.json"
    if not path.exists():
        print(f"missing criterion estimate for {bench}: {path}", file=sys.stderr)
        sys.exit(1)
    data = json.loads(path.read_text())
    mean = data["mean"]["point_estimate"]
    checks.append((bench, mean))

for bench, mean in checks:
    print(f"{bench}: mean_ns={mean}")
    if mean > limit:
        print(f"{bench} exceeded mean limit {limit}ns", file=sys.stderr)
        sys.exit(1)
PY

if [[ -n "${SYNAPSE_BENCH_SERVER_URL:-}" ]]; then
  http_output="$(python3 scripts/http_bench.py \
    --url "${SYNAPSE_BENCH_SERVER_URL%/}/execute" \
    --requests "${SYNAPSE_HTTP_REQUESTS:-20}" \
    --concurrency "${SYNAPSE_HTTP_CONCURRENCY:-4}" \
    ${SYNAPSE_BENCH_TOKEN:+--token "$SYNAPSE_BENCH_TOKEN"} \
    --tenant-id "${SYNAPSE_BENCH_TENANT_ID:-default}")"
  printf '%s\n' "$http_output"
  python3 - "$http_p95_limit_ms" "$http_error_rate_limit" "$http_rps_min" "$http_output" <<'PY'
import json
import sys

p95_limit = float(sys.argv[1])
error_limit = float(sys.argv[2])
rps_min = float(sys.argv[3])
summary = json.loads(sys.argv[4])

if summary["p95_ms"] > p95_limit:
    print(f"p95 {summary['p95_ms']}ms exceeded limit {p95_limit}ms", file=sys.stderr)
    sys.exit(1)
if summary["error_rate"] > error_limit:
    print(
        f"error rate {summary['error_rate']} exceeded limit {error_limit}",
        file=sys.stderr,
    )
    sys.exit(1)
if summary["throughput_rps"] < rps_min:
    print(
        f"throughput {summary['throughput_rps']} below minimum {rps_min}",
        file=sys.stderr,
    )
    sys.exit(1)
PY
else
  echo "SYNAPSE_BENCH_SERVER_URL not set; skipping HTTP benchmark gate"
fi
