#!/bin/bash
# Bitcoin Core Mempool Operations Benchmark (Portable)

set -e

# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"

# Bitcoin Core Mempool Operations Benchmark
# Measures mempool operations using bench_bitcoin


OUTPUT_FILE="$OUTPUT_DIR/core-mempool-operations-bench-$(date +%Y%m%d-%H%M%S).json"

# Reliably find or build bench_bitcoin
BENCH_BITCOIN=$(get_bench_bitcoin)

echo "Running mempool operations benchmarks..."
TEMP_JSON=$(mktemp)
LOG_FILE="/tmp/core-mempool.log"
# Run multiple mempool benchmarks and combine results
if "$BENCH_BITCOIN" -filter="MempoolCheck|MempoolEviction" -min-time=500 2>&1 | tee "$LOG_FILE"; then
    # Parse bench_bitcoin pipe-delimited table format
    BENCHMARKS="[]"
    while IFS= read -r line; do
        if echo "$line" | grep -qE '`.*`'; then
            BENCH_NAME=$(echo "$line" | grep -oE '`[^`]+`' | tr -d '`' || echo "")
            TIME_NS=$(echo "$line" | awk -F'|' '{gsub(/[^0-9.]/,"",$2); print $2}' 2>/dev/null | head -1 || echo "")
            
            if [ -n "$BENCH_NAME" ] && [ -n "$TIME_NS" ] && [ "$TIME_NS" != "0" ] && [ "$TIME_NS" != "" ]; then
                TIME_MS=$(awk "BEGIN {printf \"%.6f\", $TIME_NS / 1000000}")
                BENCHMARKS=$(echo "$BENCHMARKS" | jq --arg name "$BENCH_NAME" --arg time "$TIME_MS" --arg timens "$TIME_NS" '. += [{"name": $name, "time_ms": ($time | tonumber), "time_ns": ($timens | tonumber)}]' 2>/dev/null || echo "$BENCHMARKS")
            fi
        fi
    done < "$LOG_FILE"
    
    cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "measurement_method": "bench_bitcoin (actual mempool operations)",
  "benchmarks": $BENCHMARKS
}
EOF
else
    cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "Benchmark execution failed"
}
EOF
    exit 1
fi

echo "✅ Results saved to: $OUTPUT_FILE"

