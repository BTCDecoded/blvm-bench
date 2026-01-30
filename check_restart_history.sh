#!/bin/bash
# Check restart history from monitor log
# Usage: ./check_restart_history.sh [number-of-restarts-to-show]

LOG_FILE="/tmp/cache-build-monitor.log"
NUM_RESTARTS=${1:-10}

if [ ! -f "$LOG_FILE" ]; then
    echo "❌ Monitor log not found: $LOG_FILE"
    exit 1
fi

echo "📊 RESTART HISTORY (last $NUM_RESTARTS restarts)"
echo "=" * 70

# Extract restart events
grep -E "(Restart attempt|Process appears to be STUCK|Killing stuck process|restarted successfully)" "$LOG_FILE" | tail -$((NUM_RESTARTS * 4)) | while read -r line; do
    echo "$line"
done

echo ""
echo "📈 Summary:"
echo "  Total restarts: $(grep -c "Restart attempt" "$LOG_FILE" 2>/dev/null || echo 0)"
echo "  Total stuck detections: $(grep -c "Process appears to be STUCK" "$LOG_FILE" 2>/dev/null || echo 0)"
echo ""
echo "🔍 Recent stuck reasons:"
grep -A 5 "Process appears to be STUCK" "$LOG_FILE" | tail -20




