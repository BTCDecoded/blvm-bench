#!/bin/bash
# Check progress of Start9 block cache creation

CACHE_FILE="$HOME/.cache/blvm-bench/start9_ordered_blocks.bin"

if [ ! -f "$CACHE_FILE" ]; then
    echo "❌ Cache file not found: $CACHE_FILE"
    echo "   Cache creation has not started yet or is in progress."
    exit 1
fi

CACHE_SIZE=$(stat -f%z "$CACHE_FILE" 2>/dev/null || stat -c%s "$CACHE_FILE" 2>/dev/null)
CACHE_SIZE_MB=$((CACHE_SIZE / 1048576))
CACHE_SIZE_GB=$(echo "scale=2; $CACHE_SIZE / 1073741824" | bc)

# Try to read block count from cache
if [ $CACHE_SIZE -ge 8 ]; then
    # Read first 8 bytes (block count)
    BLOCK_COUNT_HEX=$(dd if="$CACHE_FILE" bs=1 count=8 2>/dev/null | od -An -tx1 | tr -d ' \n')
    # Convert little-endian hex to decimal (simplified - just show hex for now)
    echo "📊 Cache Status:"
    echo "   File: $CACHE_FILE"
    echo "   Size: ${CACHE_SIZE_MB} MB (${CACHE_SIZE_GB} GB)"
    echo "   Block count header: $BLOCK_COUNT_HEX (first 8 bytes)"
    echo ""
    echo "💡 To see actual progress, check the test output for progress messages."
else
    echo "⚠️  Cache file exists but is too small (${CACHE_SIZE} bytes)"
    echo "   Cache creation may be in progress or failed."
fi

