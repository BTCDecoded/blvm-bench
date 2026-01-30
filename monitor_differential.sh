#!/bin/bash
# Monitor differential test performance and resource usage

PID=$(pgrep -f "cargo test.*differential.*test_historical_blocks_parallel" | head -1)

if [ -z "$PID" ]; then
    echo "❌ No differential test process found"
    exit 1
fi

echo "📊 Differential Test Performance Monitor"
echo "========================================"
echo "Process PID: $PID"
echo ""

# Get CPU info
CPU_CORES=$(nproc)
CPU_MODEL=$(lscpu | grep "Model name" | cut -d: -f2 | xargs)
echo "🖥️  System: $CPU_MODEL ($CPU_CORES cores)"
echo ""

# Monitor loop
while kill -0 $PID 2>/dev/null; do
    clear
    echo "📊 Differential Test Performance Monitor"
    echo "========================================"
    echo "Process PID: $PID"
    echo "Time: $(date '+%Y-%m-%d %H:%M:%S')"
    echo ""
    
    # Process stats
    PS_OUTPUT=$(ps -p $PID -o etime,state,pcpu,pmem,rss,vsz,cmd --no-headers 2>/dev/null)
    if [ -z "$PS_OUTPUT" ]; then
        echo "❌ Process not found (may have completed)"
        break
    fi
    
    ETIME=$(echo $PS_OUTPUT | awk '{print $1}')
    STATE=$(echo $PS_OUTPUT | awk '{print $2}')
    CPU=$(echo $PS_OUTPUT | awk '{print $3}')
    MEM=$(echo $PS_OUTPUT | awk '{print $4}')
    RSS=$(echo $PS_OUTPUT | awk '{print $5}')
    VSZ=$(echo $PS_OUTPUT | awk '{print $6}')
    
    # Convert RSS to MB/GB
    RSS_MB=$((RSS / 1024))
    if [ $RSS_MB -gt 1024 ]; then
        RSS_GB=$(echo "scale=2; $RSS_MB / 1024" | bc)
        RSS_DISPLAY="${RSS_GB}GB"
    else
        RSS_DISPLAY="${RSS_MB}MB"
    fi
    
    echo "⏱️  Runtime: $ETIME"
    echo "💻 CPU: ${CPU}% | Memory: ${MEM}% | RSS: $RSS_DISPLAY"
    echo "📊 State: $STATE"
    echo ""
    
    # System-wide stats
    echo "🖥️  System Resources:"
    FREE_OUTPUT=$(free -h | grep "^Mem:")
    MEM_TOTAL=$(echo $FREE_OUTPUT | awk '{print $2}')
    MEM_USED=$(echo $FREE_OUTPUT | awk '{print $3}')
    MEM_AVAIL=$(echo $FREE_OUTPUT | awk '{print $7}')
    echo "   Memory: $MEM_USED / $MEM_TOTAL (Available: $MEM_AVAIL)"
    
    SWAP_OUTPUT=$(free -h | grep "^Swap:")
    SWAP_TOTAL=$(echo $SWAP_OUTPUT | awk '{print $2}')
    SWAP_USED=$(echo $SWAP_OUTPUT | awk '{print $3}')
    echo "   Swap: $SWAP_USED / $SWAP_TOTAL"
    echo ""
    
    # CPU load average
    LOAD=$(uptime | awk -F'load average:' '{print $2}' | xargs)
    echo "⚡ Load Average: $LOAD"
    echo ""
    
    # Check for child processes (parallel workers)
    CHILD_COUNT=$(pgrep -P $PID 2>/dev/null | wc -l)
    if [ $CHILD_COUNT -gt 0 ]; then
        echo "🔄 Parallel Workers: $CHILD_COUNT active"
        
        # Get CPU usage of all child processes
        CHILD_CPU=$(pgrep -P $PID 2>/dev/null | xargs ps -o pcpu --no-headers 2>/dev/null | awk '{sum+=$1} END {print sum}')
        if [ ! -z "$CHILD_CPU" ]; then
            echo "   Total Worker CPU: ${CHILD_CPU}%"
        fi
        echo ""
    fi
    
    # Temperature check (if available)
    if [ -f /sys/class/thermal/thermal_zone0/temp ]; then
        TEMP=$(cat /sys/class/thermal/thermal_zone0/temp)
        TEMP_C=$((TEMP / 1000))
        echo "🌡️  CPU Temperature: ${TEMP_C}°C"
        if [ $TEMP_C -gt 80 ]; then
            echo "   ⚠️  WARNING: High temperature!"
        fi
        echo ""
    fi
    
    # Check log file for progress
    LOG_FILE=$(ls -t /tmp/differential-*.log 2>/dev/null | head -1)
    if [ ! -z "$LOG_FILE" ] && [ -f "$LOG_FILE" ]; then
        echo "📝 Recent Log Activity:"
        tail -3 "$LOG_FILE" 2>/dev/null | sed 's/^/   /'
        echo ""
    fi
    
    # Warnings
    # CPU warning (using awk instead of bc for compatibility)
    CPU_INT=$(echo "$CPU" | awk -F. '{print $1}')
    if [ "$CPU_INT" -gt 95 ]; then
        echo "⚠️  WARNING: Very high CPU usage (${CPU}%)"
    fi
    
    # Memory warning
    MEM_INT=$(echo "$MEM" | awk -F. '{print $1}')
    if [ "$MEM_INT" -gt 80 ]; then
        echo "⚠️  WARNING: High memory usage (${MEM}%)"
    fi
    
    if [ ! -z "$SWAP_USED" ] && [ "$SWAP_USED" != "0B" ] && [ "$SWAP_USED" != "0" ]; then
        echo "⚠️  WARNING: Swap is being used"
    fi
    
    echo ""
    echo "Press Ctrl+C to stop monitoring"
    sleep 5
done

echo ""
echo "✅ Process completed or stopped"
