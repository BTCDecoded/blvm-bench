#!/bin/bash
# Incremental chunk generator - processes one chunk at a time and moves to secondary drive
# This prevents disk space issues and allows monitoring progress

set -euo pipefail

TEMP_FILE="$HOME/.cache/blvm-bench/blvm-bench-blocks-temp.bin"
META_FILE="$TEMP_FILE.meta"
CACHE_DIR="$HOME/.cache/blvm-bench"
CHUNKS_DIR="$CACHE_DIR/chunks"
SECONDARY_DIR="/run/media/acolyte/Extra/blockchain"
NUM_CHUNKS=4

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
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

info() {
    echo -e "${BLUE}[INFO]${NC} $*"
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
log "Blocks per chunk: $BLOCKS_PER_CHUNK"

# Create directories
mkdir -p "$CHUNKS_DIR"
mkdir -p "$SECONDARY_DIR"

# Check if zstd is available
if ! command -v zstd &> /dev/null; then
    error "zstd not found. Install with: sudo pacman -S zstd"
fi

# Check which chunks already exist on secondary drive
existing_chunks=()
for i in $(seq 0 $((NUM_CHUNKS - 1))); do
    if [ -f "$SECONDARY_DIR/chunk_${i}.bin.zst" ]; then
        existing_chunks+=($i)
        size=$(du -h "$SECONDARY_DIR/chunk_${i}.bin.zst" | cut -f1)
        info "Chunk $i already exists on secondary drive: $size"
    fi
done

# Find the next chunk to process
next_chunk=-1
for i in $(seq 0 $((NUM_CHUNKS - 1))); do
    if [[ ! " ${existing_chunks[@]} " =~ " ${i} " ]]; then
        next_chunk=$i
        break
    fi
done

if [ $next_chunk -eq -1 ]; then
    log "✅ All chunks already exist on secondary drive!"
    exit 0
fi

log "Starting from chunk $next_chunk"

# Function to split and compress one chunk
split_chunk() {
    local chunk_num=$1
    local start_block=$2
    local num_blocks=$3
    local output_file="$CHUNKS_DIR/chunk_${chunk_num}.bin.zst"
    
    log "Processing chunk $chunk_num (blocks $start_block to $((start_block + num_blocks - 1)))"
    info "Output: $output_file"
    
    # Check if already exists locally
    if [ -f "$output_file" ]; then
        warn "Chunk $chunk_num already exists locally, skipping generation"
        return 0
    fi
    
    # Read blocks from temp file and write to chunk
    # Format: [len:u32][data...][len:u32][data...]...
    python3 << PYTHON_EOF
import struct
import sys
import subprocess
import os

temp_file = "$TEMP_FILE"
output_file = "$output_file"
start_block = $start_block
num_blocks = $num_blocks
chunk_num = $chunk_num

# Maximum reasonable block size (Bitcoin max block is ~4MB, but we allow up to 10MB for safety)
MAX_BLOCK_SIZE = 10 * 1024 * 1024  # 10 MB

# Open temp file
with open(temp_file, 'rb') as f_in:
    # Skip to start block
    blocks_skipped = 0
    bytes_skipped = 0
    errors_skipped = 0
    consecutive_errors = 0
    MAX_CONSECUTIVE_ERRORS = 100  # Allow up to 100 consecutive corrupted blocks before giving up
    
    while blocks_skipped < start_block:
        current_pos = f_in.tell()
        
        # Read block length
        len_bytes = f_in.read(4)
        if len(len_bytes) < 4:
            print(f"Error: Reached end of file at block {blocks_skipped}", file=sys.stderr)
            sys.exit(1)
        block_len = struct.unpack('<I', len_bytes)[0]
        
        # Validate block length
        if block_len > MAX_BLOCK_SIZE:
            consecutive_errors += 1
            if consecutive_errors <= 3:  # Only log first few
                print(f"⚠️  Block {blocks_skipped} has suspicious size: {block_len:,} bytes ({block_len/(1024**2):.1f} MB) - skipping", file=sys.stderr)
            
            if consecutive_errors > MAX_CONSECUTIVE_ERRORS:
                print(f"Error: Too many consecutive corrupted blocks ({consecutive_errors}) starting at block {blocks_skipped}", file=sys.stderr)
                sys.exit(1)
            
            # For skipping phase, just skip a reasonable amount and continue
            # Assume average block size is ~1MB, skip that much
            skip_amount = min(block_len, 2 * 1024 * 1024)  # Skip up to 2MB
            f_in.seek(skip_amount, 1)
            blocks_skipped += 1
            bytes_skipped += 4 + skip_amount
            errors_skipped += 1
            continue
        
        # Valid block - reset error counter
        consecutive_errors = 0
        
        # Skip block data
        f_in.seek(block_len, 1)
        blocks_skipped += 1
        bytes_skipped += 4 + block_len
        
        if blocks_skipped % 25000 == 0:
            print(f"  Skipped {blocks_skipped:,}/{start_block:,} blocks...", file=sys.stderr)
    
    if errors_skipped > 0:
        print(f"  ⚠️  Encountered {errors_skipped} suspicious blocks while skipping", file=sys.stderr)
    
    print(f"  Reached start block {start_block} (skipped {bytes_skipped / (1024**2):.1f} MB)", file=sys.stderr)
    
    # Read num_blocks blocks and compress
    zstd_proc = subprocess.Popen(
        ['zstd', '-1', '--stdout'],
        stdin=subprocess.PIPE,
        stdout=open(output_file, 'wb'),
        stderr=subprocess.PIPE
    )
    
    blocks_read = 0
    total_bytes = 0
    errors_encountered = 0
    
    while blocks_read < num_blocks:
        # Read block length
        len_bytes = f_in.read(4)
        if len(len_bytes) < 4:
            break
        block_len = struct.unpack('<I', len_bytes)[0]
        
        # Validate block length
        if block_len > MAX_BLOCK_SIZE:
            print(f"⚠️  Block {start_block + blocks_read} has suspicious size: {block_len:,} bytes ({block_len/(1024**2):.1f} MB)", file=sys.stderr)
            print(f"   Skipping this block (may be corrupted)", file=sys.stderr)
            errors_encountered += 1
            # Skip this block and try next
            # Try to find next valid block
            found_valid = False
            for offset in range(1, 1000):
                f_in.seek(-3, 1)
                test_bytes = f_in.read(4)
                if len(test_bytes) == 4:
                    test_len = struct.unpack('<I', test_bytes)[0]
                    if test_len < MAX_BLOCK_SIZE and test_len > 0:
                        block_len = test_len
                        len_bytes = test_bytes
                        found_valid = True
                        break
                f_in.seek(3, 1)
            
            if not found_valid:
                print(f"Error: Could not find valid block after block {start_block + blocks_read}", file=sys.stderr)
                break
        
        # Read block data
        block_data = f_in.read(block_len)
        if len(block_data) < block_len:
            print(f"Warning: Block {start_block + blocks_read} truncated (expected {block_len}, got {len(block_data)})", file=sys.stderr)
            break
        
        # Write to zstd (length + data)
        zstd_proc.stdin.write(len_bytes)
        zstd_proc.stdin.write(block_data)
        blocks_read += 1
        total_bytes += 4 + block_len
        
        if blocks_read % 10000 == 0:
            avg_size = total_bytes / blocks_read if blocks_read > 0 else 0
            print(f"  Processed {blocks_read:,}/{num_blocks:,} blocks ({total_bytes / (1024**2):.1f} MB, avg {avg_size/1024:.1f} KB/block)...", file=sys.stderr)
    
    zstd_proc.stdin.close()
    zstd_stdout, zstd_stderr = zstd_proc.communicate()
    
    if zstd_proc.returncode != 0:
        print(f"Error: zstd compression failed: {zstd_stderr.decode()}", file=sys.stderr)
        sys.exit(1)
    
    chunk_num_val = $chunk_num
    if errors_encountered > 0:
        print(f"  ⚠️  Chunk {chunk_num_val}: {blocks_read:,} blocks compressed ({errors_encountered} errors encountered)", file=sys.stderr)
    else:
        print(f"  ✅ Chunk {chunk_num_val}: {blocks_read:,} blocks compressed", file=sys.stderr)
PYTHON_EOF

    local exit_code=$?
    if [ $exit_code -eq 0 ]; then
        chunk_size=$(du -h "$output_file" | cut -f1)
        log "  ✅ Chunk $chunk_num complete: $chunk_size"
        return 0
    else
        error "Failed to create chunk $chunk_num"
    fi
}

# Function to move chunk to secondary drive
move_chunk_to_secondary() {
    local chunk_num=$1
    local local_file="$CHUNKS_DIR/chunk_${chunk_num}.bin.zst"
    local secondary_file="$SECONDARY_DIR/chunk_${chunk_num}.bin.zst"
    
    if [ ! -f "$local_file" ]; then
        error "Local chunk file not found: $local_file"
    fi
    
    log "Moving chunk $chunk_num to secondary drive..."
    info "  From: $local_file"
    info "  To: $secondary_file"
    
    # Use cp for safe copy with verification, then remove source
    cp -v "$local_file" "$secondary_file" || {
        error "Failed to copy chunk $chunk_num to secondary drive"
    }
    
    # Verify the copy
    local_size=$(stat -f%z "$local_file" 2>/dev/null || stat -c%s "$local_file")
    secondary_size=$(stat -f%z "$secondary_file" 2>/dev/null || stat -c%s "$secondary_file")
    
    if [ "$local_size" -eq "$secondary_size" ]; then
        log "  ✅ Copy verified (${local_size} bytes)"
        rm -f "$local_file"
        log "  ✅ Removed local copy to free space"
    else
        error "Copy verification failed: local=$local_size, secondary=$secondary_size"
    fi
}

# Process chunks starting from next_chunk
for i in $(seq $next_chunk $((NUM_CHUNKS - 1))); do
    start_block=$((i * BLOCKS_PER_CHUNK))
    if [ $i -eq $((NUM_CHUNKS - 1)) ]; then
        # Last chunk gets remaining blocks
        num_blocks=$((BLOCK_COUNT - start_block))
    else
        num_blocks=$BLOCKS_PER_CHUNK
    fi
    
    log ""
    log "═══════════════════════════════════════════════════════════"
    log "Processing Chunk $i of $((NUM_CHUNKS - 1))"
    log "═══════════════════════════════════════════════════════════"
    
    # Generate chunk
    split_chunk $i $start_block $num_blocks
    
    # Move to secondary drive
    move_chunk_to_secondary $i
    
    log "✅ Chunk $i complete and moved to secondary drive"
    log ""
done

# Save/update chunk metadata on secondary drive
CHUNK_META="$SECONDARY_DIR/chunks.meta"
cat > "$CHUNK_META" << EOF
# Chunk metadata
total_blocks=$BLOCK_COUNT
num_chunks=$NUM_CHUNKS
blocks_per_chunk=$BLOCKS_PER_CHUNK
compression=zstd
created=$(date -Iseconds)
EOF
log "Saved chunk metadata: $CHUNK_META"

log ""
log "═══════════════════════════════════════════════════════════"
log "✅ All chunks complete and moved to secondary drive!"
log "═══════════════════════════════════════════════════════════"
log ""
log "Chunks location: $SECONDARY_DIR/chunk_*.bin.zst"
log "Metadata: $CHUNK_META"
log ""
log "⚠️  IMPORTANT: Temp file ($TEMP_FILE) is preserved"
log "   You can delete it manually after verifying chunks work correctly"
