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

echo "[v1-gate] cargo fmt"
cargo fmt --all --check

echo "[v1-gate] cargo clippy"
cargo clippy --workspace --all-targets -- -D warnings

echo "[v1-gate] cargo test"
cargo test --workspace

echo "[v1-gate] python sdk tests"
PYTHONPATH="sdk/python/src${PYTHONPATH:+:$PYTHONPATH}" \
  "$python_bin" -m unittest discover -s sdk/python/tests

echo "[v1-gate] quickstart smoke"
scripts/quickstart_smoke.sh

echo "[v1-gate] python sdk import smoke"
"$python_bin" -c "import pathlib; exec(compile(pathlib.Path('examples/pr-review-agent/run_demo.py').read_text(), 'examples/pr-review-agent/run_demo.py', 'exec'), {'__name__': 'synapse_demo_smoke', '__file__': 'examples/pr-review-agent/run_demo.py'})"

echo "[v1-gate] pr review demo smoke"
scripts/pr_review_demo_smoke.sh

echo "[v1-gate] completed"
