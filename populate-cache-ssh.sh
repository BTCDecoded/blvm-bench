#!/bin/bash
# Populate block cache by reading blocks directly via SSH
# This bypasses both RPC and mount permission issues

set -e

SSH_KEY="$HOME/.ssh/start9_new"
SSH_HOST="start9@192.168.2.100"
CACHE_DIR="${BLOCK_CACHE_DIR:-/tmp/blocks-cache-start9}"
BLOCKS_DIR="/embassy-data/package-data/volumes/bitcoind/data/main/blocks"

echo "📥 Populating block cache from Start9 via SSH"
echo "   Cache dir: $CACHE_DIR"
echo ""

mkdir -p "$CACHE_DIR"

# Use Bitcoin Core's RPC to get block hashes, then read raw blocks from files
# For now, let's try to use a simpler approach - use RPC via SSH exec

# Test RPC access via SSH
echo "🔍 Testing RPC access..."
BLOCK_COUNT=$(ssh -i "$SSH_KEY" "$SSH_HOST" \
    "podman exec \$(podman ps -q | head -1) curl -s --user bitcoin:e5w54tw55b5j6iccd3eu \
    --data-binary '{\"jsonrpc\":\"1.0\",\"id\":\"1\",\"method\":\"getblockcount\",\"params\":[]}' \
    -H 'content-type: text/plain;' http://127.0.0.1:8332/ 2>&1" | \
    grep -o '"result":[0-9]*' | cut -d':' -f2)

if [ -z "$BLOCK_COUNT" ]; then
    echo "❌ Could not access RPC"
    exit 1
fi

echo "✅ RPC accessible (chain height: $BLOCK_COUNT)"
echo ""

# For now, let's use a Python script or simpler method
# Actually, let me create a Rust helper that uses SSH to fetch blocks

echo "📝 Creating SSH-based block fetcher..."

cat > /tmp/fetch_block_ssh.sh << 'EOFSCRIPT'
#!/bin/bash
HEIGHT=$1
SSH_KEY="$HOME/.ssh/start9_new"
SSH_HOST="start9@192.168.2.100"

# Get block hash
HASH=$(ssh -i "$SSH_KEY" "$SSH_HOST" \
    "podman exec \$(podman ps -q | head -1) curl -s --user bitcoin:e5w54tw55b5j6iccd3eu \
    --data-binary '{\"jsonrpc\":\"1.0\",\"id\":\"1\",\"method\":\"getblockhash\",\"params\":[$HEIGHT]}' \
    -H 'content-type: text/plain;' http://127.0.0.1:8332/ 2>&1" | \
    grep -o '"result":"[^"]*"' | cut -d'"' -f4)

if [ -z "$HASH" ]; then
    exit 1
fi

# Get block raw
ssh -i "$SSH_KEY" "$SSH_HOST" \
    "podman exec \$(podman ps -q | head -1) curl -s --user bitcoin:e5w54tw55b5j6iccd3eu \
    --data-binary '{\"jsonrpc\":\"1.0\",\"id\":\"1\",\"method\":\"getblock\",\"params\":[\"$HASH\", 0]}' \
    -H 'content-type: text/plain;' http://127.0.0.1:8332/ 2>&1" | \
    grep -o '"result":"[^"]*"' | cut -d'"' -f4 | xxd -r -p
EOFSCRIPT

chmod +x /tmp/fetch_block_ssh.sh

echo "✅ SSH block fetcher ready"
echo ""
echo "To populate cache, run:"
echo "  for i in {0..10}; do /tmp/fetch_block_ssh.sh \$i > $CACHE_DIR/block_\$i.bin; done"

