#!/bin/bash
# Bitcoin Core Script Verification Benchmark (Portable)

set -e

# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"

# Bitcoin Core Script Verification Benchmark
# Measures script verification performance using bench_bitcoin


BENCH_BITCOIN="$CORE_DIR/build/bin/bench_bitcoin"

OUTPUT_FILE="$OUTPUT_DIR/core-script-verification-bench-$(date +%Y%m%d-%H%M%SZ).json"

if [ ! -f "$BENCH_BITCOIN" ]; then
    echo "ERROR: bench_bitcoin not found at $BENCH_BITCOIN"
    echo "Please build Bitcoin Core first: cd $CORE_DIR && make -j$(nproc)"
    exit 1
fi

echo "Running Core Script Verification benchmark..."
echo "Output: $OUTPUT_FILE"

BENCH_OUTPUT=$("$BENCH_BITCOIN" -filter="VerifyScriptBench|VerifyNestedIfScript" 2>&1 || true)

# Parse bench_bitcoin output - look for the table format
# Format: | TIME_VALUE | op/s | ... | `BenchmarkName`
BENCHMARKS="[]"
while IFS= read -r line; do
    # Look for lines with VerifyScriptBench or VerifyNestedIfScript in the benchmark column
    if echo "$line" | grep -qE "VerifyScriptBench|VerifyNestedIfScript"; then
        # Extract benchmark name (last column, remove backticks)
        BENCH_NAME=$(echo "$line" | grep -oE "VerifyScriptBench|VerifyNestedIfScript" | head -1 || echo "")
        # Extract time value from first column (ns/op column)
        # Format: | 34,604.58 | ... (first number after |)
        TIME_VALUE=$(echo "$line" | awk -F'|' '{gsub(/[^0-9.]/,"",$2); print $2}' | head -1 || echo "0")
        # bench_bitcoin always outputs in ns/op format
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
  "measurement_method": "bench_bitcoin (script verification)",
  "benchmark_suite": "script_verification",
  "benchmarks": $BENCHMARKS
}
EOF
    echo "✅ Results saved to: $OUTPUT_FILE"
else
    cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "No script verification benchmarks found or execution failed",
  "note": "Ensure Bitcoin Core is built with benchmarks and 'VerifyScriptBench', 'VerifyNestedIfScript' exist.",
  "benchmarks": []
}
EOF
    exit 1
fi

