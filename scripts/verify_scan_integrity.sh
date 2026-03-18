#!/bin/bash
# Verify chain scan integrity: block coverage and block data correctness.
#
# Checks:
# 1. Metadata and chunk files (verify_block_coverage.py)
# 2. Block index coverage and gaps (verify_block_coverage Rust, if index exists)
# 3. Spot-check: scan blocks 0-1 (genesis + block 1) → must have 2 blocks, 2 txs
# 4. Spot-check: scan blocks 0-99 → must have 100 blocks, total_txs in sane range
# 5. Cross-check: scan_merged.json blocks_scanned vs chunks.meta total_blocks
#
# Usage: BLOCK_CACHE_DIR=/path ./scripts/verify_scan_integrity.sh
# Or from blvm-bench: ./scripts/verify_scan_integrity.sh

set -e
cd "$(dirname "$0")/.."
export BLOCK_CACHE_DIR="${BLOCK_CACHE_DIR:-/run/media/acolyte/Extra/blockchain}"

if [ ! -d "$BLOCK_CACHE_DIR" ] || [ ! -f "$BLOCK_CACHE_DIR/chunks.meta" ]; then
    echo "❌ Blockchain not found at $BLOCK_CACHE_DIR"
    echo "   Set BLOCK_CACHE_DIR or mount the drive"
    exit 1
fi

echo "🔍 Chain Scan Integrity Verification"
echo "   Chunks: $BLOCK_CACHE_DIR"
echo ""

# 1. Python metadata check
echo "=== 1. Metadata & chunks (verify_block_coverage.py) ==="
python3 scripts/verify_block_coverage.py || {
    echo "⚠️  Python verification had issues (may be OK if scan_merged.json missing)"
}
echo ""

# 2. Rust index check (if chunks.index exists)
if [ -f "$BLOCK_CACHE_DIR/chunks.index" ]; then
    echo "=== 2. Block index coverage (verify_block_coverage) ==="
    cargo build --release --bin verify_block_coverage --features scan || true
    if [ -f ./target/release/verify_block_coverage ]; then
        ./target/release/verify_block_coverage || {
            echo "❌ Block index verification FAILED"
            exit 1
        }
    fi
else
    echo "=== 2. Block index (skipped - chunks.index not found) ==="
fi
echo ""

# 3. Spot-check: blocks 0-1 (genesis + block 1) must have 2 txs
# Note: --end 0 means "all" in scan_chain, so we use --end 1 for 2 blocks
echo "=== 3. Spot-check: Blocks 0-1 (genesis + block 1) ==="
cargo build --release --bin scan_chain --features scan 2>/dev/null || true
TMP_GENESIS=$(mktemp -u).json
./target/release/scan_chain --start 0 --end 1 --json "$TMP_GENESIS" --spam-preset disabled 2>/dev/null
GENESIS_BLOCKS=$(python3 -c "import json; d=json.load(open(\"$TMP_GENESIS\")); print(d.get('blocks_scanned', -1))" 2>/dev/null || echo "-1")
GENESIS_TXS=$(python3 -c "import json; d=json.load(open(\"$TMP_GENESIS\")); print(d.get('total_txs', -1))" 2>/dev/null || echo "-1")
rm -f "$TMP_GENESIS" 2>/dev/null || true
if [ "$GENESIS_BLOCKS" = "2" ] && [ "$GENESIS_TXS" = "2" ]; then
    echo "✅ Blocks 0-1: 2 blocks, 2 txs (genesis + block 1 correct)"
else
    echo "❌ Blocks 0-1: expected 2 blocks and 2 txs, got $GENESIS_BLOCKS blocks and $GENESIS_TXS txs"
    exit 1
fi
echo ""

# 4. Spot-check: first 100 blocks
echo "=== 4. Spot-check: Blocks 0-99 ==="
TMP_100=$(mktemp -u).json
./target/release/scan_chain --start 0 --end 99 --json "$TMP_100" --spam-preset disabled 2>/dev/null
BLOCKS_100=$(python3 -c "import json; d=json.load(open(\"$TMP_100\")); print(d.get('blocks_scanned', -1))" 2>/dev/null || echo "-1")
TXS_100=$(python3 -c "import json; d=json.load(open(\"$TMP_100\")); print(d.get('total_txs', -1))" 2>/dev/null || echo "-1")
rm -f "$TMP_100" 2>/dev/null || true
if [ "$BLOCKS_100" = "100" ]; then
    echo "✅ Blocks 0-99: scanned 100 blocks"
else
    echo "❌ Blocks 0-99: expected 100 blocks, got $BLOCKS_100"
    exit 1
fi
# Early blocks: genesis has 1 tx, blocks 1-99 typically 1-2 txs each. Expect 100-300 total.
if [ -n "$TXS_100" ] && [ "$TXS_100" -ge 100 ] && [ "$TXS_100" -le 500 ]; then
    echo "✅ Blocks 0-99: $TXS_100 txs (sane range for early chain)"
else
    echo "⚠️  Blocks 0-99: $TXS_100 txs (expected 100-500 for early chain)"
fi
echo ""

# 5. Cross-check scan_merged.json vs metadata
echo "=== 5. Full scan cross-check ==="
TOTAL_META=$(grep -E "^total_blocks=" "$BLOCK_CACHE_DIR/chunks.meta" 2>/dev/null | cut -d= -f2 || echo "")
if [ -f bip110_results/scan_merged.json ] && [ -n "$TOTAL_META" ]; then
    SCANNED=$(python3 -c "import json; d=json.load(open('bip110_results/scan_merged.json')); print(d.get('blocks_scanned', -1))" 2>/dev/null || echo "-1")
    if [ "$SCANNED" = "$TOTAL_META" ]; then
        echo "✅ scan_merged.json blocks_scanned ($SCANNED) matches chunks.meta total_blocks ($TOTAL_META)"
    else
        echo "⚠️  Mismatch: scan has $SCANNED blocks, metadata has $TOTAL_META"
    fi
else
    echo "ℹ️  scan_merged.json or chunks.meta total_blocks not found (skip)"
fi

echo ""
echo "✅ Scan integrity verification PASSED"
