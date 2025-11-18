#!/bin/bash
# Helper function to add statistical data to a benchmark JSON entry
# Usage: add_stats_to_benchmark <benchmark_json> <estimates.json_path>
# Returns: Updated benchmark JSON with statistics

set -e

BENCHMARK_JSON="$1"
ESTIMATES_FILE="$2"

if [ ! -f "$ESTIMATES_FILE" ]; then
    echo "$BENCHMARK_JSON"
    exit 0
fi

# Extract stats and merge into benchmark JSON
STATS=$(source "$(dirname "$0")/extract-criterion-stats.sh" "$ESTIMATES_FILE")

echo "$BENCHMARK_JSON" | jq --argjson stats "$STATS" '.statistics = $stats' 2>/dev/null || echo "$BENCHMARK_JSON"

