#!/bin/bash
# Benchmark Merkle Tree Operations with Pre-computed Hashes
# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"
# This benchmark matches Core's approach: uses pre-computed hashes (not transactions)
# for fair comparison of tree-building performance

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"
RESULTS_DIR="$PROJECT_ROOT/results"
BENCH_DIR="$BLLVM_BENCH_ROOT"

# Create results directory if it doesn't exist
mkdir -p "$RESULTS_DIR"

echo "=== Bitcoin Commons Merkle Tree Operations (Pre-computed Hashes) Benchmark ==="
echo ""

cd "$BENCH_DIR"

# Run the benchmark with production features
echo "Running merkle tree benchmarks with pre-computed hashes..."
BENCH_OUTPUT=$(RUSTFLAGS="-C target-cpu=native" cargo bench --bench merkle_tree_precomputed --features production 2>&1 | tee /tmp/merkle_precomputed_bench.log)

# Parse results from Criterion JSON output for all leaf counts
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
OUTPUT_FILE="$RESULTS_DIR/commons-merkle-tree-precomputed-bench-$TIMESTAMP.json"
OUTPUT_DIR=$(get_output_dir "${1:-$RESULTS_DIR}")
OUTPUT_FILE="$OUTPUT_DIR/commons-merkle-tree-precomputed-bench-$(date +%Y%m%d-%H%M%S).json"


BENCHMARKS_JSON="["
FIRST=true

# Extract from Criterion JSON (more reliable than stdout)
CRITERION_DIR="$BENCH_DIR/target/criterion"
FIRST=true

# Try JSON first, fallback to stdout
for leaf_count in 1 10 100 1000 2000 9001; do
    BENCH_NAME="merkle_root_precomputed_${leaf_count}leaves"
    BENCH_DIR_JSON="$CRITERION_DIR/merkle_root_precomputed/$BENCH_NAME"
    
    # Try JSON first
    if [ -d "$BENCH_DIR_JSON" ] && [ -f "$BENCH_DIR_JSON/base/estimates.json" ]; then
        TIME_NS=$(jq -r '.mean.point_estimate // 0' "$BENCH_DIR_JSON/base/estimates.json" 2>/dev/null || echo "0")
        if [ -n "$TIME_NS" ] && [ "$TIME_NS" != "null" ] && [ "$TIME_NS" != "0" ]; then
            TIME_MS=$(awk "BEGIN {printf \"%.6f\", $TIME_NS / 1000000}" 2>/dev/null || echo "0")
            
            if [ "$FIRST" = "true" ]; then
                FIRST=false
            else
                BENCHMARKS_JSON+=","
            fi
            
            BENCHMARKS_JSON+="{\"name\":\"$BENCH_NAME\",\"time_ns\":$TIME_NS,\"time_ms\":$TIME_MS,\"unit\":\"ms\"}"
            echo "  Found $BENCH_NAME: ${TIME_MS}ms (from JSON)"
            continue
        fi
    fi
    
    # Fallback to stdout parsing
    TIME_LINE=$(echo "$BENCH_OUTPUT" | grep -A 2 "$BENCH_NAME" | grep "time:" | head -1)
    
    if [ -n "$TIME_LINE" ]; then
        # Extract median time (middle value in brackets) - handle both "ms" and "µs"
        # Pattern: time:   [lower median upper unit]
        MEDIAN_VALUE=$(echo "$TIME_LINE" | sed -n 's/.*\[[^]]* \([0-9.]*\) \([0-9.]*\) \([0-9.]*\) \([a-zµ]*\)\].*/\2/p')
        TIME_UNIT=$(echo "$TIME_LINE" | sed -n 's/.*\[[^]]* \([0-9.]*\) \([0-9.]*\) \([0-9.]*\) \([a-zµ]*\)\].*/\4/p')
        
        # Also try simpler pattern
        if [ -z "$MEDIAN_VALUE" ]; then
            MEDIAN_VALUE=$(echo "$TIME_LINE" | grep -oE '[0-9]+\.[0-9]+ [a-zµ]+' | awk '{print $1}' | head -2 | tail -1)
            TIME_UNIT=$(echo "$TIME_LINE" | grep -oE '[0-9]+\.[0-9]+ [a-zµ]+' | awk '{print $2}' | head -2 | tail -1)
        fi
        
        MEDIAN_MS="$MEDIAN_VALUE"
        
        if [ -n "$MEDIAN_MS" ] && [ -n "$TIME_UNIT" ]; then
            # Convert to nanoseconds and milliseconds
            case "$TIME_UNIT" in
                "ns")
                    MEDIAN_NS=$(awk "BEGIN {printf \"%.0f\", $MEDIAN_MS}")
                    MEDIAN_MS=$(awk "BEGIN {printf \"%.9f\", $MEDIAN_MS / 1000000}")
                    ;;
                "µs"|"us")
                    MEDIAN_NS=$(awk "BEGIN {printf \"%.0f\", $MEDIAN_MS * 1000}")
                    MEDIAN_MS=$(awk "BEGIN {printf \"%.9f\", $MEDIAN_MS / 1000}")
                    ;;
                "ms")
                    MEDIAN_NS=$(awk "BEGIN {printf \"%.0f\", $MEDIAN_MS * 1000000}")
                    ;;
                "s")
                    MEDIAN_NS=$(awk "BEGIN {printf \"%.0f\", $MEDIAN_MS * 1000000000}")
                    MEDIAN_MS=$(awk "BEGIN {printf \"%.9f\", $MEDIAN_MS * 1000}")
                    ;;
            esac
            
            if [ "$FIRST" = "true" ]; then
                FIRST=false
            else
                BENCHMARKS_JSON+=","
            fi
            
            BENCHMARKS_JSON+="{\"name\":\"$BENCH_NAME\",\"time_ns\":$MEDIAN_NS,\"time_ms\":$MEDIAN_MS,\"unit\":\"ms\"}"
            echo "  Found $BENCH_NAME: ${MEDIAN_MS}ms"
        fi
    fi
done

BENCHMARKS_JSON+="]"

# Create final JSON output
cat > "$OUTPUT_FILE" <<EOF
{
  "benchmark": "merkle_tree_precomputed",
  "timestamp": "$TIMESTAMP",
  "benchmarks": $BENCHMARKS_JSON,
  "methodology": "Pre-computed hashes (like Core's MerkleRoot benchmark)",
  "comparison_note": "This benchmark uses pre-computed random hashes, matching Core's approach for fair tree-building performance comparison"
}
EOF

echo "✅ Results saved to: $OUTPUT_FILE"

echo ""
echo "=== Benchmark Complete ==="
