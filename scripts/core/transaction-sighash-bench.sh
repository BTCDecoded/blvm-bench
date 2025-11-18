#!/bin/bash
# Bitcoin Core Transaction Sighash Calculation Benchmark
# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"
# Measures transaction sighash (signature hash) calculation performance using bench_bitcoin
set -e

OUTPUT_DIR="${1:-$(dirname "$0")/../results}"
mkdir -p "$OUTPUT_DIR"

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CORE_DIR="$PROJECT_ROOT/core"
# Reliably find or build bench_bitcoin
BENCH_BITCOIN=$(get_bench_bitcoin)

echo "Running Core Transaction Sighash Calculation benchmark..."
echo "Output: $OUTPUT_FILE"

BENCH_OUTPUT=$("$BENCH_BITCOIN" -filter="TransactionSighashCalculation" 2>&1 || true)

# Parse bench_bitcoin output - look for the table format
# Format: | TIME_VALUE | op/s | ... | `BenchmarkName`
BENCHMARKS="[]"
while IFS= read -r line; do
    if echo "$line" | grep -qE "TransactionSighashCalculation"; then
        BENCH_NAME="TransactionSighashCalculation"
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
  "measurement_method": "bench_bitcoin (transaction sighash calculation)",
  "benchmark_suite": "transaction_sighash",
  "benchmarks": $BENCHMARKS,
  "comparison_note": "Measures SignatureHash calculation (hash that gets signed with ECDSA/Schnorr). Fair comparison with Commons' calculate_transaction_sighash."
}
EOF
    echo "✅ Results saved to: $OUTPUT_FILE"
else
    cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "No transaction sighash benchmarks found or execution failed",
  "note": "Ensure Bitcoin Core is built with benchmarks and 'TransactionSighashCalculation' exists.",
  "benchmarks": []
}
EOF
    exit 1
fi

