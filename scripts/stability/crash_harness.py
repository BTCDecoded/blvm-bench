#!/usr/bin/env python3
"""Kill blvm with SIGKILL and check that a restart keeps the chain tip.

The beta gate is 50 kill cycles during IBD and 50 during steady state.
This script records every cycle. It exits 2 when the binary or the chain
is not available, and exits 1 when a cycle loses the tip or the UTXO hash.

A cycle compares `getblockcount` and, when the RPC returns it, the
`gettxoutsetinfo` hash, against the values from before the kill.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

GATE_IBD_CYCLES = 50
GATE_STEADY_CYCLES = 50
SECRET_MARKERS = ("password", "api_key", "rpc-token-", "BEGIN PRIVATE")


def strip_secrets(text: str, token: str) -> str:
    redacted = text.replace(token, "[REDACTED]")
    redacted = redacted.replace(f"Bearer {token}", "Bearer [REDACTED]")
    return redacted


def rpc(url: str, token: str, method: str) -> dict:
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": []}).encode()
    request = urllib.request.Request(
        url,
        data=body,
        headers={
            "content-type": "application/json",
            "authorization": f"Bearer {token}",
        },
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        payload = json.loads(response.read().decode())
    if "error" in payload and payload["error"]:
        raise RuntimeError(f"{method} failed: {payload['error']}")
    return payload["result"]


def chain_snapshot(url: str, token: str) -> dict:
    info = rpc(url, token, "getblockchaininfo")
    blocks = info.get("blocks", info.get("height"))
    snapshot = {"blocks": blocks, "utxo_hash": None}
    try:
        utxo = rpc(url, token, "gettxoutsetinfo")
        snapshot["utxo_hash"] = utxo.get("hash") or utxo.get("hash_serialized")
    except Exception as err:  # noqa: BLE001 - a missing method is recorded, not fatal
        snapshot["utxo_error"] = str(err)
    return snapshot


def same_chain(before: dict, after: dict) -> str | None:
    if before.get("blocks") != after.get("blocks"):
        return f"tip changed from {before.get('blocks')} to {after.get('blocks')}"
    if before.get("utxo_hash") and after.get("utxo_hash") and before["utxo_hash"] != after["utxo_hash"]:
        return "utxo hash changed"
    return None


def write_config(directory: Path, token: str, profile: Path | None) -> Path:
    if profile and profile.exists():
        text = profile.read_text()
        text = text.replace('protocol_version = "mainnet"', 'protocol_version = "regtest"')
        text = text.replace("required = true", f'required = true\ntokens = ["{token}"]', 1)
    else:
        text = "\n".join(
            [
                'protocol_version = "regtest"',
                "listen = false",
                'listen_addr = "127.0.0.1:18444"',
                'rpc_addr = "127.0.0.1:18443"',
                "[rpc_auth]",
                "required = true",
                f'tokens = ["{token}"]',
                "",
            ]
        )
    path = directory / "blvm.toml"
    path.write_text(text)
    os.chmod(path, 0o600)
    return path


def start_node(binary: Path, data_dir: Path, config: Path, log_path: Path) -> subprocess.Popen:
    log = log_path.open("ab")
    return subprocess.Popen(
        [
            str(binary),
            "--network",
            "regtest",
            "--data-dir",
            str(data_dir),
            "--config",
            str(config),
            "--noassumevalid",
        ],
        stdout=log,
        stderr=subprocess.STDOUT,
    )


def wait_rpc(url: str, token: str, timeout: float) -> dict:
    deadline = time.time() + timeout
    last = "rpc did not answer"
    while time.time() < deadline:
        try:
            return chain_snapshot(url, token)
        except Exception as err:  # noqa: BLE001
            last = str(err)
            time.sleep(0.5)
    raise RuntimeError(last)


def stop(process: subprocess.Popen) -> None:
    if process.poll() is not None:
        return
    process.send_signal(signal.SIGKILL)
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=10)


def run_cycles(args: argparse.Namespace, mode: str, cycles: int, log_file) -> int:
    token = "rpc-token-" + os.urandom(16).hex()
    work = Path(tempfile.mkdtemp(prefix=f"blvm-crash-{mode}-"))
    config = write_config(work, token, Path(args.profile) if args.profile else None)
    data = work / "data"
    data.mkdir()
    url = args.rpc_url
    failures = 0
    process = None
    try:
        process = start_node(Path(args.binary), data, config, work / "node.log")
        before = wait_rpc(url, token, args.startup_timeout)
        for index in range(cycles):
            delay = random.uniform(args.min_delay, args.max_delay)
            time.sleep(delay)
            if process.poll() is not None:
                failures += 1
                log_file.write(f"{mode} cycle {index}: node exited before the kill\n")
                break
            stop(process)
            process = start_node(Path(args.binary), data, config, work / "node.log")
            try:
                after = wait_rpc(url, token, args.startup_timeout)
            except Exception as err:  # noqa: BLE001
                failures += 1
                log_file.write(f"{mode} cycle {index}: restart failed: {strip_secrets(str(err), token)}\n")
                break
            mismatch = same_chain(before, after)
            node_log = (work / "node.log").read_text(errors="replace")
            if token in node_log or any(marker in node_log for marker in SECRET_MARKERS if marker != "rpc-token-"):
                failures += 1
                log_file.write(f"{mode} cycle {index}: a secret appeared in the node log\n")
            if mismatch:
                failures += 1
                log_file.write(
                    f"{mode} cycle {index}: {mismatch} after {delay:.2f}s delay; "
                    f"data_dir={data}\n"
                )
                break
            before = after
            log_file.write(f"{mode} cycle {index}: ok tip={after.get('blocks')}\n")
    except Exception as err:  # noqa: BLE001
        failures += 1
        log_file.write(f"{mode}: setup failed: {strip_secrets(str(err), token)}\n")
    finally:
        if process is not None:
            stop(process)
        if failures and args.keep_failures:
            log_file.write(f"{mode}: kept {work}\n")
        else:
            shutil.rmtree(work, ignore_errors=True)
    return failures


def self_test() -> int:
    token = "rpc-token-abcdef"
    text = f"Authorization: Bearer {token} password=nope"
    cleaned = strip_secrets(text, token)
    assert token not in cleaned
    assert "Bearer [REDACTED]" in cleaned
    assert same_chain({"blocks": 3, "utxo_hash": "aa"}, {"blocks": 3, "utxo_hash": "aa"}) is None
    assert "tip changed" in (same_chain({"blocks": 3}, {"blocks": 4}) or "")
    assert "utxo hash" in (
        same_chain({"blocks": 1, "utxo_hash": "aa"}, {"blocks": 1, "utxo_hash": "bb"}) or ""
    )
    print("crash harness self-test passed")
    print(f"gate: {GATE_IBD_CYCLES} IBD cycles and {GATE_STEADY_CYCLES} steady-state cycles")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.environ.get("BLVM_BIN", ""))
    parser.add_argument("--profile", default="")
    parser.add_argument("--rpc-url", default="http://127.0.0.1:18443")
    parser.add_argument("--ibd-cycles", type=int, default=0)
    parser.add_argument("--steady-cycles", type=int, default=0)
    parser.add_argument("--min-delay", type=float, default=0.5)
    parser.add_argument("--max-delay", type=float, default=3.0)
    parser.add_argument("--startup-timeout", type=float, default=30.0)
    parser.add_argument("--keep-failures", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--log", default="")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if not args.binary or not Path(args.binary).is_file():
        print("crash harness: no blvm binary. Set --binary or BLVM_BIN. Exit 2.", file=sys.stderr)
        print(f"gate remains {GATE_IBD_CYCLES}+{GATE_STEADY_CYCLES} cycles, 0 executed.", file=sys.stderr)
        return 2
    log_path = Path(args.log) if args.log else Path(tempfile.gettempdir()) / "blvm-crash-harness.log"
    with log_path.open("a", encoding="utf-8") as log_file:
        log_file.write(f"binary={args.binary} ibd={args.ibd_cycles} steady={args.steady_cycles}\n")
        failures = 0
        if args.ibd_cycles:
            failures += run_cycles(args, "ibd", args.ibd_cycles, log_file)
        if args.steady_cycles:
            failures += run_cycles(args, "steady", args.steady_cycles, log_file)
    print(f"crash harness log: {log_path}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
