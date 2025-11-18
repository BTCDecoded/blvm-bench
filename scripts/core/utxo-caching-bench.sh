#!/bin/bash
# Bitcoin Core UTXO Caching Benchmark
# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"
# Measures CCoinsCaching performance using bench_bitcoin
# Fair comparison with Commons UTXO caching

set -e

OUTPUT_DIR="${1:-$(dirname "$0")/../results}"
OUTPUT_DIR=$(cd "$OUTPUT_DIR" && pwd)
mkdir -p "$OUTPUT_DIR"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CORE_DIR="$PROJECT_ROOT/core"
# Reliably find or build bench_bitcoin
BENCH_BITCOIN=$(get_bench_bitcoin)

echo "Running bench_bitcoin for UTXO operations (this may take 1-2 minutes)..."
echo "This benchmarks UTXO insert/get/remove operations (matches Commons' utxo_insert/utxo_get/utxo_remove)."

# Run bench_bitcoin and capture output for all UTXO operations
# Run all three benchmarks separately and combine output
BENCH_OUTPUT=$("$BENCH_BITCOIN" -filter="UTXOInsert|UTXOGet|UTXORemove" 2>&1 || echo "")

# Parse bench_bitcoin output
# Format: "| 163.32 | 6,123,018.76 | 1.1% | 1,763.00 | 598.70 | 2.945 | 93.00 | 0.0% | 0.01 | `UTXOInsert`"
# Column 1: median time in ns, Column 2: ops/sec
parse_bench_bitcoin() {
    local line="$1"
    if [ -z "$line" ]; then
        echo "0|0"
        return
    fi
    # Extract median time (first column after the leading |)
    # Remove commas and extract the number
    time_ns=$(echo "$line" | awk -F'|' '{gsub(/[^0-9.]/,"",$2); print $2}' 2>/dev/null || echo "0")
    # Extract ops/sec (second column after the leading |)
    ops_per_sec=$(echo "$line" | awk -F'|' '{gsub(/[^0-9.]/,"",$3); print $3}' 2>/dev/null || echo "0")
    echo "${time_ns}|${ops_per_sec}"
}

UTXO_INSERT_LINE=$(echo "$BENCH_OUTPUT" | grep -iE "UTXOInsert" | head -1 || echo "")
UTXO_GET_LINE=$(echo "$BENCH_OUTPUT" | grep -iE "UTXOGet" | head -1 || echo "")
UTXO_REMOVE_LINE=$(echo "$BENCH_OUTPUT" | grep -iE "UTXORemove" | head -1 || echo "")

UTXO_INSERT_DATA=$(parse_bench_bitcoin "$UTXO_INSERT_LINE")
UTXO_GET_DATA=$(parse_bench_bitcoin "$UTXO_GET_LINE")
UTXO_REMOVE_DATA=$(parse_bench_bitcoin "$UTXO_REMOVE_LINE")

UTXO_INSERT_TIME_NS=$(echo "$UTXO_INSERT_DATA" | cut -d'|' -f1)
UTXO_INSERT_OPS=$(echo "$UTXO_INSERT_DATA" | cut -d'|' -f2)
UTXO_GET_TIME_NS=$(echo "$UTXO_GET_DATA" | cut -d'|' -f1)
UTXO_GET_OPS=$(echo "$UTXO_GET_DATA" | cut -d'|' -f2)
UTXO_REMOVE_TIME_NS=$(echo "$UTXO_REMOVE_DATA" | cut -d'|' -f1)
UTXO_REMOVE_OPS=$(echo "$UTXO_REMOVE_DATA" | cut -d'|' -f2)

BENCHMARKS="[]"

if [ "$UTXO_INSERT_TIME_NS" != "0" ] && [ -n "$UTXO_INSERT_TIME_NS" ]; then
    TIME_MS=$(awk "BEGIN {printf \"%.6f\", $UTXO_INSERT_TIME_NS / 1000000}" 2>/dev/null || echo "0")
    BENCHMARKS=$(echo "$BENCHMARKS" | jq --arg name "UTXOInsert" --arg time "$TIME_MS" --arg timens "$UTXO_INSERT_TIME_NS" --arg ops "$UTXO_INSERT_OPS" '. += [{"name": $name, "time_ms": ($time | tonumber), "time_ns": ($timens | tonumber), "ops_per_sec": ($ops | tonumber)}]' 2>/dev/null || echo "$BENCHMARKS")
fi

if [ "$UTXO_GET_TIME_NS" != "0" ] && [ -n "$UTXO_GET_TIME_NS" ]; then
    TIME_MS=$(awk "BEGIN {printf \"%.6f\", $UTXO_GET_TIME_NS / 1000000}" 2>/dev/null || echo "0")
    BENCHMARKS=$(echo "$BENCHMARKS" | jq --arg name "UTXOGet" --arg time "$TIME_MS" --arg timens "$UTXO_GET_TIME_NS" --arg ops "$UTXO_GET_OPS" '. += [{"name": $name, "time_ms": ($time | tonumber), "time_ns": ($timens | tonumber), "ops_per_sec": ($ops | tonumber)}]' 2>/dev/null || echo "$BENCHMARKS")
fi

if [ "$UTXO_REMOVE_TIME_NS" != "0" ] && [ -n "$UTXO_REMOVE_TIME_NS" ]; then
    TIME_MS=$(awk "BEGIN {printf \"%.6f\", $UTXO_REMOVE_TIME_NS / 1000000}" 2>/dev/null || echo "0")
    BENCHMARKS=$(echo "$BENCHMARKS" | jq --arg name "UTXORemove" --arg time "$TIME_MS" --arg timens "$UTXO_REMOVE_TIME_NS" --arg ops "$UTXO_REMOVE_OPS" '. += [{"name": $name, "time_ms": ($time | tonumber), "time_ns": ($timens | tonumber), "ops_per_sec": ($ops | tonumber)}]' 2>/dev/null || echo "$BENCHMARKS")
fi

cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "measurement_method": "bench_bitcoin (UTXO operations - insert/get/remove matching Commons)",
  "benchmarks": $BENCHMARKS
}
EOF

echo "✅ Results saved to: $OUTPUT_FILE"

