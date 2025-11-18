#!/bin/bash
# Bitcoin Core Block Serialization/Deserialization Benchmark (Portable)

set -e

# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"

# Bitcoin Core Block Serialization/Deserialization Benchmark
# Measures block read/write performance using bench_bitcoin


BENCH_BITCOIN="$CORE_DIR/build/bin/bench_bitcoin"

OUTPUT_FILE="$OUTPUT_DIR/core-block-serialization-bench-$(date +%Y%m%d-%H%M%S).json"

if [ ! -f "$BENCH_BITCOIN" ]; then
    echo "ERROR: bench_bitcoin not found at $BENCH_BITCOIN"
    echo "Please build Bitcoin Core first: cd $CORE_DIR && make -j$(nproc)"
    exit 1
fi

echo "Running Core Block Serialization benchmark..."
echo "Output: $OUTPUT_FILE"

# Run bench_bitcoin for block read/write operations
BENCH_OUTPUT=$("$BENCH_BITCOIN" -filter="ReadBlock|WriteBlock|DeserializeBlock" 2>&1 || true)

# Parse bench_bitcoin output
# Format: "ReadBlockBench        , 1234.56, 1234.56, 1234.56, 1234.56, 1234.56"
BENCHMARKS=()

# Extract ReadBlockBench
if echo "$BENCH_OUTPUT" | grep -q "ReadBlockBench"; then
    READ_TIME=$(echo "$BENCH_OUTPUT" | grep "ReadBlockBench" | awk -F',' '{print $2}' | tr -d ' ' || echo "")
    if [ -n "$READ_TIME" ] && [ "$READ_TIME" != "null" ]; then
        BENCHMARKS+=("{\"name\":\"ReadBlockBench\",\"time_ms\":$READ_TIME}")
    fi
fi

# Extract WriteBlockBench
if echo "$BENCH_OUTPUT" | grep -q "WriteBlockBench"; then
    WRITE_TIME=$(echo "$BENCH_OUTPUT" | grep "WriteBlockBench" | awk -F',' '{print $2}' | tr -d ' ' || echo "")
    if [ -n "$WRITE_TIME" ] && [ "$WRITE_TIME" != "null" ]; then
        BENCHMARKS+=("{\"name\":\"WriteBlockBench\",\"time_ms\":$WRITE_TIME}")
    fi
fi

# Extract DeserializeBlockTest
if echo "$BENCH_OUTPUT" | grep -q "DeserializeBlockTest"; then
    DESER_TIME=$(echo "$BENCH_OUTPUT" | grep "DeserializeBlockTest" | awk -F',' '{print $2}' | tr -d ' ' || echo "")
    if [ -n "$DESER_TIME" ] && [ "$DESER_TIME" != "null" ]; then
        BENCHMARKS+=("{\"name\":\"DeserializeBlockTest\",\"time_ms\":$DESER_TIME}")
    fi
fi

# Create JSON output
if [ ${#BENCHMARKS[@]} -gt 0 ]; then
    BENCHMARKS_JSON=$(IFS=','; echo "${BENCHMARKS[*]}")
    cat > "$OUTPUT_FILE" << EOF
{
  "benchmark": "block_serialization",
  "implementation": "bitcoin_core",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "methodology": "Block serialization/deserialization using bench_bitcoin (ReadBlock, WriteBlock, DeserializeBlock)",
  "benchmarks": [$BENCHMARKS_JSON]
}
EOF
    echo "✓ Block serialization benchmark completed"
    echo "  Found ${#BENCHMARKS[@]} benchmark(s)"
else
    echo "WARNING: No block serialization benchmarks found"
    cat > "$OUTPUT_FILE" << EOF
{
  "benchmark": "block_serialization",
  "implementation": "bitcoin_core",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "error": "No block serialization benchmarks found",
  "raw_output": "$(echo "$BENCH_OUTPUT" | head -50 | jq -Rs .)"
}
EOF
fi

echo ""
echo "Benchmark data written to: $OUTPUT_FILE"
