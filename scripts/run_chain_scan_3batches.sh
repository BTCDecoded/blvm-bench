#!/bin/bash
# Run chain scan in 3 batches and merge results.
# Requires: BLOCK_CACHE_DIR or /run/media/acolyte/Extra/blockchain (mount external drive)
# Usage: BLOCK_CACHE_DIR=/path ./scripts/run_chain_scan_3batches.sh

set -e
cd "$(dirname "$0")/.."
mkdir -p bip110_results

# Use BLOCK_CACHE_DIR if set; else try default external drive
export BLOCK_CACHE_DIR="${BLOCK_CACHE_DIR:-/run/media/acolyte/Extra/blockchain}"
if [ ! -d "$BLOCK_CACHE_DIR" ] || [ ! -f "$BLOCK_CACHE_DIR/chunks.meta" ]; then
    echo "❌ Blockchain not found. Set BLOCK_CACHE_DIR or mount $BLOCK_CACHE_DIR"
    echo "   Need: chunks.meta and chunk_*.bin.zst files"
    exit 1
fi
echo "📦 Using blockchain: $BLOCK_CACHE_DIR"

BIN="./target/release/scan_chain"
echo "Building scan_chain (release)..."
cargo build --release --bin scan_chain --features scan

echo "=== Batch 1: 0–400k (strict preset) ==="
$BIN --start 0 --end 400000 --json bip110_results/scan_0_400k.json --spam-preset strict --batch-size 64

echo ""
echo "=== Batch 2: 400k–600k (strict preset) ==="
$BIN --start 400001 --end 600000 --json bip110_results/scan_400k_600k.json --spam-preset strict --batch-size 64

echo ""
echo "=== Batch 3: 600k–912723 (strict preset) ==="
$BIN --start 600001 --end 912723 --json bip110_results/scan_600k_900k.json --spam-preset strict --batch-size 64

echo ""
echo "=== Merging ==="
python3 scripts/merge_scan_json.py \
    bip110_results/scan_0_400k.json \
    bip110_results/scan_400k_600k.json \
    bip110_results/scan_600k_900k.json \
    -o bip110_results/scan_merged.json

echo ""
echo "Done. Results: bip110_results/scan_merged.json"
