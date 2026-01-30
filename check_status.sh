#!/bin/bash
STATUS_FILE="/tmp/block1_fix_status.txt"
LOG_FILE="/tmp/block1_fix_monitor.log"
echo "=== Block 1 Fix Monitor Status ==="
if [ -f "$STATUS_FILE" ]; then
    cat "$STATUS_FILE"
else
    echo "Status: Monitor starting or not running"
fi
echo ""
echo "Recent activity:"
tail -15 "$LOG_FILE" 2>/dev/null || echo "No log yet"
echo ""
ps aux | grep "[m]onitor_block1" | head -1 || echo "Monitor process not found"










