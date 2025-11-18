#!/bin/bash
# Bitcoin Core Transaction ID Calculation Benchmark (Portable)

set -e

# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"

# Bitcoin Core Transaction ID Calculation Benchmark
# Measures transaction ID (hash) calculation performance using bench_bitcoin


# Reliably find or build bench_bitcoin
BENCH_BITCOIN=$(get_bench_bitcoin)

echo "Running Core Transaction ID Calculation benchmark..."
echo "Output: $OUTPUT_FILE"

# Run bench_bitcoin for TransactionIdCalculation benchmark
BENCH_OUTPUT=$("$BENCH_BITCOIN" -filter="TransactionIdCalculation" 2>&1 || true)

# Parse bench_bitcoin output
# Format: "TransactionIdCalculation        , 1234.56, 1234.56, 1234.56, 1234.56, 1234.56"
if echo "$BENCH_OUTPUT" | grep -q "TransactionIdCalculation"; then
    TIME_MS=$(echo "$BENCH_OUTPUT" | grep "TransactionIdCalculation" | awk -F',' '{print $2}' | tr -d ' ' || echo "")
    
    if [ -n "$TIME_MS" ] && [ "$TIME_MS" != "null" ] && [ "$TIME_MS" != "0" ]; then
        cat > "$OUTPUT_FILE" << EOF
{
  "benchmark": "transaction_id_calculation",
  "implementation": "bitcoin_core",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "methodology": "Transaction ID calculation using GetHash() (double SHA256 of serialized transaction without witness)",
  "benchmarks": [
    {
      "name": "TransactionIdCalculation",
      "time_ms": $TIME_MS,
      "comparison_note": "Measures double SHA256 of serialized transaction (same as Commons' calculate_tx_id)"
    }
  ]
}
EOF
        echo "✓ Transaction ID calculation benchmark completed"
        echo "  Time: ${TIME_MS} ms"
    else
        echo "WARNING: Could not parse time from bench_bitcoin output"
        cat > "$OUTPUT_FILE" << EOF
{
  "benchmark": "transaction_id_calculation",
  "implementation": "bitcoin_core",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "Could not parse timing from bench_bitcoin output",
  "raw_output": "$(echo "$BENCH_OUTPUT" | head -50 | jq -Rs .)"
}
EOF
    fi
else
    echo "WARNING: Benchmark 'TransactionIdCalculation' not found"
    cat > "$OUTPUT_FILE" << EOF
{
  "benchmark": "transaction_id_calculation",
  "implementation": "bitcoin_core",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "Benchmark 'TransactionIdCalculation' not found. Please rebuild Core with the new benchmark.",
  "raw_output": "$(echo "$BENCH_OUTPUT" | head -50 | jq -Rs .)"
}
EOF
fi

echo ""
echo "Benchmark data written to: $OUTPUT_FILE"

