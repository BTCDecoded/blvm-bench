#!/bin/bash
# Bitcoin Core Transaction Serialization Benchmark (Portable)

set -e

# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"

# Bitcoin Core Transaction Serialization Benchmark
# Measures transaction serialization performance using bench_bitcoin


BENCH_BITCOIN="$CORE_DIR$CORE_PATH/$CORE_PATH/build/bin/bench_bitcoin"

OUTPUT_FILE="$OUTPUT_DIR/core-transaction-serialization-bench-$(date +%Y%m%d-%H%M%S).json"

if [ ! -f "$BENCH_BITCOIN" ]; then
    echo "ERROR: bench_bitcoin not found at $BENCH_BITCOIN"
    echo "Please build Bitcoin Core first: cd $CORE_DIR && make -j$(nproc)"
    exit 1
fi

echo "Running Core Transaction Serialization benchmark..."
echo "Output: $OUTPUT_FILE"

# Run bench_bitcoin for TransactionSerialization benchmark
BENCH_OUTPUT=$("$BENCH_BITCOIN" -filter="TransactionSerialization" 2>&1 || true)

# Parse bench_bitcoin output
if echo "$BENCH_OUTPUT" | grep -q "TransactionSerialization"; then
    TIME_MS=$(echo "$BENCH_OUTPUT" | grep "TransactionSerialization" | awk -F',' '{print $2}' | tr -d ' ' || echo "")
    
    if [ -n "$TIME_MS" ] && [ "$TIME_MS" != "null" ] && [ "$TIME_MS" != "0" ]; then
        cat > "$OUTPUT_FILE" << EOF
{
  "benchmark": "transaction_serialization",
  "implementation": "bitcoin_core",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "methodology": "Transaction serialization to Bitcoin wire format using SerializeTransaction",
  "benchmarks": [
    {
      "name": "TransactionSerialization",
      "time_ms": $TIME_MS,
      "comparison_note": "Measures serialization of transaction to Bitcoin wire format (same as Commons' serialize_transaction)"
    }
  ]
}
EOF
        echo "✓ Transaction serialization benchmark completed"
        echo "  Time: ${TIME_MS} ms"
    else
        echo "WARNING: Could not parse time from bench_bitcoin output"
        cat > "$OUTPUT_FILE" << EOF
{
  "benchmark": "transaction_serialization",
  "implementation": "bitcoin_core",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "Could not parse timing from bench_bitcoin output",
  "raw_output": "$(echo "$BENCH_OUTPUT" | head -50 | jq -Rs .)"
}
EOF
    fi
else
    echo "WARNING: Benchmark 'TransactionSerialization' not found"
    cat > "$OUTPUT_FILE" << EOF
{
  "benchmark": "transaction_serialization",
  "implementation": "bitcoin_core",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "Benchmark 'TransactionSerialization' not found. Please rebuild Core with the new benchmark.",
  "raw_output": "$(echo "$BENCH_OUTPUT" | head -50 | jq -Rs .)"
}
EOF
fi

echo ""
echo "Benchmark data written to: $OUTPUT_FILE"

