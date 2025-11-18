#!/bin/bash
# Bitcoin Core Block Assembly Benchmark (Portable)

set -e

# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"

# Bitcoin Core Block Assembly Benchmark
# Measures block assembly performance using bench_bitcoin


# Reliably find or build bench_bitcoin
BENCH_BITCOIN=$(get_bench_bitcoin)

echo "Running Core Block Assembly benchmark..."
echo "Output: $OUTPUT_FILE"

BENCH_OUTPUT=$("$BENCH_BITCOIN" -filter="AssembleBlock" 2>&1 || true)

# Parse bench_bitcoin output - look for the table format
BENCHMARKS="[]"
while IFS= read -r line; do
    if echo "$line" | grep -qE "AssembleBlock"; then
        BENCH_NAME="AssembleBlock"
        # Extract time value from first column (ns/op column)
        TIME_VALUE=$(echo "$line" | awk -F'|' '{gsub(/[^0-9.]/,"",$2); print $2}' | head -1 || echo "0")
        TIME_UNIT="ns"

        TIME_NS="0"
        case "$TIME_UNIT" in
            "ns")
                TIME_NS=$(awk "BEGIN {printf \"%.0f\", $TIME_VALUE}" 2>/dev/null || echo "0")
                ;;
            "µs"|"us")
                TIME_NS=$(awk "BEGIN {printf \"%.0f\", $TIME_VALUE * 1000}" 2>/dev/null || echo "0")
                ;;
            "ms")
                TIME_NS=$(awk "BEGIN {printf \"%.0f\", $TIME_VALUE * 1000000}" 2>/dev/null || echo "0")
                ;;
            "s")
                TIME_NS=$(awk "BEGIN {printf \"%.0f\", $TIME_VALUE * 1000000000}" 2>/dev/null || echo "0")
                ;;
        esac

        if [ -n "$BENCH_NAME" ] && [ "$TIME_NS" != "0" ] && [ -n "$TIME_NS" ]; then
            TIME_MS=$(awk "BEGIN {printf \"%.9f\", $TIME_NS / 1000000}" 2>/dev/null || echo "0")
            BENCHMARKS=$(echo "$BENCHMARKS" | jq --arg name "$BENCH_NAME" --argjson time_ms "$TIME_MS" --argjson time_ns "$TIME_NS" '. += [{"name": $name, "time_ms": $time_ms, "time_ns": $time_ns}]' 2>/dev/null || echo "$BENCHMARKS")
        fi
    fi
done <<< "$BENCH_OUTPUT"

if [ "$BENCHMARKS" != "[]" ]; then
    cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "measurement_method": "bench_bitcoin (block assembly)",
  "benchmark_suite": "block_assembly",
  "benchmarks": $BENCHMARKS,
  "comparison_note": "Measures BlockAssembler::AssembleBlock (creating blocks from mempool transactions). Fair comparison with Commons' create_new_block."
}
EOF
    echo "✅ Results saved to: $OUTPUT_FILE"
else
    cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "No block assembly benchmarks found or execution failed",
  "note": "Ensure Bitcoin Core is built with benchmarks and 'AssembleBlock' exists.",
  "benchmarks": []
}
EOF
    exit 1
fi

