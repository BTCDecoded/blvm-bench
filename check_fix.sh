#!/bin/bash
# Check every 180 seconds if block 1 fix is working

set -e

cd /home/acolyte/src/BitcoinCommons/blvm-bench

echo "🔍 Starting continuous verification loop (checking every 180 seconds)..."
echo "   Press Ctrl+C to stop"

ITERATION=0
while true; do
    ITERATION=$((ITERATION + 1))
    echo ""
    echo "=========================================="
    echo "Check #$ITERATION at $(date)"
    echo "=========================================="
    
    if cargo test --release --features differential --test verify_block1_fix -- --nocapture 2>&1 | tee /tmp/verify_block1_fix_check.log | grep -q "SUCCESS: Block 1 found"; then
        echo ""
        echo "✅✅✅ FIX VERIFIED: Block 1 is found and chained correctly! ✅✅✅"
        echo "   Test passed at $(date)"
        exit 0
    else
        echo ""
        echo "❌ Fix not working yet - will check again in 180 seconds..."
        echo "   Last error output:"
        tail -20 /tmp/verify_block1_fix_check.log | grep -E "FAILURE|Error|❌" || echo "   (no errors in last 20 lines)"
        sleep 180
    fi
done























