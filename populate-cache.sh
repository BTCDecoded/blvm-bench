#!/bin/bash
# Populate block cache from Start9 using podman exec
set -e

SSH_KEY="$HOME/.ssh/start9_new"
SSH_HOST="start9@192.168.2.100"
CACHE_DIR="${BLOCK_CACHE_DIR:-/tmp/blocks-cache-start9}"
START_HEIGHT="${1:-0}"
END_HEIGHT="${2:-10}"

echo "📥 Populating block cache from Start9"
echo "   Cache dir: $CACHE_DIR"
echo "   Range: $START_HEIGHT to $END_HEIGHT"
echo ""

mkdir -p "$CACHE_DIR"

# Find Bitcoin Core container
echo "🔍 Finding Bitcoin Core container..."
CONTAINER_ID=$(ssh -i "$SSH_KEY" "$SSH_HOST" "podman ps --format '{{.ID}}\t{{.Names}}' | grep -i bitcoin | head -1 | cut -f1" 2>&1)

if [ -z "$CONTAINER_ID" ]; then
    # Try any running container
    CONTAINER_ID=$(ssh -i "$SSH_KEY" "$SSH_HOST" "podman ps -q | head -1" 2>&1)
fi

if [ -z "$CONTAINER_ID" ]; then
    echo "❌ Could not find container"
    exit 1
fi

echo "✅ Found container: $CONTAINER_ID"
echo ""

# Fetch blocks
for height in $(seq $START_HEIGHT $END_HEIGHT); do
    CACHE_FILE="$CACHE_DIR/block_${height}.bin"
    
    # Skip if already cached and valid
    if [ -f "$CACHE_FILE" ] && [ $(stat -f%z "$CACHE_FILE" 2>/dev/null || stat -c%s "$CACHE_FILE" 2>/dev/null) -gt 80 ]; then
        if [ $((height % 10)) -eq 0 ]; then
            echo "   Skipping $height (already cached)"
        fi
        continue
    fi
    
    if [ $((height % 10)) -eq 0 ]; then
        echo "   Progress: $height/$END_HEIGHT"
    fi
    
    # Get block hash
    BLOCK_HASH=$(ssh -i "$SSH_KEY" "$SSH_HOST" \
        "podman exec $CONTAINER_ID curl -s --user bitcoin:e5w54tw55b5j6iccd3eu \
        --data-binary '{\"jsonrpc\":\"1.0\",\"id\":\"1\",\"method\":\"getblockhash\",\"params\":[$height]}' \
        -H 'content-type: text/plain;' http://127.0.0.1:8332/ 2>&1" | \
        grep -o '"result":"[^"]*"' | cut -d'"' -f4)
    
    if [ -z "$BLOCK_HASH" ]; then
        echo "⚠️  Failed to get hash for block $height"
        continue
    fi
    
    # Get block raw (hex)
    BLOCK_HEX=$(ssh -i "$SSH_KEY" "$SSH_HOST" \
        "podman exec $CONTAINER_ID curl -s --user bitcoin:e5w54tw55b5j6iccd3eu \
        --data-binary '{\"jsonrpc\":\"1.0\",\"id\":\"1\",\"method\":\"getblock\",\"params\":[\"$BLOCK_HASH\", 0]}' \
        -H 'content-type: text/plain;' http://127.0.0.1:8332/ 2>&1" | \
        grep -o '"result":"[^"]*"' | cut -d'"' -f4)
    
    if [ -z "$BLOCK_HEX" ]; then
        echo "⚠️  Failed to get block $height"
        continue
    fi
    
    # Decode hex and save
    echo "$BLOCK_HEX" | xxd -r -p > "$CACHE_FILE"
    
    if [ -f "$CACHE_FILE" ] && [ $(stat -f%z "$CACHE_FILE" 2>/dev/null || stat -c%s "$CACHE_FILE" 2>/dev/null) -gt 80 ]; then
        echo "✅ Block $height cached ($(stat -f%z "$CACHE_FILE" 2>/dev/null || stat -c%s "$CACHE_FILE" 2>/dev/null) bytes)"
    else
        echo "⚠️  Block $height cache file invalid"
        rm -f "$CACHE_FILE"
    fi
done

echo ""
echo "✅ Cache populated"
echo "   Cache dir: $CACHE_DIR"

