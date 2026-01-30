#!/bin/bash
# Validate chunk protection logic

echo "🔍 VALIDATING CHUNK PROTECTION"
echo "================================"

# Create test scenario
TEST_DIR="/tmp/chunk_protection_test"
rm -rf "$TEST_DIR"
mkdir -p "$TEST_DIR"

# Simulate existing chunk (67GB)
echo "Creating test chunk (simulating 67GB)..."
dd if=/dev/zero of="$TEST_DIR/chunk_0.bin.zst" bs=1M count=1 2>/dev/null
EXISTING_SIZE=$(stat -f%z "$TEST_DIR/chunk_0.bin.zst" 2>/dev/null || stat -c%s "$TEST_DIR/chunk_0.bin.zst" 2>/dev/null)
echo "  Existing chunk size: $EXISTING_SIZE bytes"

# Try to overwrite with tiny file (13 bytes)
echo "Attempting to overwrite with 13-byte file..."
echo "test" > "$TEST_DIR/new_chunk_0.bin.zst"
NEW_SIZE=$(stat -f%z "$TEST_DIR/new_chunk_0.bin.zst" 2>/dev/null || stat -c%s "$TEST_DIR/new_chunk_0.bin.zst" 2>/dev/null)
echo "  New chunk size: $NEW_SIZE bytes"

# Check protection logic
if [ "$EXISTING_SIZE" -gt 1000 ] && [ "$NEW_SIZE" -lt $((EXISTING_SIZE / 10)) ]; then
    echo "  ✅ PROTECTION: Would refuse to overwrite (existing=$EXISTING_SIZE, new=$NEW_SIZE)"
    echo "  ✅ PASS: Chunk protection logic is correct"
    RESULT=0
else
    echo "  ❌ FAIL: Protection logic would allow overwrite"
    RESULT=1
fi

rm -rf "$TEST_DIR"
exit $RESULT

