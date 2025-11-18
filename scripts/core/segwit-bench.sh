#!/bin/bash
# Bitcoin Core SegWit Operations Benchmark (Portable)

set -e

# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"

# Bitcoin Core SegWit Operations Benchmark
# Uses bench_bitcoin to benchmark SegWit block validation (ConnectBlockAllEcdsa/AllSchnorr)



# Reliably find or build bench_bitcoin
BENCH_BITCOIN=$(get_bench_bitcoin)

echo "Running bench_bitcoin for SegWit operations (this may take 1-2 minutes)..."
echo "This benchmarks SegWit block validation (ConnectBlockAllEcdsa/AllSchnorr)"

# Run bench_bitcoin and capture output
BENCH_OUTPUT=$("$BENCH_BITCOIN" 2>&1 || echo "")

# Extract SegWit-related benchmark results
CONNECT_BLOCK_ALL_SCHNORR=$(echo "$BENCH_OUTPUT" | grep -E "ConnectBlockAllSchnorr" | head -1 || echo "")
CONNECT_BLOCK_ALL_ECDSA=$(echo "$BENCH_OUTPUT" | grep -E "ConnectBlockAllEcdsa" | head -1 || echo "")
CONNECT_BLOCK_MIXED=$(echo "$BENCH_OUTPUT" | grep -E "ConnectBlockMixedEcdsaSchnorr" | head -1 || echo "")

# Parse bench_bitcoin output
parse_bench_bitcoin() {
    local line="$1"
    if [ -z "$line" ]; then
        echo "0|0"
        return
    fi
    time_ns=$(echo "$line" | awk -F'|' '{gsub(/[^0-9.]/,"",$2); print $2}' 2>/dev/null || echo "0")
    ops_per_sec=$(echo "$line" | awk -F'|' '{gsub(/[^0-9.]/,"",$3); print $3}' 2>/dev/null || echo "0")
    echo "${time_ns}|${ops_per_sec}"
}

ALL_SCHNORR_DATA=$(parse_bench_bitcoin "$CONNECT_BLOCK_ALL_SCHNORR")
ALL_SCHNORR_TIME_NS=$(echo "$ALL_SCHNORR_DATA" | cut -d'|' -f1)
ALL_SCHNORR_OPS=$(echo "$ALL_SCHNORR_DATA" | cut -d'|' -f2)

ALL_ECDSA_DATA=$(parse_bench_bitcoin "$CONNECT_BLOCK_ALL_ECDSA")
ALL_ECDSA_TIME_NS=$(echo "$ALL_ECDSA_DATA" | cut -d'|' -f1)
ALL_ECDSA_OPS=$(echo "$ALL_ECDSA_DATA" | cut -d'|' -f2)

MIXED_DATA=$(parse_bench_bitcoin "$CONNECT_BLOCK_MIXED")
MIXED_TIME_NS=$(echo "$MIXED_DATA" | cut -d'|' -f1)
MIXED_OPS=$(echo "$MIXED_DATA" | cut -d'|' -f2)

# Convert to milliseconds
ALL_SCHNORR_TIME_MS=$(awk "BEGIN {printf \"%.6f\", $ALL_SCHNORR_TIME_NS / 1000000}" 2>/dev/null || echo "0")
ALL_ECDSA_TIME_MS=$(awk "BEGIN {printf \"%.6f\", $ALL_ECDSA_TIME_NS / 1000000}" 2>/dev/null || echo "0")
MIXED_TIME_MS=$(awk "BEGIN {printf \"%.6f\", $MIXED_TIME_NS / 1000000}" 2>/dev/null || echo "0")

# Generate JSON output
cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "bitcoin_core_segwit_operations": {
    "connect_block_all_schnorr": {
      "time_per_block_ns": $ALL_SCHNORR_TIME_NS,
      "time_per_block_ms": $ALL_SCHNORR_TIME_MS,
      "blocks_per_second": $ALL_SCHNORR_OPS,
      "implementation": "Chainstate::ConnectBlock (all Schnorr/Taproot signatures)",
      "note": "SegWit v1 (Taproot) block validation"
    },
    "connect_block_all_ecdsa": {
      "time_per_block_ns": $ALL_ECDSA_TIME_NS,
      "time_per_block_ms": $ALL_ECDSA_TIME_MS,
      "blocks_per_second": $ALL_ECDSA_OPS,
      "implementation": "Chainstate::ConnectBlock (all ECDSA/SegWit v0 signatures)",
      "note": "SegWit v0 block validation"
    },
    "connect_block_mixed": {
      "time_per_block_ns": $MIXED_TIME_NS,
      "time_per_block_ms": $MIXED_TIME_MS,
      "blocks_per_second": $MIXED_OPS,
      "implementation": "Chainstate::ConnectBlock (mixed ECDSA/Schnorr)",
      "note": "Mixed SegWit v0 and v1 block validation"
    },
    "primary_comparison": {
      "time_per_block_ms": $MIXED_TIME_MS,
      "time_per_block_ns": $MIXED_TIME_NS,
      "blocks_per_second": $MIXED_OPS,
      "note": "Primary metric for comparison (mixed, most realistic)"
    },
    "measurement_method": "bench_bitcoin - Core's actual ConnectBlock implementation with SegWit",
    "comparison_note": "This measures actual SegWit block validation - comparable to Commons' segwit_operations benchmark"
  }
}
EOF

echo "Results saved to: $OUTPUT_FILE"
echo "Primary (mixed): $MIXED_TIME_MS ms per block ($MIXED_OPS blocks/sec)"
echo "All Schnorr: $ALL_SCHNORR_TIME_MS ms per block ($ALL_SCHNORR_OPS blocks/sec)"
echo "All ECDSA: $ALL_ECDSA_TIME_MS ms per block ($ALL_ECDSA_OPS blocks/sec)"


