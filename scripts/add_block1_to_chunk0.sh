#!/bin/bash
# Script to add missing block 1 to chunk_0
# Block 1 is missing from Start9 files, so we get it from RPC

set -e

CHUNK_DIR="/run/media/acolyte/Extra/blockchain"
BLOCK1_HASH="00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048"

echo "Getting block 1 from Bitcoin Core RPC..."
if ! command -v bitcoin-cli &> /dev/null; then
    echo "ERROR: bitcoin-cli not found. Cannot get block 1."
    exit 1
fi

# Get block 1 raw hex
BLOCK1_HEX=$(bitcoin-cli getblock "$BLOCK1_HASH" 0)
if [ -z "$BLOCK1_HEX" ]; then
    echo "ERROR: Failed to get block 1 from RPC"
    exit 1
fi

echo "Block 1 retrieved (${#BLOCK1_HEX} hex chars)"
echo "Converting to binary and adding to chunk_0..."

# Convert hex to binary
BLOCK1_BIN=$(echo "$BLOCK1_HEX" | xxd -r -p)

# Get block length (little-endian u32)
BLOCK1_LEN=${#BLOCK1_HEX}
BLOCK1_LEN=$((BLOCK1_LEN / 2))  # Convert hex chars to bytes
LEN_BYTES=$(printf "%08x" $BLOCK1_LEN | sed 's/\(..\)\(..\)\(..\)\(..\)/\4\3\2\1/' | xxd -r -p)

# Decompress chunk_0
echo "Decompressing chunk_0..."
zstd -d --stdout "$CHUNK_DIR/chunk_0.bin.zst" > /tmp/chunk_0_decompressed.bin

# Prepend block 1 to chunk_0
echo "Adding block 1 to beginning of chunk_0..."
cat <(echo -n "$LEN_BYTES") <(echo -n "$BLOCK1_BIN") /tmp/chunk_0_decompressed.bin > /tmp/chunk_0_with_block1.bin

# Recompress
echo "Recompressing chunk_0..."
zstd -3 -f /tmp/chunk_0_with_block1.bin -o "$CHUNK_DIR/chunk_0.bin.zst"

# Cleanup
rm -f /tmp/chunk_0_decompressed.bin /tmp/chunk_0_with_block1.bin

echo "✅ Block 1 added to chunk_0!"
























