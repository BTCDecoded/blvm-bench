#!/usr/bin/env python3
"""Sample RSS and disk for a running process.

A steady-state report is unbounded when the last 12 hour median RSS is more
than twice the first 12 hour median, or when files other than block and
chainstate data grow by more than 2 GiB over 7 days. This script only
samples. It does not decide that a short sample is a soak.
"""

from __future__ import annotations

import argparse
import os
import shutil
import time
from pathlib import Path


def rss_kib(pid: int) -> int:
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        if line.startswith("VmRSS:"):
            return int(line.split()[1])
    raise RuntimeError(f"VmRSS missing for {pid}")


def disk_bytes(path: Path) -> int:
    total = 0
    if not path.exists():
        return 0
    for root, _dirs, files in os.walk(path):
        for name in files:
            total += (Path(root) / name).stat().st_size
    return total


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, default=os.getpid())
    parser.add_argument("--path", default=".")
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--interval", type=float, default=1.0)
    parser.add_argument("--label", default="self-test")
    args = parser.parse_args()
    rows = []
    for _ in range(args.samples):
        rows.append((rss_kib(args.pid), disk_bytes(Path(args.path))))
        time.sleep(args.interval)
    rss_values = [row[0] for row in rows]
    disk_values = [row[1] for row in rows]
    print(f"label={args.label} pid={args.pid} path={args.path}")
    print(f"samples={len(rows)} interval_s={args.interval}")
    print(f"rss_kib min={min(rss_values)} max={max(rss_values)} last={rss_values[-1]}")
    print(f"disk_bytes min={min(disk_values)} max={max(disk_values)} last={disk_values[-1]}")
    print("thresholds: 72h steady state, last-12h median RSS <= 2x first-12h median;")
    print("side files other than blocks and chainstate <= 2 GiB growth over 7 days.")
    print("A sample shorter than 72 hours does not meet the beta gate.")
    usage = shutil.disk_usage(args.path)
    print(f"filesystem_free_bytes={usage.free}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
