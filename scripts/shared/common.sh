#!/bin/bash
# Common functions and path setup for bllvm-bench scripts
# This file should be sourced by all benchmark scripts

set -e

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BLLVM_BENCH_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Discover paths
if [ -f "$BLLVM_BENCH_ROOT/scripts/discover-paths.sh" ]; then
    # Source the path discovery - use source instead of eval to avoid quote issues
    # Temporarily disable set -e to allow script to complete even if paths not found
    set +e
    . "$BLLVM_BENCH_ROOT/scripts/discover-paths.sh"
    set -e
else
    echo "⚠️  Warning: discover-paths.sh not found, paths may not be set correctly" >&2
fi

# Validate required paths (allow Commons-only runs)
if [ -z "$COMMONS_CONSENSUS_PATH" ]; then
    echo "❌ Error: Commons consensus path not discovered" >&2
    echo "   Please set paths in config/config.toml or ensure Commons are in standard locations" >&2
    exit 1
fi

# Set up results directory
RESULTS_DIR="${RESULTS_DIR:-$BLLVM_BENCH_ROOT/results}"
mkdir -p "$RESULTS_DIR"

# Helper function to get output directory (from first argument or default)
get_output_dir() {
    local output_dir="${1:-$RESULTS_DIR}"
    output_dir=$(cd "$output_dir" 2>/dev/null && pwd || echo "$(cd "$(dirname "$0")/../.." && pwd)/results")
    mkdir -p "$output_dir"
    echo "$output_dir"
}

# Helper function to format time
format_time() {
    local time_value="$1"
    if [ -z "$time_value" ] || [ "$time_value" = "null" ] || [ "$time_value" = "0" ]; then
        echo ""
        return
    fi
    
    # Convert to appropriate unit
    if awk "BEGIN {exit !($time_value >= 1000)}" 2>/dev/null; then
        # >= 1000 ms, show as seconds
        awk "BEGIN {printf \"%.3f s\", $time_value / 1000}" 2>/dev/null || echo "${time_value} ms"
    elif awk "BEGIN {exit !($time_value >= 1)}" 2>/dev/null; then
        # >= 1 ms, show as milliseconds
        awk "BEGIN {printf \"%.2f ms\", $time_value}" 2>/dev/null || echo "${time_value} ms"
    elif awk "BEGIN {exit !($time_value >= 0.001)}" 2>/dev/null; then
        # >= 0.001 ms (1 µs), show as microseconds
        awk "BEGIN {printf \"%.2f µs\", $time_value * 1000}" 2>/dev/null || echo "${time_value} ms"
    else
        # < 1 µs, show as nanoseconds
        awk "BEGIN {printf \"%.2f ns\", $time_value * 1000000}" 2>/dev/null || echo "${time_value} ms"
    fi
}

# Export common variables
export CORE_PATH
export COMMONS_CONSENSUS_PATH
export COMMONS_NODE_PATH
export BLLVM_BENCH_ROOT
export RESULTS_DIR

