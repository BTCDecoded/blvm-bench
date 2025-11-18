#!/bin/bash
# Validate benchmark JSON output
# Checks for required fields and data quality

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/shared/common.sh"

JSON_FILE="${1:-}"

if [ -z "$JSON_FILE" ]; then
    echo "Usage: $0 <json-file>"
    exit 1
fi

if [ ! -f "$JSON_FILE" ]; then
    echo "❌ File not found: $JSON_FILE"
    exit 1
fi

echo "Validating: $JSON_FILE"
echo ""

ERRORS=0
WARNINGS=0

# Check if valid JSON
if ! jq empty "$JSON_FILE" 2>/dev/null; then
    echo "❌ Invalid JSON"
    exit 1
fi

# Check for timestamp
if ! jq -e '.timestamp' "$JSON_FILE" >/dev/null 2>&1; then
    echo "⚠️  Missing timestamp"
    WARNINGS=$((WARNINGS + 1))
fi

# Check for benchmark data
if jq -e '.benchmarks' "$JSON_FILE" >/dev/null 2>&1; then
    BENCH_COUNT=$(jq '.benchmarks | length' "$JSON_FILE" 2>/dev/null || echo "0")
    if [ "$BENCH_COUNT" = "0" ]; then
        echo "⚠️  No benchmarks found"
        WARNINGS=$((WARNINGS + 1))
    else
        echo "✅ Found $BENCH_COUNT benchmark(s)"
    fi
fi

# Check for timing data
HAS_TIMING=$(jq '[.. | select(type == "number" and . > 0)] | length' "$JSON_FILE" 2>/dev/null || echo "0")
if [ "$HAS_TIMING" = "0" ]; then
    echo "❌ No timing data found"
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Found timing data"
fi

# Summary
echo ""
if [ $ERRORS -eq 0 ] && [ $WARNINGS -eq 0 ]; then
    echo "✅ Validation passed"
    exit 0
elif [ $ERRORS -eq 0 ]; then
    echo "⚠️  Validation passed with $WARNINGS warning(s)"
    exit 0
else
    echo "❌ Validation failed with $ERRORS error(s) and $WARNINGS warning(s)"
    exit 1
fi

