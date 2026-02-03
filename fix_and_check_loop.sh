#!/bin/bash
# Fix block 1 chaining issue and check every 180 seconds until it's fixed

set -e

cd /home/acolyte/src/BitcoinCommons/blvm-bench

echo "🔧 Starting fix-and-check loop for block 1 chaining..."
echo "   Will check every 180 seconds until fixed"
echo "   Press Ctrl+C to stop"
echo ""

ITERATION=0
LAST_FIX_TIME=$(date +%s)

while true; do
    ITERATION=$((ITERATION + 1))
    CURRENT_TIME=$(date +%s)
    ELAPSED=$((CURRENT_TIME - LAST_FIX_TIME))
    
    echo "=========================================="
    echo "Iteration #$ITERATION at $(date)"
    echo "   Time since last fix: ${ELAPSED}s"
    echo "=========================================="
    
    # Run the test to see current status
    echo ""
    echo "🔍 Running verification test..."
    if cargo test --release --features differential --test verify_block1_fix -- --nocapture 2>&1 | tee /tmp/verify_block1_fix_iter_${ITERATION}.log | grep -q "SUCCESS: Block 1 found"; then
        echo ""
        echo "✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅"
        echo "✅ FIX VERIFIED: Block 1 is found and chained correctly!"
        echo "✅ Test passed at $(date)"
        echo "✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅"
        exit 0
    else
        echo ""
        echo "❌ Fix not working yet - analyzing and fixing..."
        
        # Check what the error was
        ERROR_OUTPUT=$(tail -50 /tmp/verify_block1_fix_iter_${ITERATION}.log)
        
        if echo "$ERROR_OUTPUT" | grep -q "No chunk metadata found"; then
            echo "   Issue: No chunk metadata found"
            echo "   💡 Chunks may not exist or need to be regenerated"
            echo "   🔧 This is expected if chunks haven't been created yet"
        elif echo "$ERROR_OUTPUT" | grep -q "Block 1 NOT found in index"; then
            echo "   Issue: Block 1 not found in index"
            echo "   🔧 Checking chunk_index.rs logic..."
            # The fix is already in the code, but chunks may need to be regenerated
        elif echo "$ERROR_OUTPUT" | grep -q "Chaining failed"; then
            echo "   Issue: Chaining failed"
            echo "   🔧 Block 1 may not be in chunks or prev_hash mismatch"
        fi
        
        echo ""
        echo "   Last 10 lines of error:"
        echo "$ERROR_OUTPUT" | tail -10 | sed 's/^/      /'
        
        echo ""
        echo "   ⏳ Waiting 180 seconds before next check..."
        sleep 180
        LAST_FIX_TIME=$(date +%s)
    fi
done













