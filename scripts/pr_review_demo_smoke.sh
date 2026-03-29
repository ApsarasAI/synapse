#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ -n "${SYNAPSE_PYTHON_BIN:-}" ]]; then
  python_bin="$SYNAPSE_PYTHON_BIN"
elif command -v python3.11 >/dev/null 2>&1; then
  python_bin="python3.11"
else
  python_bin="python3"
fi

if ! command -v "$python_bin" >/dev/null 2>&1; then
  echo "$python_bin is required for PR review demo smoke" >&2
  exit 1
fi

if ! "$python_bin" -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 1)' >/dev/null 2>&1; then
  echo "$python_bin must be Python 3.10 or newer for sdk/python" >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required for PR review demo smoke" >&2
  exit 1
fi

smoke_root="$(mktemp -d)"
runtime_store="$smoke_root/runtime-store"
audit_root="$smoke_root/audit"
fake_cgroup_root="$smoke_root/fake-cgroupv2"
listen="127.0.0.1:18081"
server_pid=""

cleanup() {
  status=$?
  if [[ $status -ne 0 && -f "$smoke_root/server.log" ]]; then
    cat "$smoke_root/server.log" >&2 || true
  fi
  if [[ -n "${server_pid}" ]] && kill -0 "${server_pid}" 2>/dev/null; then
    kill "${server_pid}" 2>/dev/null || true
    wait "${server_pid}" 2>/dev/null || true
  fi
  rm -rf "$smoke_root"
  exit $status
}
trap cleanup EXIT

mkdir -p "$runtime_store" "$audit_root" "$fake_cgroup_root"
printf 'cpu memory pids\n' > "$fake_cgroup_root/cgroup.controllers"
: > "$fake_cgroup_root/cgroup.subtree_control"

export SYNAPSE_RUNTIME_STORE_DIR="$runtime_store"
export SYNAPSE_AUDIT_ROOT="$audit_root"

cargo run -p synapse-cli -- runtime import-host --language python --version system --command python3 --activate >/dev/null
cargo run -p synapse-cli -- serve --listen "$listen" >"$smoke_root/server.log" 2>&1 &
server_pid="$!"

for _ in {1..50}; do
  if curl --silent --fail "http://$listen/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.2
done

curl --silent --fail "http://$listen/health" | grep -Fx "ok" >/dev/null

demo_output="$(
  SYNAPSE_BASE_URL="http://$listen" \
  SYNAPSE_REQUEST_ID="pr-review-demo-smoke" \
  PYTHONPATH="$repo_root/sdk/python/src" \
  "$python_bin" examples/pr-review-agent/run_demo.py
)"

printf '%s\n' "$demo_output" | grep -F 'PR review findings:' >/dev/null
printf '%s\n' "$demo_output" | grep -F 'Returning 0 on divide-by-zero hides an error condition' >/dev/null
curl --silent --fail "http://$listen/audits/pr-review-demo-smoke" | grep -F '"request_id":"pr-review-demo-smoke"' >/dev/null
