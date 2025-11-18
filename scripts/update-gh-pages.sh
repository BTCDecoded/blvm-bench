#!/bin/bash
# Update GitHub Pages with latest benchmark data
# Copies consolidated JSON to docs/data/ for static site

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BLLVM_BENCH_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
source "$SCRIPT_DIR/shared/common.sh"

RESULTS_DIR="$BLLVM_BENCH_ROOT/results"
DOCS_DATA_DIR="$BLLVM_BENCH_ROOT/docs/data"

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  Updating GitHub Pages Data                                  ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# Ensure consolidated JSON exists
if [ ! -f "$BLLVM_BENCH_ROOT/scripts/generate-consolidated-json.sh" ]; then
    echo "❌ generate-consolidated-json.sh not found"
    exit 1
fi

# Generate consolidated JSON if needed
echo "Generating consolidated JSON..."
"$BLLVM_BENCH_ROOT/scripts/generate-consolidated-json.sh"

# Find latest consolidated JSON
LATEST_JSON="$RESULTS_DIR/benchmark-results-consolidated-latest.json"

if [ ! -f "$LATEST_JSON" ]; then
    echo "❌ No consolidated JSON found at $LATEST_JSON. Run 'make json' first."
    exit 1
fi

echo "Found: $LATEST_JSON"

# Ensure docs/data directory exists
mkdir -p "$DOCS_DATA_DIR"

# Copy to docs/data with standardized name
TARGET_FILE="$DOCS_DATA_DIR/benchmark-results-consolidated-latest.json"
cp "$LATEST_JSON" "$TARGET_FILE"

echo "✅ Copied to: $TARGET_FILE"
echo ""
echo "Next steps:"
echo "  1. Review the JSON: $TARGET_FILE"
echo "  2. Commit and push:"
echo "     git add docs/data/"
echo "     git commit -m 'Update benchmark data'"
echo "     git push"
echo ""
echo "The site will update automatically via GitHub Pages."

