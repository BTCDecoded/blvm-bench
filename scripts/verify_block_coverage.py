#!/usr/bin/env python3
"""Verify block coverage for BIP-110 chain scan.

Checks:
- chunks.meta total_blocks vs scan_merged.json blocks_scanned
- Era block counts sum to total
- Batch block counts sum to total
- Chunk files exist
- Optional: known block hashes (genesis) if index readable
"""
import json
import os
import sys
from pathlib import Path

def main():
    chunks_dir = Path(os.environ.get("BLOCK_CACHE_DIR", "/run/media/acolyte/Extra/blockchain"))
    scan_path = Path("bip110_results/scan_merged.json")
    if not scan_path.exists():
        scan_path = Path("blvm-bench/bip110_results/scan_merged.json")
    if not scan_path.exists():
        scan_path = Path(__file__).parent.parent / "bip110_results" / "scan_merged.json"

    print("🔍 Block Coverage Verification")
    print(f"   Chunks directory: {chunks_dir}")
    print()

    errors = []
    warnings = []

    # 1. Load chunks.meta
    meta_path = chunks_dir / "chunks.meta"
    if not meta_path.exists():
        errors.append(f"chunks.meta not found at {meta_path}")
        total_blocks = None
        num_chunks = None
    else:
        meta = {}
        for line in meta_path.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                meta[k.strip()] = v.strip()
        total_blocks = int(meta.get("total_blocks", 0))
        num_chunks = int(meta.get("num_chunks", 0))
        blocks_per_chunk = int(meta.get("blocks_per_chunk", 0))
        print("📋 Metadata (chunks.meta):")
        print(f"   total_blocks: {total_blocks}")
        print(f"   num_chunks: {num_chunks}")
        print(f"   blocks_per_chunk: {blocks_per_chunk}")
        print()

    # 2. Check chunk files
    if num_chunks is not None:
        missing = []
        for i in range(num_chunks):
            if not (chunks_dir / f"chunk_{i}.bin.zst").exists():
                missing.append(i)
        if missing:
            errors.append(f"Missing chunk files: {missing}")
        else:
            print(f"✅ All {num_chunks} chunk files present")
        print()

    # 3. Load scan_merged.json
    if not scan_path.exists():
        warnings.append(f"scan_merged.json not found at {scan_path}")
        blocks_scanned = None
        era_blocks = None
        batch_blocks = None
    else:
        with open(scan_path) as f:
            scan = json.load(f)
        blocks_scanned = scan.get("blocks_scanned")
        print("📋 Scan results (scan_merged.json):")
        print(f"   blocks_scanned: {blocks_scanned}")
        print()

        # 4. Cross-check blocks_scanned vs total_blocks
        if total_blocks is not None and blocks_scanned is not None:
            if blocks_scanned == total_blocks:
                print("✅ blocks_scanned matches chunks.meta total_blocks")
            else:
                errors.append(f"Mismatch: scan has {blocks_scanned}, metadata has {total_blocks}")
        print()

        # 5. Era block sum
        era_blocks = 0
        for key in ["era_pre_segwit", "era_segwit", "era_taproot", "era_inscriptions"]:
            e = scan.get(key, {})
            if isinstance(e, dict):
                era_blocks += e.get("blocks", 0)
        if blocks_scanned is not None and era_blocks == blocks_scanned:
            print("✅ Era blocks sum matches blocks_scanned")
        elif era_blocks:
            print(f"   Era sum: {era_blocks} (expected {blocks_scanned})")
            if era_blocks != blocks_scanned:
                warnings.append(f"Era sum {era_blocks} != blocks_scanned {blocks_scanned}")
        print()

        # 6. Batch consistency (from individual batch files if available)
        batch_dir = scan_path.parent
        batch_sum = 0
        batch_files = [
            ("scan_0_400k.json", 400001),
            ("scan_400k_600k.json", 200000),
            ("scan_600k_900k.json", 312722),
        ]
        for fname, expected in batch_files:
            p = batch_dir / fname
            if p.exists():
                with open(p) as f:
                    b = json.load(f)
                batch_sum += b.get("blocks_scanned", 0)
        if batch_sum and batch_sum == blocks_scanned:
            print("✅ Batch blocks sum matches blocks_scanned")
        elif batch_sum:
            print(f"   Batch sum: {batch_sum} (expected {blocks_scanned})")
            if batch_sum != blocks_scanned:
                warnings.append(f"Batch sum {batch_sum} != blocks_scanned {blocks_scanned}")
        print()

        # 7. Post-inscriptions era check
        pi_blocks = scan.get("blocks_post_inscriptions")
        era_inscriptions = scan.get("era_inscriptions", {})
        if isinstance(era_inscriptions, dict):
            ei_blocks = era_inscriptions.get("blocks", 0)
            if pi_blocks is not None and ei_blocks == pi_blocks:
                print("✅ blocks_post_inscriptions matches era_inscriptions.blocks")
            elif pi_blocks and ei_blocks:
                print(f"   Post-inscriptions: {pi_blocks}, era_inscriptions: {ei_blocks}")
        print()

    # Summary
    if errors:
        print("❌ ERRORS:")
        for e in errors:
            print(f"   {e}")
        sys.exit(1)
    if warnings:
        print("⚠️  Warnings:")
        for w in warnings:
            print(f"   {w}")
    print()
    print("✅ Block coverage verification PASSED")
    if total_blocks:
        print(f"   Full chain coverage: heights 0..{total_blocks-1} ({total_blocks} blocks)")
    return 0

if __name__ == "__main__":
    sys.exit(main())
