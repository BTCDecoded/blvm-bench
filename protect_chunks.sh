#!/bin/bash
# PROTECT CHUNK FILES - READ-ONLY MODE
# DO NOT DELETE OR MODIFY THESE FILES

CHUNK_DIR="/run/media/acolyte/Extra/blockchain"

echo "🛡️  PROTECTING CHUNK FILES"
echo "=========================="

if [ ! -d "$CHUNK_DIR" ]; then
    echo "❌ Chunk directory not found: $CHUNK_DIR"
    exit 1
fi

# Make all chunks read-only
echo "Making chunks read-only..."
chmod 444 "$CHUNK_DIR"/chunk_*.bin.zst 2>/dev/null

# Create checksums if they don't exist
if [ ! -f "$CHUNK_DIR/chunks.sha256" ]; then
    echo "Creating checksums..."
    cd "$CHUNK_DIR"
    for chunk in chunk_*.bin.zst; do
        if [ -f "$chunk" ]; then
            echo "  Computing checksum for $chunk..."
            sha256sum "$chunk" >> chunks.sha256
        fi
    done
    chmod 444 chunks.sha256
    echo "✅ Checksums created: chunks.sha256"
fi

# Verify chunks exist
CHUNK_COUNT=$(ls -1 "$CHUNK_DIR"/chunk_*.bin.zst 2>/dev/null | wc -l)
echo "✅ Found $CHUNK_COUNT chunks (read-only)"
echo ""
echo "📊 Chunk Status:"
ls -lh "$CHUNK_DIR"/chunk_*.bin.zst 2>/dev/null | awk '{print "  " $9 " - " $5 " (" $6 " " $7 " " $8 ")"}'
echo ""
echo "✅ Chunks protected!"
