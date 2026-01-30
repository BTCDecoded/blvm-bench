#!/bin/bash
# Monitor cache resume progress

LOG_FILE=$(ls -t /tmp/start9-cache-resume-*.log 2>/dev/null | head -1)
TEMP_FILE="$HOME/.cache/blvm-bench/blvm-bench-blocks-temp.bin"
META_FILE="$HOME/.cache/blvm-bench/blvm-bench-blocks-temp.bin.meta"
CHUNKS_DIR="/run/media/acolyte/Extra/blockchain"

echo "📊 Cache Resume Status Monitor"
echo "=============================="
echo ""

# Check if process is running
if pgrep -f "cargo test.*start9" > /dev/null; then
    echo "✅ Process: Running"
else
    echo "❌ Process: Not running"
fi

# Check temp file
if [ -f "$TEMP_FILE" ]; then
    SIZE=$(stat -c%s "$TEMP_FILE" 2>/dev/null)
    SIZE_GB=$(echo "scale=2; $SIZE / 1073741824" | bc)
    echo "📦 Temp file: ${SIZE_GB} GB"
else
    echo "📦 Temp file: Not found"
fi

# Check metadata
if [ -f "$META_FILE" ]; then
    BLOCKS=$(od -An -tu8 "$META_FILE" | tr -d ' ')
    echo "📊 Blocks in temp: ${BLOCKS}"
    
    # Calculate which chunk we're on
    CHUNK_NUM=$((BLOCKS / 125000))
    NEXT_CHUNK=$((CHUNK_NUM + 1))
    BLOCKS_IN_CURRENT=$((BLOCKS % 125000))
    echo "📦 Current chunk: ${CHUNK_NUM} (${BLOCKS_IN_CURRENT}/125000 blocks collected for chunk ${NEXT_CHUNK})"
else
    echo "📊 Metadata: Not found"
fi

# Check existing chunks
if [ -d "$CHUNKS_DIR" ]; then
    CHUNK_COUNT=$(ls -1 "$CHUNKS_DIR"/chunk_*.bin.zst 2>/dev/null | wc -l)
    if [ "$CHUNK_COUNT" -gt 0 ]; then
        echo "📦 Existing chunks: ${CHUNK_COUNT}"
        ls -lh "$CHUNKS_DIR"/chunk_*.bin.zst 2>/dev/null | awk '{print "   "$9" - "$5}'
    else
        echo "📦 Existing chunks: 0"
    fi
else
    echo "📦 Chunks directory: Not found"
fi

# Check log file
if [ -n "$LOG_FILE" ] && [ -f "$LOG_FILE" ]; then
    echo ""
    echo "📝 Recent log output:"
    echo "---"
    tail -20 "$LOG_FILE" | grep -E "(Progress|blocks|chunk|Resuming|Reading|Collected)" | tail -10
    echo "---"
    echo ""
    echo "📄 Full log: $LOG_FILE"
else
    echo ""
    echo "📝 Log file: Not found"
fi

echo ""
echo "💡 To watch progress in real-time:"
echo "   tail -f $LOG_FILE"

