#!/bin/bash
# Bitcoin Core Mempool RBF (Replace-By-Fee) Benchmark (Portable)

set -e

# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"

# Bitcoin Core Mempool RBF (Replace-By-Fee) Benchmark
# Measures RBF checks using bench_bitcoin



BENCH_BITCOIN="$CORE_DIR$CORE_PATH/$CORE_PATH/build/bin/bench_bitcoin"
OUTPUT_FILE="$OUTPUT_DIR/core-mempool-rbf-bench-$(date +%Y%m%d-%H%M%S).json"

echo "=== Bitcoin Core Mempool RBF Benchmark ==="
echo ""

if [ ! -f "$BENCH_BITCOIN" ]; then
    echo "❌ bench_bitcoin not found at $BENCH_BITCOIN"
    cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "bench_bitcoin binary not found"
}
EOF
    exit 1
fi

echo "Running RBF benchmarks..."
echo "Note: Core may not have a specific RBF benchmark."
echo "Using MempoolCheck and MempoolEviction as proxies for mempool operations including RBF."

LOG_FILE="/tmp/core-mempool-rbf.log"
# Run all benchmarks and extract mempool-related ones
if "$BENCH_BITCOIN" 2>&1 | tee "$LOG_FILE"; then
    BENCHMARKS="[]"
    # Extract MempoolCheck and MempoolEviction (use MempoolCheck as primary for RBF)
    while IFS= read -r line; do
        if echo "$line" | grep -qE '`MempoolCheck`|`MempoolEviction`' && ! echo "$line" | grep -q "EphemeralSpends"; then
            BENCH_NAME=$(echo "$line" | grep -oE '`[^`]+`' | tr -d '`' | head -1 || echo "")
            TIME_NS=$(echo "$line" | awk -F'|' '{gsub(/[^0-9.]/,"",$2); print $2}' 2>/dev/null | head -1 || echo "")
            
            if [ -n "$BENCH_NAME" ] && [ -n "$TIME_NS" ] && [ "$TIME_NS" != "0" ] && [ "$TIME_NS" != "" ]; then
                TIME_MS=$(awk "BEGIN {printf \"%.6f\", $TIME_NS / 1000000}")
                BENCHMARKS=$(echo "$BENCHMARKS" | jq --arg name "$BENCH_NAME" --arg time "$TIME_MS" --arg timens "$TIME_NS" '. += [{"name": $name, "time_ms": ($time | tonumber), "time_ns": ($timens | tonumber)}]' 2>/dev/null || echo "$BENCHMARKS")
            fi
        fi
    done < "$LOG_FILE"
else
    BENCHMARKS="[]"
fi

cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "measurement_method": "bench_bitcoin (mempool operations - proxy for RBF)",
  "benchmark_suite": "mempool_rbf",
  "benchmarks": $BENCHMARKS,
  "note": "Core may not have a specific RBF benchmark. Using MempoolCheck/MempoolEviction as proxies."
}
EOF

echo "✅ Results saved to: $OUTPUT_FILE"

