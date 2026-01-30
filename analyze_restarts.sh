#!/bin/bash
# Analyze why the process keeps restarting
# Usage: ./analyze_restarts.sh

LOG_FILE="/tmp/cache-build-monitor.log"

echo "📊 RESTART ANALYSIS"
echo "======================================================================"
echo ""

if [ ! -f "$LOG_FILE" ]; then
    echo "❌ Monitor log not found: $LOG_FILE"
    exit 1
fi

echo "1. RESTART FREQUENCY:"
echo "   Total restarts: $(grep -c "Restart attempt" "$LOG_FILE" 2>/dev/null || echo 0)"
echo "   Total stuck detections: $(grep -c "Process appears to be STUCK" "$LOG_FILE" 2>/dev/null || echo 0)"
echo ""

echo "2. STUCK REASONS (most common):"
grep -A 2 "Process appears to be STUCK" "$LOG_FILE" | grep "  -" | sort | uniq -c | sort -rn | head -10
echo ""

echo "3. RECENT RESTART PATTERN:"
echo "   Last 5 restarts with timestamps:"
grep "Restart attempt\|Process appears to be STUCK" "$LOG_FILE" | tail -10 | while read -r line; do
    echo "   $line"
done
echo ""

echo "4. TIME BETWEEN RESTARTS:"
restart_times=$(grep "Restart attempt" "$LOG_FILE" | tail -5 | while read -r line; do
    echo "$line" | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}'
done)
if [ -n "$restart_times" ]; then
    echo "$restart_times" | awk 'NR>1 {print "   " prev " -> " $0} {prev=$0}'
else
    echo "   No restart pattern found"
fi
echo ""

echo "5. PROCESS STATES BEFORE RESTART:"
grep -B 3 "Killing stuck process" "$LOG_FILE" | grep "Process state\|Process runtime" | tail -10
echo ""

echo "6. RECOMMENDATIONS:"
stuck_count=$(grep -c "Process appears to be STUCK" "$LOG_FILE" 2>/dev/null || echo 0)
if [ "$stuck_count" -gt 10 ]; then
    echo "   ⚠️  High restart frequency detected ($stuck_count restarts)"
    echo "   Consider:"
    echo "     - Increasing stuck threshold (currently 3600s = 1 hour)"
    echo "     - Checking SSHFS mount health"
    echo "     - Verifying process is actually stuck vs. just slow in sparse regions"
fi




