#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required for quickstart smoke" >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required for quickstart smoke" >&2
  exit 1
fi

if ! command -v bwrap >/dev/null 2>&1; then
  echo "bwrap is required for quickstart smoke" >&2
  exit 1
fi

if ! command -v strace >/dev/null 2>&1; then
  echo "strace is required for quickstart smoke" >&2
  exit 1
fi

smoke_root="$(mktemp -d)"
runtime_store="$smoke_root/runtime-store"
audit_root="$smoke_root/audit"
fake_cgroup_root="$smoke_root/fake-cgroupv2"
listen="127.0.0.1:18080"
server_pid=""

cleanup() {
  if [[ -n "${server_pid}" ]] && kill -0 "${server_pid}" 2>/dev/null; then
    kill "${server_pid}" 2>/dev/null || true
    wait "${server_pid}" 2>/dev/null || true
  fi
  rm -rf "$smoke_root"
}
trap cleanup EXIT

mkdir -p "$runtime_store" "$audit_root" "$fake_cgroup_root"
printf 'cpu memory pids\n' > "$fake_cgroup_root/cgroup.controllers"
: > "$fake_cgroup_root/cgroup.subtree_control"

export SYNAPSE_RUNTIME_STORE_DIR="$runtime_store"
export SYNAPSE_AUDIT_ROOT="$audit_root"

cargo run -p synapse-cli -- runtime import-host --language python --version system --command python3 --activate >/dev/null

SYNAPSE_CGROUP_V2_ROOT="$fake_cgroup_root" cargo run -p synapse-cli -- doctor >/dev/null

cargo run -p synapse-cli -- serve --listen "$listen" >"$smoke_root/server.log" 2>&1 &
server_pid="$!"

for _ in {1..50}; do
  if curl --silent --fail "http://$listen/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.2
done

curl --silent --fail "http://$listen/health" | grep -Fx "ok" >/dev/null

execute_response="$(curl --silent --fail \
  -X POST "http://$listen/execute" \
  -H 'content-type: application/json' \
  -H 'x-synapse-request-id: smoke-request' \
  -d '{
    "language": "python",
    "code": "print(\"smoke ok\")\n",
    "timeout_ms": 5000,
    "memory_limit_mb": 128
  }')"

printf '%s' "$execute_response" | grep -F '"stdout":"smoke ok\n"' >/dev/null
printf '%s' "$execute_response" | grep -F '"request_id":"smoke-request"' >/dev/null

curl --silent --fail "http://$listen/audits/smoke-request" | grep -F '"request_id":"smoke-request"' >/dev/null
curl --silent --fail "http://$listen/metrics" | grep -F 'synapse_execute_requests_total' >/dev/null
