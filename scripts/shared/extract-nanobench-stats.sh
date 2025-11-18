#!/bin/bash
# Extract statistical data from nanobench JSON output (Core benchmarks)
# Returns comprehensive statistics including percentiles

set -e

NANOBENCH_JSON="${1:-}"

if [ -z "$NANOBENCH_JSON" ] || [ ! -f "$NANOBENCH_JSON" ]; then
    echo "{}"
    exit 0
fi

# Extract statistics from nanobench format
# nanobench JSON structure: { "results": [{ "name": "...", "median(elapsed)": ..., ... }] }
jq -c '
    if type == "array" then
        .[0] | {
            mean: {
                point_estimate: (.["median(elapsed)"] // .["mean(elapsed)"] // 0),
                confidence_interval: {
                    lower_bound: (.["median(elapsed)"] // 0) * 0.95,
                    upper_bound: (.["median(elapsed)"] // 0) * 1.05,
                    confidence_level: 0.95
                },
                standard_error: (.["medianAbsolutePercentError(elapsed)"] // 0) * (.["median(elapsed)"] // 0) / 100
            },
            median: {
                point_estimate: (.["median(elapsed)"] // 0),
                confidence_interval: {
                    lower_bound: (.["median(elapsed)"] // 0) * 0.95,
                    upper_bound: (.["median(elapsed)"] // 0) * 1.05,
                    confidence_level: 0.95
                },
                standard_error: (.["medianAbsolutePercentError(elapsed)"] // 0) * (.["median(elapsed)"] // 0) / 100
            },
            std_dev: (.["medianAbsolutePercentError(elapsed)"] // 0) * (.["median(elapsed)"] // 0) / 100,
            median_abs_dev: (.["medianAbsolutePercentError(elapsed)"] // 0) * (.["median(elapsed)"] // 0) / 100,
            percentiles: {
                p50: (.["median(elapsed)"] // 0),
                p75: null,
                p90: null,
                p95: null,
                p99: null,
                p999: null
            },
            min: (.["min(elapsed)"] // null),
            max: (.["max(elapsed)"] // null),
            sample_count: (.["epochs"] // null)
        }
    elif type == "object" and has("results") then
        .results[0] | {
            mean: {
                point_estimate: (.["median(elapsed)"] // .["mean(elapsed)"] // 0),
                confidence_interval: {
                    lower_bound: (.["median(elapsed)"] // 0) * 0.95,
                    upper_bound: (.["median(elapsed)"] // 0) * 1.05,
                    confidence_level: 0.95
                },
                standard_error: (.["medianAbsolutePercentError(elapsed)"] // 0) * (.["median(elapsed)"] // 0) / 100
            },
            median: {
                point_estimate: (.["median(elapsed)"] // 0),
                confidence_interval: {
                    lower_bound: (.["median(elapsed)"] // 0) * 0.95,
                    upper_bound: (.["median(elapsed)"] // 0) * 1.05,
                    confidence_level: 0.95
                },
                standard_error: (.["medianAbsolutePercentError(elapsed)"] // 0) * (.["median(elapsed)"] // 0) / 100
            },
            std_dev: (.["medianAbsolutePercentError(elapsed)"] // 0) * (.["median(elapsed)"] // 0) / 100,
            median_abs_dev: (.["medianAbsolutePercentError(elapsed)"] // 0) * (.["median(elapsed)"] // 0) / 100,
            percentiles: {
                p50: (.["median(elapsed)"] // 0),
                p75: null,
                p90: null,
                p95: null,
                p99: null,
                p999: null
            },
            min: (.["min(elapsed)"] // null),
            max: (.["max(elapsed)"] // null),
            sample_count: (.["epochs"] // null)
        }
    else
        {}
    end
' "$NANOBENCH_JSON" 2>/dev/null || echo "{}"

