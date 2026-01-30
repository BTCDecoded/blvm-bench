#!/bin/bash
# Create chunk metadata file for automatic detection

CHUNK_DIR="/run/media/acolyte/Extra/blockchain"

echo "📝 Creating chunk metadata file..."

cat > "$CHUNK_DIR/chunks.meta" << METADATA
# Chunk metadata for differential testing
# Generated: $(date)
total_blocks=875000
num_chunks=7
blocks_per_chunk=125000
compression=zstd
METADATA

chmod 444 "$CHUNK_DIR/chunks.meta"
echo "✅ Metadata file created: $CHUNK_DIR/chunks.meta"
cat "$CHUNK_DIR/chunks.meta"
