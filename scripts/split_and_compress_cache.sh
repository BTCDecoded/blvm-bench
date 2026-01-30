#!/bin/bash
# Split and compress block cache into chunks
# VERY CAREFUL: Only processes temp file, preserves all work

set -euo pipefail

TEMP_FILE="$HOME/.cache/blvm-bench/blvm-bench-blocks-temp.bin"
META_FILE="$TEMP_FILE.meta"
ORDERED_CACHE="$HOME/.cache/blvm-bench/start9_ordered_blocks.bin"
CACHE_DIR="$HOME/.cache/blvm-bench"
CHUNKS_DIR="$CACHE_DIR/chunks"
NUM_CHUNKS=4

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log() {
    echo -e "${GREEN}[$(date '+%Y-%m-%d %H:%M:%S')]${NC} $*"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

error() {
    echo -e "${RED}[ERROR]${NC} $*"
    exit 1
}

# Verify temp file exists
if [ ! -f "$TEMP_FILE" ]; then
    error "Temp file not found: $TEMP_FILE"
fi

# Read block count from metadata
if [ ! -f "$META_FILE" ]; then
    error "Metadata file not found: $META_FILE"
fi

BLOCK_COUNT=$(python3 -c "
import struct
with open('$META_FILE', 'rb') as f:
    data = f.read(8)
    if len(data) == 8:
        print(struct.unpack('<Q', data)[0])
    else:
        print(0)
")

if [ "$BLOCK_COUNT" -eq 0 ]; then
    error "Could not read block count from metadata"
fi

log "Found temp file with $BLOCK_COUNT blocks"
TEMP_SIZE=$(du -h "$TEMP_FILE" | cut -f1)
log "Temp file size: $TEMP_SIZE"

# Calculate blocks per chunk
BLOCKS_PER_CHUNK=$((BLOCK_COUNT / NUM_CHUNKS))
log "Splitting into $NUM_CHUNKS chunks (~$BLOCKS_PER_CHUNK blocks each)"

# Create chunks directory
mkdir -p "$CHUNKS_DIR"
log "Created chunks directory: $CHUNKS_DIR"

# Check if zstd is available
if ! command -v zstd &> /dev/null; then
    error "zstd not found. Install with: sudo pacman -S zstd"
fi

log "Using zstd compression (fast, good ratio)"

# Function to split and compress one chunk
split_chunk() {
    local chunk_num=$1
    local start_block=$2
    local num_blocks=$3
    local output_file="$CHUNKS_DIR/chunk_${chunk_num}.bin.zst"
    
    log "Processing chunk $chunk_num (blocks $start_block to $((start_block + num_blocks - 1)))"
    
    # Read blocks from temp file and write to chunk
    # Format: [len:u32][data...][len:u32][data...]...
    python3 << PYTHON_EOF
import struct
import sys

temp_file = "$TEMP_FILE"
output_file = "$output_file"
start_block = $start_block
num_blocks = $num_blocks
chunk_num = $chunk_num

# Open temp file
with open(temp_file, 'rb') as f_in:
    # Skip to start block
    blocks_skipped = 0
    while blocks_skipped < start_block:
        # Read block length
        len_bytes = f_in.read(4)
        if len(len_bytes) < 4:
            print(f"Error: Reached end of file at block {blocks_skipped}", file=sys.stderr)
            sys.exit(1)
        block_len = struct.unpack('<I', len_bytes)[0]
        # Skip block data
        f_in.seek(block_len, 1)
        blocks_skipped += 1
    
    # Read num_blocks blocks
    import subprocess
    zstd_proc = subprocess.Popen(['zstd', '-1', '--stdout'], stdin=subprocess.PIPE, stdout=open(output_file, 'wb'))
    
    blocks_read = 0
    while blocks_read < num_blocks:
        # Read block length
        len_bytes = f_in.read(4)
        if len(len_bytes) < 4:
            break
        block_len = struct.unpack('<I', len_bytes)[0]
        
        # Read block data
        block_data = f_in.read(block_len)
        if len(block_data) < block_len:
            break
        
        # Write to zstd (length + data)
        zstd_proc.stdin.write(len_bytes)
        zstd_proc.stdin.write(block_data)
        blocks_read += 1
        
        if blocks_read % 10000 == 0:
            print(f"  Processed {blocks_read}/{num_blocks} blocks...", file=sys.stderr)
    
    zstd_proc.stdin.close()
    zstd_proc.wait()
    
    if zstd_proc.returncode != 0:
        print(f"Error: zstd compression failed", file=sys.stderr)
        sys.exit(1)
    
    chunk_num_val = $chunk_num
    print(f"  ✅ Chunk {chunk_num_val}: {blocks_read} blocks compressed", file=sys.stderr)
PYTHON_EOF

    local exit_code=$?
    if [ $exit_code -eq 0 ]; then
        chunk_size=$(du -h "$output_file" | cut -f1)
        log "  ✅ Chunk $chunk_num complete: $chunk_size"
    else
        error "Failed to create chunk $chunk_num"
    fi
}

# Split into chunks
log "Starting chunk creation..."
for i in $(seq 0 $((NUM_CHUNKS - 1))); do
    start_block=$((i * BLOCKS_PER_CHUNK))
    if [ $i -eq $((NUM_CHUNKS - 1)) ]; then
        # Last chunk gets remaining blocks
        num_blocks=$((BLOCK_COUNT - start_block))
    else
        num_blocks=$BLOCKS_PER_CHUNK
    fi
    
    split_chunk $i $start_block $num_blocks
done

# Verify chunks
log "Verifying chunks..."
TOTAL_CHUNK_SIZE=0
for i in $(seq 0 $((NUM_CHUNKS - 1))); do
    chunk_file="$CHUNKS_DIR/chunk_${i}.bin.zst"
    if [ -f "$chunk_file" ]; then
        size=$(du -b "$chunk_file" | cut -f1)
        TOTAL_CHUNK_SIZE=$((TOTAL_CHUNK_SIZE + size))
        size_h=$(du -h "$chunk_file" | cut -f1)
        log "  ✅ chunk_${i}.bin.zst: $size_h"
    else
        error "Chunk $i not found: $chunk_file"
    fi
done

TOTAL_CHUNK_SIZE_GB=$(echo "scale=2; $TOTAL_CHUNK_SIZE / 1073741824" | bc)
log "Total compressed size: ${TOTAL_CHUNK_SIZE_GB} GB"

# Save chunk metadata
CHUNK_META="$CHUNKS_DIR/chunks.meta"
cat > "$CHUNK_META" << EOF
# Chunk metadata
total_blocks=$BLOCK_COUNT
num_chunks=$NUM_CHUNKS
blocks_per_chunk=$BLOCKS_PER_CHUNK
compression=zstd
created=$(date -Iseconds)
EOF
log "Saved chunk metadata: $CHUNK_META"

# Now safely delete the incomplete ordered cache
if [ -f "$ORDERED_CACHE" ]; then
    ORDERED_SIZE=$(du -h "$ORDERED_CACHE" | cut -f1)
    warn "Deleting incomplete ordered cache: $ORDERED_CACHE ($ORDERED_SIZE)"
    warn "This is safe - we'll rebuild it per chunk later"
    rm -f "$ORDERED_CACHE"
    log "✅ Deleted incomplete ordered cache"
else
    log "No ordered cache to delete"
fi

log ""
log "✅ Chunking complete!"
log "  Chunks: $CHUNKS_DIR/chunk_*.bin.zst"
log "  Total compressed: ${TOTAL_CHUNK_SIZE_GB} GB"
log "  Original temp file preserved: $TEMP_FILE"
log ""
log "⚠️  IMPORTANT: Temp file ($TEMP_FILE) is preserved"
log "   You can delete it manually after verifying chunks work correctly"
