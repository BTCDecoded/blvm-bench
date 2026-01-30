#!/bin/bash
# Quick test script for 100 blocks
# Tests both RPC and direct file reading (if mounted)

set -e

cd "$(dirname "$0")"

echo "🧪 Testing differential validation with 100 blocks"
echo ""

# Load environment
source start9-env.sh

# Check if mount is available
if mountpoint -q ~/mnt/bitcoin-start9 2>/dev/null; then
    echo "✅ Direct file mount detected - will use for faster performance"
    MOUNT_AVAILABLE=true
else
    echo "📡 Using RPC (tunnel should be running)"
    echo "   To use direct file mount: sudo pacman -S fuse-sshfs && ./mount-start9.sh"
    MOUNT_AVAILABLE=false
fi

echo ""
echo "⚙️  Configuration:"
echo "   Blocks: 0-100"
echo "   Workers: 4"
echo "   Chunk size: 50"
echo ""

# Run test
HISTORICAL_BLOCK_START=0 \
HISTORICAL_BLOCK_END=100 \
PARALLEL_WORKERS=4 \
CHUNK_SIZE=50 \
cargo test --test integration --features differential test_historical_blocks_parallel -- --nocapture

echo ""
echo "✅ Test complete!"

# Quick test script for 100 blocks
# Tests both RPC and direct file reading (if mounted)

set -e

cd "$(dirname "$0")"

echo "🧪 Testing differential validation with 100 blocks"
echo ""

# Load environment
source start9-env.sh

# Check if mount is available
if mountpoint -q ~/mnt/bitcoin-start9 2>/dev/null; then
    echo "✅ Direct file mount detected - will use for faster performance"
    MOUNT_AVAILABLE=true
else
    echo "📡 Using RPC (tunnel should be running)"
    echo "   To use direct file mount: sudo pacman -S fuse-sshfs && ./mount-start9.sh"
    MOUNT_AVAILABLE=false
fi

echo ""
echo "⚙️  Configuration:"
echo "   Blocks: 0-100"
echo "   Workers: 4"
echo "   Chunk size: 50"
echo ""

# Run test
HISTORICAL_BLOCK_START=0 \
HISTORICAL_BLOCK_END=100 \
PARALLEL_WORKERS=4 \
CHUNK_SIZE=50 \
cargo test --test integration --features differential test_historical_blocks_parallel -- --nocapture

echo ""
echo "✅ Test complete!"

