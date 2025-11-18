#!/bin/bash
# Track benchmark history - stores results with timestamps for historical analysis
# Creates time series data for performance trends

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BLLVM_BENCH_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
source "$SCRIPT_DIR/shared/common.sh"

CURRENT_JSON="${1:-}"
HISTORY_DIR="${HISTORY_DIR:-$BLLVM_BENCH_ROOT/results/history}"

if [ -z "$CURRENT_JSON" ]; then
    # Find latest consolidated JSON
    CURRENT_JSON="$BLLVM_BENCH_ROOT/results/benchmark-results-consolidated-latest.json"
fi

if [ ! -f "$CURRENT_JSON" ]; then
    echo "❌ No current benchmark JSON found"
    echo "Usage: $0 [path/to/consolidated-json.json]"
    exit 1
fi

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  Historical Tracking                                         ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

mkdir -p "$HISTORY_DIR"

# Create timestamped history entry
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
HISTORY_FILE="$HISTORY_DIR/history-$TIMESTAMP.json"

# Copy current results with metadata
jq --arg timestamp "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
   --arg git_commit "$(git rev-parse HEAD 2>/dev/null || echo "unknown")" \
   --arg git_branch "$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")" \
   '. + {
       history_metadata: {
         timestamp: $timestamp,
         git_commit: $git_commit,
         git_branch: $git_branch,
         stored_at: (now | todateiso8601)
       }
   }' "$CURRENT_JSON" > "$HISTORY_FILE"

echo "✅ History entry created: $HISTORY_FILE"

# Create or update time series data
TIMESERIES_FILE="$HISTORY_DIR/timeseries.json"

if [ ! -f "$TIMESERIES_FILE" ]; then
    # Initialize time series
    jq -n '{
        benchmarks: {},
        entries: []
    }' > "$TIMESERIES_FILE"
fi

# Extract benchmark data and add to time series
jq --slurpfile current "$CURRENT_JSON" \
   --arg timestamp "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
   --arg git_commit "$(git rev-parse HEAD 2>/dev/null || echo "unknown")" \
   '
    .entries += [{
        timestamp: $timestamp,
        git_commit: $git_commit,
        data: $current[0]
    }] |
    # Update benchmark time series
    . as $ts |
    ($current[0].benchmarks // {}) as $benchmarks |
    reduce ($benchmarks | keys[]) as $name (
        .benchmarks;
        .[$name] = ((.[$name] // []) + [{
            timestamp: $timestamp,
            git_commit: $git_commit,
            core_time_ms: ($benchmarks[$name].core.benchmarks[0].time_ms // $benchmarks[$name].core.time_ms // null),
            commons_time_ms: ($benchmarks[$name].commons.benchmarks[0].time_ms // $benchmarks[$name].commons.time_ms // null),
            comparison: $benchmarks[$name].comparison
        }])
    )
   ' "$TIMESERIES_FILE" > "$TIMESERIES_FILE.tmp" && mv "$TIMESERIES_FILE.tmp" "$TIMESERIES_FILE"

echo "✅ Time series updated: $TIMESERIES_FILE"

# Generate trend analysis
TREND_FILE="$HISTORY_DIR/trends-$(date +%Y%m%d-%H%M%S).json"
jq '
    .benchmarks | 
    to_entries | 
    map({
        benchmark: .key,
        data_points: (.value | length),
        first_timestamp: (.value[0].timestamp // null),
        last_timestamp: (.value[-1].timestamp // null),
        trends: {
            core: {
                first: (.value[0].core_time_ms // null),
                last: (.value[-1].core_time_ms // null),
                change_percent: (if (.value[0].core_time_ms // 0) > 0 and (.value[-1].core_time_ms // 0) > 0 
                    then ((.value[-1].core_time_ms - .value[0].core_time_ms) / .value[0].core_time_ms * 100)
                    else null end)
            },
            commons: {
                first: (.value[0].commons_time_ms // null),
                last: (.value[-1].commons_time_ms // null),
                change_percent: (if (.value[0].commons_time_ms // 0) > 0 and (.value[-1].commons_time_ms // 0) > 0
                    then ((.value[-1].commons_time_ms - .value[0].commons_time_ms) / .value[0].commons_time_ms * 100)
                    else null end)
            }
        }
    })
' "$TIMESERIES_FILE" > "$TREND_FILE" 2>/dev/null || echo "{}" > "$TREND_FILE"

echo "✅ Trend analysis: $TREND_FILE"
echo ""
echo "📊 Historical data summary:"
echo "  Total entries: $(jq '.entries | length' "$TIMESERIES_FILE" 2>/dev/null || echo "0")"
echo "  Tracked benchmarks: $(jq '.benchmarks | keys | length' "$TIMESERIES_FILE" 2>/dev/null || echo "0")"

