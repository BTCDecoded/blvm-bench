#!/bin/bash
# Bitcoin Core RIPEMD160 Benchmark (Portable)

set -e

# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"

# Bitcoin Core RIPEMD160 Benchmark
# Measures RIPEMD160 hash performance using bench_bitcoin



BENCH_BITCOIN="$CORE_DIR$CORE_PATH/$CORE_PATH/build/bin/bench_bitcoin"
OUTPUT_FILE="$OUTPUT_DIR/core-ripemd160-bench-$(date +%Y%m%d-%H%M%S).json"

echo "=== Bitcoin Core RIPEMD160 Benchmark ==="
echo ""

if [ ! -f "$BENCH_BITCOIN" ]; then
    echo "❌ bench_bitcoin not found at $BENCH_BITCOIN"
    echo "   Building bench_bitcoin..."
    cd "$CORE_DIR"
    make -j$(nproc) bench_bitcoin 2>&1 | tail -5
    if [ ! -f "$BENCH_BITCOIN" ]; then
        echo "❌ Failed to build bench_bitcoin"
        exit 1
    fi
fi

echo "Running bench_bitcoin for RIPEMD160 operations (this may take a few minutes)..."
echo "This benchmarks RIPEMD160 hash performance."

# Run bench_bitcoin and capture output
BENCH_OUTPUT=$("$BENCH_BITCOIN" -filter="RIPEMD160" 2>&1 || echo "")

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

RIPEMD160_LINE=$(echo "$BENCH_OUTPUT" | grep -iE "RIPEMD160|BenchRIPEMD160" | head -1 || echo "")

RIPEMD160_DATA=$(parse_bench_bitcoin "$RIPEMD160_LINE")
RIPEMD160_TIME_NS=$(echo "$RIPEMD160_DATA" | cut -d'|' -f1)
RIPEMD160_OPS=$(echo "$RIPEMD160_DATA" | cut -d'|' -f2)

BENCHMARKS="[]"

if [ "$RIPEMD160_TIME_NS" != "0" ]; then
    TIME_MS=$(awk "BEGIN {printf \"%.6f\", $RIPEMD160_TIME_NS / 1000000}")
    BENCHMARKS=$(echo "$BENCHMARKS" | jq --arg name "BenchRIPEMD160" --arg time "$TIME_MS" --arg timens "$RIPEMD160_TIME_NS" '. += [{"name": $name, "time_ms": ($time | tonumber), "time_ns": ($timens | tonumber), "ops_per_sec": ($RIPEMD160_OPS | tonumber)}]' 2>/dev/null || echo "$BENCHMARKS")
fi

cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "measurement_method": "bench_bitcoin (RIPEMD160 hash operations)",
  "benchmarks": $BENCHMARKS
}
EOF
echo "✅ Results saved to: $OUTPUT_FILE"


