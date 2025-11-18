#!/bin/bash
# Generate CSV Report from Consolidated JSON
# Reads consolidated JSON and generates CSV for spreadsheet analysis

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BLLVM_BENCH_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
source "$SCRIPT_DIR/shared/common.sh"

OUTPUT_DIR=$(get_output_dir "${1:-$RESULTS_DIR}")

# Find latest consolidated JSON
CONSOLIDATED_JSON="$OUTPUT_DIR/benchmark-results-consolidated-latest.json"

if [ ! -f "$CONSOLIDATED_JSON" ]; then
    echo "❌ No consolidated JSON found at $CONSOLIDATED_JSON. Generate it first with: make json"
    exit 1
fi

CSV_FILE="$OUTPUT_DIR/benchmark-results-consolidated-latest.csv"

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  Generating CSV Report from Consolidated JSON                 ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "Reading from: $CONSOLIDATED_JSON"
echo ""

# Generate CSV header
cat > "$CSV_FILE" << 'EOF'
Benchmark,Implementation,Time (ms),Time (ns),Mean (ns),Median (ns),Std Dev (ns),CI Lower (ns),CI Upper (ns),Confidence Level,Ops/sec,Winner,Speedup
EOF

# Extract benchmark data and convert to CSV
jq -r '.benchmarks | to_entries[] | 
    .key as $bench_key |
    .value | 
    if .core then
        (.core | 
            if .bitcoin_core_block_validation.primary_comparison then
                "\($bench_key),Core,\(.bitcoin_core_block_validation.primary_comparison.time_per_block_ms),\(.bitcoin_core_block_validation.primary_comparison.time_per_block_ns),,,,," 
            elif .benchmarks then
                (.benchmarks[0] | 
                    "\($bench_key),Core,\(.time_ms),\(.time_ns),\(.statistics.mean.point_estimate // ""),\(.statistics.median.point_estimate // ""),\(.statistics.std_dev // ""),\(.statistics.mean.confidence_interval.lower_bound // ""),\(.statistics.mean.confidence_interval.upper_bound // ""),\(.statistics.mean.confidence_interval.confidence_level // "")"
                )
            else
                "\($bench_key),Core,,,,,,,"
            end
        )
    else
        ""
    end,
    if .commons then
        (.commons | 
            if .bitcoin_commons_block_validation.connect_block then
                "\($bench_key),Commons,\(.bitcoin_commons_block_validation.connect_block.time_per_block_ms),\(.bitcoin_commons_block_validation.connect_block.time_per_block_ns),,,,," 
            elif .benchmarks then
                (.benchmarks[0] | 
                    "\($bench_key),Commons,\(.time_ms),\(.time_ns),\(.statistics.mean.point_estimate // ""),\(.statistics.median.point_estimate // ""),\(.statistics.std_dev // ""),\(.statistics.mean.confidence_interval.lower_bound // ""),\(.statistics.mean.confidence_interval.upper_bound // ""),\(.statistics.mean.confidence_interval.confidence_level // "")"
                )
            else
                "\($bench_key),Commons,,,,,,,"
            end
        )
    else
        ""
    end,
    if .comparison then
        "\($bench_key),Comparison,,,,,,,\(.comparison.winner // ""),\(.comparison.speedup // "")"
    else
        ""
    end
' "$CONSOLIDATED_JSON" | grep -v "^$" >> "$CSV_FILE"

# Calculate ops/sec for rows that have time_ms
awk -F',' 'BEGIN {OFS=","} {
    if ($3 != "" && $3 != "Time (ms)" && $3 > 0) {
        $11 = sprintf("%.2f", 1000 / $3)
    }
    print
}' "$CSV_FILE" > "$CSV_FILE.tmp" && mv "$CSV_FILE.tmp" "$CSV_FILE"

echo "✅ CSV generated: $CSV_FILE"
echo ""
echo "Summary:"
echo "  Total rows: $(wc -l < "$CSV_FILE" | tr -d ' ') (including header)"
echo "  Benchmarks: $(($(wc -l < "$CSV_FILE" | tr -d ' ') - 1))"
echo ""
echo "CSV columns:"
echo "  - Benchmark name"
echo "  - Implementation (Core/Commons/Comparison)"
echo "  - Time (ms)"
echo "  - Time (ns)"
echo "  - Mean (ns) - statistical mean"
echo "  - Median (ns) - statistical median"
echo "  - Std Dev (ns) - standard deviation"
echo "  - CI Lower (ns) - confidence interval lower bound"
echo "  - CI Upper (ns) - confidence interval upper bound"
echo "  - Confidence Level - confidence level (typically 0.95)"
echo "  - Ops/sec - operations per second"
echo "  - Winner - winner of comparison (if applicable)"
echo "  - Speedup - speedup factor (if applicable)"

