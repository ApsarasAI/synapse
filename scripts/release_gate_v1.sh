#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ -n "${SYNAPSE_CARGO_BIN:-}" ]]; then
  cargo_bin="$SYNAPSE_CARGO_BIN"
else
  cargo_bin="cargo"
fi

if [[ -n "${SYNAPSE_PYTHON_BIN:-}" ]]; then
  python_bin="$SYNAPSE_PYTHON_BIN"
elif command -v python3.11 >/dev/null 2>&1; then
  python_bin="python3.11"
else
  python_bin="python3"
fi

quickstart_smoke_script="${SYNAPSE_QUICKSTART_SMOKE_SCRIPT:-scripts/quickstart_smoke.sh}"
pr_review_demo_smoke_script="${SYNAPSE_PR_REVIEW_DEMO_SMOKE_SCRIPT:-scripts/pr_review_demo_smoke.sh}"
perf_gate_script="${SYNAPSE_PERF_GATE_SCRIPT:-scripts/perf_gate.sh}"
run_perf_gate="${SYNAPSE_RELEASE_RUN_PERF_GATE:-0}"

echo "[v1-gate] cargo fmt"
"$cargo_bin" fmt --all --check

echo "[v1-gate] cargo clippy"
"$cargo_bin" clippy --workspace --all-targets -- -D warnings

echo "[v1-gate] cargo test"
"$cargo_bin" test --workspace

echo "[v1-gate] python sdk tests"
PYTHONPATH="sdk/python/src${PYTHONPATH:+:$PYTHONPATH}" \
  "$python_bin" -m unittest discover -s sdk/python/tests

echo "[v1-gate] gtm asset tests"
"$python_bin" -m unittest discover -s scripts/tests

echo "[v1-gate] quickstart smoke"
"$quickstart_smoke_script"

echo "[v1-gate] python sdk import smoke"
"$python_bin" -c "import pathlib; exec(compile(pathlib.Path('examples/pr-review-agent/run_demo.py').read_text(), 'examples/pr-review-agent/run_demo.py', 'exec'), {'__name__': 'synapse_demo_smoke', '__file__': 'examples/pr-review-agent/run_demo.py'})"

echo "[v1-gate] pr review demo smoke"
"$pr_review_demo_smoke_script"

if [[ "$run_perf_gate" == "1" ]]; then
  echo "[v1-gate] perf gate"
  "$perf_gate_script"
else
  echo "[v1-gate] perf gate skipped; set SYNAPSE_RELEASE_RUN_PERF_GATE=1 to enable"
fi

echo "[v1-gate] completed"
