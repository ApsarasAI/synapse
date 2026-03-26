#!/usr/bin/env python3
import argparse
import json
import statistics
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from typing import Dict, List, Tuple


def run_once(url: str, body: bytes, headers: Dict[str, str], timeout: float) -> Tuple[bool, float]:
    started = time.perf_counter()
    request = urllib.request.Request(url, data=body, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            response.read()
            ok = 200 <= response.status < 300
    except urllib.error.HTTPError as error:
        error.read()
        ok = False
    except Exception:
        ok = False
    return ok, (time.perf_counter() - started) * 1000.0


def percentile(values: List[float], ratio: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, int(round((len(ordered) - 1) * ratio))))
    return ordered[index]


def main() -> int:
    parser = argparse.ArgumentParser(description="Minimal HTTP benchmark for Synapse execute API")
    parser.add_argument("--url", default="http://127.0.0.1:8080/execute")
    parser.add_argument("--requests", type=int, default=20)
    parser.add_argument("--concurrency", type=int, default=4)
    parser.add_argument("--timeout", type=float, default=10.0)
    parser.add_argument("--token")
    parser.add_argument("--tenant-id", default="default")
    parser.add_argument("--code", default="print('bench')\\n")
    args = parser.parse_args()

    payload = json.dumps(
        {
            "language": "python",
            "code": args.code,
            "timeout_ms": 5000,
            "memory_limit_mb": 128,
            "tenant_id": args.tenant_id,
        }
    ).encode()
    headers = {"content-type": "application/json", "x-synapse-tenant-id": args.tenant_id}
    if args.token:
        headers["authorization"] = f"Bearer {args.token}"

    started = time.perf_counter()
    latencies: List[float] = []
    errors = 0
    lock = threading.Lock()
    with ThreadPoolExecutor(max_workers=args.concurrency) as executor:
        futures = [
            executor.submit(run_once, args.url, payload, headers, args.timeout)
            for _ in range(args.requests)
        ]
        for future in as_completed(futures):
            ok, latency_ms = future.result()
            with lock:
                latencies.append(latency_ms)
                if not ok:
                    errors += 1
    total_s = max(time.perf_counter() - started, 0.001)

    summary = {
        "requests": args.requests,
        "concurrency": args.concurrency,
        "errors": errors,
        "error_rate": errors / max(args.requests, 1),
        "throughput_rps": args.requests / total_s,
        "p50_ms": percentile(latencies, 0.50),
        "p95_ms": percentile(latencies, 0.95),
        "mean_ms": statistics.fmean(latencies) if latencies else 0.0,
    }
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0 if errors == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
