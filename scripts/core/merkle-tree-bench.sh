#!/bin/bash
# Bitcoin Core Merkle Tree Benchmark (Portable)

set -e

# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"

# Bitcoin Core Merkle Tree Benchmark
# Measures merkle tree operations using bench_bitcoin


OUTPUT_FILE="$OUTPUT_DIR/core-merkle-tree-bench-$(date +%Y%m%d-%H%M%S).json"

BENCH_BITCOIN="$CORE_PATH/build/bin/bench_bitcoin"

echo "=== Bitcoin Core Merkle Tree Benchmark ==="
echo ""

if [ ! -f "$BENCH_BITCOIN" ]; then
    echo "❌ bench_bitcoin not found"
    cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "bench_bitcoin binary not found"
}
EOF
    exit 1
fi

echo "Running merkle tree benchmarks..."
TEMP_JSON=$(mktemp)
LOG_FILE="/tmp/core-merkle.log"
if "$BENCH_BITCOIN" -filter="MerkleRoot" -min-time=500 2>&1 | tee "$LOG_FILE"; then
    # Parse bench_bitcoin pipe-delimited table format
    BENCHMARKS="[]"
    while IFS= read -r line; do
        if echo "$line" | grep -qE '`.*`'; then
            BENCH_NAME=$(echo "$line" | grep -oE '`[^`]+`' | tr -d '`' || echo "")
            # For MerkleRoot, the first column is ns/leaf
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
  "measurement_method": "bench_bitcoin (actual merkle tree operations)",
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

