#!/bin/bash
# Bitcoin Core Base58/Bech32 Benchmark (Portable)

set -e

# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../shared/common.sh"

# Bitcoin Core Base58/Bech32 Benchmark
# Measures Base58 and Bech32 encoding/decoding performance using bench_bitcoin



BENCH_BITCOIN="$CORE_DIR$CORE_PATH/$CORE_PATH/build/bin/bench_bitcoin"
OUTPUT_FILE="$OUTPUT_DIR/core-base58-bech32-bench-$(date +%Y%m%d-%H%M%S).json"

echo "=== Bitcoin Core Base58/Bech32 Benchmark ==="
echo ""

if [ ! -f "$BENCH_BITCOIN" ]; then
    echo "❌ bench_bitcoin not found at $BENCH_BITCOIN"
    echo "   Building bench_bitcoin..."
    cd "$CORE_DIR"
    make -j$(nproc) bench_bitcoin 2>&1 | tail -5
    if [ ! -f "$BENCH_BITCOIN" ]; then
        echo "❌ Failed to build bench_bitcoin"
        exit 1
    fi
fi

echo "Running bench_bitcoin for Base58/Bech32 operations (this may take a few minutes)..."
echo "This benchmarks Base58 and Bech32 encoding/decoding performance."

# Run bench_bitcoin and capture output
BENCH_OUTPUT=$("$BENCH_BITCOIN" -filter="Base58|Bech32" 2>&1 || echo "")

# Parse bench_bitcoin output
parse_bench_bitcoin() {
    local line="$1"
    if [ -z "$line" ]; then
        echo "0|0"
        return
    fi
    time_ns=$(echo "$line" | awk -F'|' '{gsub(/[^0-9.]/,"",$2); print $2}' 2>/dev/null || echo "0")
    ops_per_sec=$(echo "$line" | awk -F'|' '{gsub(/[^0-9.]/,"",$3); print $3}' 2>/dev/null || echo "0")
    echo "${time_ns}|${ops_per_sec}"
}

BASE58_ENCODE_LINE=$(echo "$BENCH_OUTPUT" | grep -iE "Base58Encode" | head -1 || echo "")
BASE58_CHECK_ENCODE_LINE=$(echo "$BENCH_OUTPUT" | grep -iE "Base58CheckEncode" | head -1 || echo "")
BASE58_DECODE_LINE=$(echo "$BENCH_OUTPUT" | grep -iE "Base58Decode" | head -1 || echo "")
BECH32_ENCODE_LINE=$(echo "$BENCH_OUTPUT" | grep -iE "Bech32Encode" | head -1 || echo "")
BECH32_DECODE_LINE=$(echo "$BENCH_OUTPUT" | grep -iE "Bech32Decode" | head -1 || echo "")

BENCHMARKS="[]"

for bench_name in "Base58Encode" "Base58CheckEncode" "Base58Decode" "Bech32Encode" "Bech32Decode"; do
    case "$bench_name" in
        "Base58Encode") LINE="$BASE58_ENCODE_LINE" ;;
        "Base58CheckEncode") LINE="$BASE58_CHECK_ENCODE_LINE" ;;
        "Base58Decode") LINE="$BASE58_DECODE_LINE" ;;
        "Bech32Encode") LINE="$BECH32_ENCODE_LINE" ;;
        "Bech32Decode") LINE="$BECH32_DECODE_LINE" ;;
    esac
    
    if [ -n "$LINE" ]; then
        DATA=$(parse_bench_bitcoin "$LINE")
        TIME_NS=$(echo "$DATA" | cut -d'|' -f1)
        OPS=$(echo "$DATA" | cut -d'|' -f2)
        
        if [ "$TIME_NS" != "0" ]; then
            TIME_MS=$(awk "BEGIN {printf \"%.6f\", $TIME_NS / 1000000}")
            BENCHMARKS=$(echo "$BENCHMARKS" | jq --arg name "$bench_name" --arg time "$TIME_MS" --arg timens "$TIME_NS" '. += [{"name": $name, "time_ms": ($time | tonumber), "time_ns": ($timens | tonumber), "ops_per_sec": ($OPS | tonumber)}]' 2>/dev/null || echo "$BENCHMARKS")
        fi
    fi
done

cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "measurement_method": "bench_bitcoin (Base58/Bech32 encoding/decoding)",
  "benchmarks": $BENCHMARKS
}
EOF
echo "✅ Results saved to: $OUTPUT_FILE"


