#!/bin/bash
# Continuous monitoring of block 1 fix - checks every 180 seconds

cd /home/acolyte/src/BitcoinCommons/blvm-bench

LOG_FILE="/tmp/block1_fix_monitor.log"
STATUS_FILE="/tmp/block1_fix_status.txt"
CHECK_INTERVAL=180

echo "==========================================" | tee -a "$LOG_FILE"
echo "Block 1 Fix Monitor Started" | tee -a "$LOG_FILE"
echo "Started at: $(date)" | tee -a "$LOG_FILE"
echo "Check interval: ${CHECK_INTERVAL} seconds" | tee -a "$LOG_FILE"
echo "Log file: $LOG_FILE" | tee -a "$LOG_FILE"
echo "Status file: $STATUS_FILE" | tee -a "$LOG_FILE"
echo "==========================================" | tee -a "$LOG_FILE"
echo ""

ITERATION=0
LAST_SUCCESS_TIME=""
FIRST_SUCCESS_TIME=""

while true; do
    ITERATION=$((ITERATION + 1))
    CHECK_TIME=$(date '+%Y-%m-%d %H:%M:%S')
    
    echo "[$CHECK_TIME] Check #$ITERATION starting..." | tee -a "$LOG_FILE"
    
    # Run the test and capture output
    TEST_OUTPUT=$(cargo test --release --features differential --test verify_block1_fix -- --nocapture 2>&1)
    TEST_EXIT_CODE=$?
    
    # Check if test passed
    if echo "$TEST_OUTPUT" | grep -q "SUCCESS: Block 1 found"; then
        if [ -z "$FIRST_SUCCESS_TIME" ]; then
            FIRST_SUCCESS_TIME="$CHECK_TIME"
            echo "" | tee -a "$LOG_FILE"
            echo "🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉" | tee -a "$LOG_FILE"
            echo "✅✅✅ BLOCK 1 FIX VERIFIED - TEST PASSING! ✅✅✅" | tee -a "$LOG_FILE"
            echo "First success at: $FIRST_SUCCESS_TIME" | tee -a "$LOG_FILE"
            echo "🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉🎉" | tee -a "$LOG_FILE"
            echo "" | tee -a "$LOG_FILE"
        fi
        
        LAST_SUCCESS_TIME="$CHECK_TIME"
        STATUS="✅ WORKING - Block 1 found and chained correctly"
        echo "[$CHECK_TIME] ✅ PASS - Block 1 fix is working!" | tee -a "$LOG_FILE"
        
        # Extract key info from test output
        BLOCK1_INFO=$(echo "$TEST_OUTPUT" | grep -A 5 "SUCCESS: Block 1 found" | head -6)
        echo "$BLOCK1_INFO" | tee -a "$LOG_FILE"
        
    elif echo "$TEST_OUTPUT" | grep -q "No chunk metadata found"; then
        STATUS="⏳ WAITING - Chunks not created yet (expected)"
        echo "[$CHECK_TIME] ⏳ Chunks not available yet (this is expected if chunks haven't been created)" | tee -a "$LOG_FILE"
        
    elif echo "$TEST_OUTPUT" | grep -q "Block 1 NOT found in index"; then
        STATUS="❌ FAILING - Block 1 not found in index"
        echo "[$CHECK_TIME] ❌ FAIL - Block 1 not found in index!" | tee -a "$LOG_FILE"
        echo "Error details:" | tee -a "$LOG_FILE"
        echo "$TEST_OUTPUT" | grep -A 10 "Block 1 NOT found" | head -15 | tee -a "$LOG_FILE"
        
    elif echo "$TEST_OUTPUT" | grep -q "Chaining failed"; then
        STATUS="❌ FAILING - Chaining failed"
        echo "[$CHECK_TIME] ❌ FAIL - Chaining failed!" | tee -a "$LOG_FILE"
        echo "$TEST_OUTPUT" | grep -A 5 "Chaining failed" | head -10 | tee -a "$LOG_FILE"
        
    else
        STATUS="❓ UNKNOWN - Check logs for details"
        echo "[$CHECK_TIME] ❓ UNKNOWN status - check full output" | tee -a "$LOG_FILE"
        echo "Last 20 lines of output:" | tee -a "$LOG_FILE"
        echo "$TEST_OUTPUT" | tail -20 | tee -a "$LOG_FILE"
    fi
    
    # Update status file
    cat > "$STATUS_FILE" <<EOF
Block 1 Fix Monitor Status
==========================
Last check: $CHECK_TIME
Iteration: $ITERATION
Status: $STATUS
First success: ${FIRST_SUCCESS_TIME:-Not yet}
Last success: ${LAST_SUCCESS_TIME:-Not yet}
Check interval: ${CHECK_INTERVAL} seconds

Full log: $LOG_FILE
EOF
    
    # Show status
    echo "" | tee -a "$LOG_FILE"
    echo "Current status: $STATUS" | tee -a "$LOG_FILE"
    if [ -n "$LAST_SUCCESS_TIME" ]; then
        echo "Last success: $LAST_SUCCESS_TIME" | tee -a "$LOG_FILE"
    fi
    echo "Next check in ${CHECK_INTERVAL} seconds..." | tee -a "$LOG_FILE"
    echo "----------------------------------------" | tee -a "$LOG_FILE"
    echo "" | tee -a "$LOG_FILE"
    
    sleep "$CHECK_INTERVAL"
done























