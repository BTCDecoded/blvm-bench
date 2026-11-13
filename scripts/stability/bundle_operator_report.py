#!/usr/bin/env python3
"""Bundle an operator report and strip secrets.

The archive contains the node log, the config with secret values replaced,
and a short system description. It does not contain cookie files, wallets,
or private keys.
"""

from __future__ import annotations

import argparse
import os
import re
import tarfile
import tempfile
from pathlib import Path

SECRET_KEYS = re.compile(
    r"(?im)^(\s*(?:password|token|api_key|rpcpassword|rpc_password|auth_token)\s*=\s*)(.+)$"
)
BEARER = re.compile(r"Bearer\s+\S+")
PRIVATE_KEY = re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----", re.S)


def strip_text(text: str) -> str:
    text = SECRET_KEYS.sub(r"\1[REDACTED]", text)
    text = BEARER.sub("Bearer [REDACTED]", text)
    text = PRIVATE_KEY.sub("[REDACTED PRIVATE KEY]", text)
    return text


def system_info() -> str:
    uname = os.uname()
    mem = Path("/proc/meminfo").read_text().splitlines()[:3]
    return "\n".join(
        [
            f"sysname={uname.sysname}",
            f"release={uname.release}",
            f"machine={uname.machine}",
            *mem,
            "",
        ]
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", type=Path)
    parser.add_argument("--config", type=Path)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        sample = 'password = "super-secret"\nAuthorization: Bearer abcdef\n'
        cleaned = strip_text(sample)
        assert "super-secret" not in cleaned
        assert "abcdef" not in cleaned
        assert "[REDACTED]" in cleaned
        print("bundle self-test passed")
        return 0
    if args.log is None or args.out is None:
        parser.error("--log and --out are required")
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "system.txt").write_text(system_info())
        (root / "node.log").write_text(strip_text(args.log.read_text(errors="replace")))
        if args.config:
            (root / "config.toml").write_text(strip_text(args.config.read_text(errors="replace")))
        with tarfile.open(args.out, "w:gz") as archive:
            for path in root.iterdir():
                archive.add(path, arcname=path.name)
    print(args.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
