#!/bin/bash
# Bitcoin Core Transaction Validation Benchmark (Portable)
# Measures actual transaction validation performance using bench_bitcoin

set -e

# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"

OUTPUT_DIR=$(get_output_dir "${1:-$RESULTS_DIR}")
OUTPUT_FILE="$OUTPUT_DIR/core-transaction-validation-bench-$(date +%Y%m%d-%H%M%S).json"

echo "=== Bitcoin Core Transaction Validation Benchmark ==="
echo ""

# Find bench_bitcoin
BENCH_BITCOIN=""
if [ -n "$CORE_PATH" ]; then
    if [ -f "$CORE_PATH/build/bin/bench_bitcoin" ]; then
        BENCH_BITCOIN="$CORE_PATH/build/bin/bench_bitcoin"
    elif [ -f "$CORE_PATH/bin/bench_bitcoin" ]; then
        BENCH_BITCOIN="$CORE_PATH/bin/bench_bitcoin"
    fi
fi

if [ -z "$BENCH_BITCOIN" ] && command -v bench_bitcoin >/dev/null 2>&1; then
    BENCH_BITCOIN=$(command -v bench_bitcoin)
fi

if [ -z "$BENCH_BITCOIN" ] || [ ! -f "$BENCH_BITCOIN" ]; then
    echo "❌ bench_bitcoin not found"
    cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "bench_bitcoin binary not found",
  "note": "Build Bitcoin Core first"
}
EOF
    exit 1
fi

echo "Running transaction validation benchmarks..."
echo "This measures actual transaction validation logic (not RPC overhead)"
echo ""

LOG_FILE="/tmp/core-tx-validation.log"
if "$BENCH_BITCOIN" -filter="DeserializeAndCheckBlockTest" -min-time=500 2>&1 | tee "$LOG_FILE"; then
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
  "measurement_method": "bench_bitcoin (actual validation logic)",
  "benchmarks": $BENCHMARKS
}
EOF
    echo "✅ Results saved to: $OUTPUT_FILE"
else
    echo "❌ Benchmark failed"
    cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "Benchmark execution failed",
  "log": "/tmp/core-tx-validation.log"
}
EOF
    exit 1
fi

echo ""
cat "$OUTPUT_FILE" | jq '.' 2>/dev/null || cat "$OUTPUT_FILE"

