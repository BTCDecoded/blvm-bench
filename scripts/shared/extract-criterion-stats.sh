#!/bin/bash
# Extract detailed statistical data from Criterion estimates.json
# Returns comprehensive statistics including percentiles, confidence intervals, etc.

set -e

ESTIMATES_JSON="${1:-}"

if [ -z "$ESTIMATES_JSON" ] || [ ! -f "$ESTIMATES_JSON" ]; then
    echo "{}"
    exit 0
fi

# Extract comprehensive statistics
jq -c '{
    mean: {
        point_estimate: .mean.point_estimate,
        confidence_interval: {
            lower_bound: .mean.confidence_interval.lower_bound,
            upper_bound: .mean.confidence_interval.upper_bound,
            confidence_level: .mean.confidence_interval.confidence_level
        },
        standard_error: .mean.standard_error
    },
    median: {
        point_estimate: .median.point_estimate,
        confidence_interval: {
            lower_bound: .median.confidence_interval.lower_bound,
            upper_bound: .median.confidence_interval.upper_bound,
            confidence_level: .median.confidence_interval.confidence_level
        },
        standard_error: .median.standard_error
    },
    std_dev: .std_dev.point_estimate,
    median_abs_dev: .median_abs_dev.point_estimate,
    // Extract percentiles if available
    percentiles: {
        p50: .median.point_estimate,
        p75: (if .percentiles then .percentiles."0.75" // .percentiles.p75 // null else null end),
        p90: (if .percentiles then .percentiles."0.90" // .percentiles.p90 // null else null end),
        p95: (if .percentiles then .percentiles."0.95" // .percentiles.p95 // null else null end),
        p99: (if .percentiles then .percentiles."0.99" // .percentiles.p99 // null else null end),
        p999: (if .percentiles then .percentiles."0.999" // .percentiles.p999 // null else null end)
    },
    // Extract min/max if available
    min: (if .min then .min.point_estimate else null end),
    max: (if .max then .max.point_estimate else null end),
    // Sample count
    sample_count: (if .sample_count then .sample_count else null end)
}' "$ESTIMATES_JSON" 2>/dev/null || echo "{}"
