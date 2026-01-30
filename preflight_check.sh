#!/bin/bash
# Pre-flight checklist for differential analysis

echo "🔍 Pre-Flight Checklist for Differential Analysis"
echo "=================================================="
echo ""

ERRORS=0
WARNINGS=0

# 1. Check chunks exist
echo "1️⃣  Checking chunks..."
CHUNK_DIR="/run/media/acolyte/Extra/blockchain"
if [ ! -d "$CHUNK_DIR" ]; then
    echo "   ❌ Chunk directory not found: $CHUNK_DIR"
    ERRORS=$((ERRORS + 1))
else
    CHUNK_COUNT=$(ls -1 "$CHUNK_DIR"/chunk_*.bin.zst 2>/dev/null | wc -l)
    if [ $CHUNK_COUNT -eq 0 ]; then
        echo "   ❌ No chunks found in $CHUNK_DIR"
        ERRORS=$((ERRORS + 1))
    else
        echo "   ✅ Found $CHUNK_COUNT chunks"
        
        # Check if chunks are read-only
        READ_ONLY_COUNT=$(stat -c "%a" "$CHUNK_DIR"/chunk_*.bin.zst 2>/dev/null | grep -c "^444$" || echo "0")
        if [ $READ_ONLY_COUNT -lt $CHUNK_COUNT ]; then
            echo "   ⚠️  Chunks are not read-only (run ./protect_chunks.sh)"
            WARNINGS=$((WARNINGS + 1))
        else
            echo "   ✅ Chunks are read-only (protected)"
        fi
    fi
fi
echo ""

# 2. Check metadata file
echo "2️⃣  Checking metadata file..."
if [ ! -f "$CHUNK_DIR/chunks.meta" ]; then
    echo "   ⚠️  Metadata file not found (run ./create_chunk_metadata.sh)"
    WARNINGS=$((WARNINGS + 1))
else
    echo "   ✅ Metadata file exists"
    cat "$CHUNK_DIR/chunks.meta" | grep -v "^#" | head -5
fi
echo ""

# 3. Check code compilation
echo "3️⃣  Checking code compilation..."
if cargo check --release --features differential > /dev/null 2>&1; then
    echo "   ✅ Code compiles successfully"
else
    echo "   ❌ Code compilation failed"
    ERRORS=$((ERRORS + 1))
fi
echo ""

# 4. Check Bitcoin Core RPC (optional but recommended)
echo "4️⃣  Checking Bitcoin Core RPC..."
if [ -z "$BITCOIN_RPC_HOST" ]; then
    echo "   ⚠️  BITCOIN_RPC_HOST not set (will try localhost)"
    WARNINGS=$((WARNINGS + 1))
else
    echo "   ✅ BITCOIN_RPC_HOST=$BITCOIN_RPC_HOST"
fi

# Try to connect (non-blocking check)
if command -v bitcoin-cli > /dev/null 2>&1; then
    if timeout 2 bitcoin-cli getblockcount > /dev/null 2>&1; then
        BLOCK_COUNT=$(bitcoin-cli getblockcount 2>/dev/null)
        echo "   ✅ Bitcoin Core RPC accessible (block height: $BLOCK_COUNT)"
    else
        echo "   ⚠️  Bitcoin Core RPC not accessible (test will use direct file reading)"
        WARNINGS=$((WARNINGS + 1))
    fi
else
    echo "   ⚠️  bitcoin-cli not found (will use direct file reading)"
    WARNINGS=$((WARNINGS + 1))
fi
echo ""

# 5. Check disk space
echo "5️⃣  Checking disk space..."
LOG_DIR="/tmp"
AVAILABLE=$(df -BG "$LOG_DIR" | tail -1 | awk '{print $4}' | sed 's/G//')
if [ "$AVAILABLE" -lt 5 ]; then
    echo "   ⚠️  Low disk space in $LOG_DIR (${AVAILABLE}GB available, need ~5GB for logs)"
    WARNINGS=$((WARNINGS + 1))
else
    echo "   ✅ Sufficient disk space (${AVAILABLE}GB available)"
fi
echo ""

# 6. Check monitoring tools
echo "6️⃣  Checking monitoring tools..."
if [ -f "./monitor_differential.sh" ]; then
    echo "   ✅ Monitoring script exists"
else
    echo "   ⚠️  Monitoring script not found"
    WARNINGS=$((WARNINGS + 1))
fi
echo ""

# 7. Check system resources
echo "7️⃣  Checking system resources..."
CPU_CORES=$(nproc)
MEM_AVAIL=$(free -g | grep "^Mem:" | awk '{print $7}')
echo "   CPU cores: $CPU_CORES"
echo "   Available memory: ${MEM_AVAIL}GB"

if [ "$MEM_AVAIL" -lt 8 ]; then
    echo "   ⚠️  Low available memory (recommend at least 8GB free)"
    WARNINGS=$((WARNINGS + 1))
else
    echo "   ✅ Sufficient memory available"
fi
echo ""

# Summary
echo "=================================================="
echo "📊 Summary:"
echo "   Errors: $ERRORS"
echo "   Warnings: $WARNINGS"
echo ""

if [ $ERRORS -eq 0 ] && [ $WARNINGS -eq 0 ]; then
    echo "✅ All checks passed! Ready to proceed."
    exit 0
elif [ $ERRORS -eq 0 ]; then
    echo "⚠️  Some warnings, but can proceed."
    echo "   Review warnings above before starting."
    exit 0
else
    echo "❌ Errors found. Please fix before proceeding."
    exit 1
fi
