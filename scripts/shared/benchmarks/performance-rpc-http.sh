#!/bin/bash
# RPC Performance Benchmark via HTTP (Fair Comparison)
# Source common functions
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../../shared/common.sh"
# Measures RPC method execution times via HTTP/JSON-RPC to match Commons methodology
# This includes network overhead, HTTP parsing, and JSON-RPC overhead like Commons

set -e

OUTPUT_DIR="${1:-$(dirname "$0")/../results}"
mkdir -p "$OUTPUT_DIR"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CORE_CLI="$PROJECT_ROOT/core/build/bin/bitcoin-cli"
BITCOIND="$PROJECT_ROOT/core/build/bin/bitcoind"
OUTPUT_FILE="$OUTPUT_DIR/performance-rpc-http-$(date +%Y%m%d-%H%M%S).json"

echo "=== RPC Performance Benchmark via HTTP (Fair Comparison) ==="
echo ""
echo "⚠️  This measures Core via HTTP/JSON-RPC to match Commons methodology"
echo ""

# Check if bitcoind is running
RPC_PORT=18443
RPC_HOST="127.0.0.1"
RPC_USER="test"
RPC_PASS="test"

if ! curl -s --connect-timeout 1 -X POST \
    -u "$RPC_USER:$RPC_PASS" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"ping","params":[],"id":1}' \
    "http://$RPC_HOST:$RPC_PORT" > /dev/null 2>&1; then
    echo "⚠️  bitcoind not running. Starting in regtest mode..."
    pkill -f "bitcoind.*regtest.*18443" 2>/dev/null || true
    sleep 2
    $BITCOIND -regtest -daemon -server -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT -rpcallowip=127.0.0.1 -rpcbind=127.0.0.1 > /dev/null 2>&1 || true
    sleep 5
    
    # Wait for bitcoind to be ready
    for i in {1..30}; do
        if curl -s --connect-timeout 1 -X POST \
            -u "$RPC_USER:$RPC_PASS" \
            -H "Content-Type: application/json" \
            -d '{"jsonrpc":"2.0","method":"ping","params":[],"id":1}' \
            "http://$RPC_HOST:$RPC_PORT" > /dev/null 2>&1; then
            echo "✅ bitcoind is ready"
            break
        fi
        if [ $i -eq 30 ]; then
            echo "❌ Failed to start bitcoind or connect"
            exit 1
        fi
        sleep 1
    done
fi

RPC_METHODS=(
    # Basic blockchain info
    "getblockchaininfo"
    "getblockcount"
    "getbestblockhash"
    "getblockhash"
    "getblock"
    "getblockheader"
    # Network info
    "getnetworkinfo"
    "getconnectioncount"
    "getnettotals"
    "getpeerinfo"
    # Mempool
    "getmempoolinfo"
    "getrawmempool"
    "getmempoolentry"
    # Chain state
    "gettxoutsetinfo"
    "getchaintips"
    "getchaintxstats"
    # Mining
    "getdifficulty"
    "getmininginfo"
    "getnetworkhashps"
    # Utility
    "ping"
    "uptime"
    "getmemoryinfo"
    "getrpcinfo"
    # Wallet (if available)
    "getwalletinfo"
    "listwallets"
)

cat > "$OUTPUT_FILE" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "measurement_method": "HTTP/JSON-RPC (fair comparison with Commons)",
  "rpc_server": "http://$RPC_HOST:$RPC_PORT",
  "rpc_performance": {
EOF

FIRST=true
for method in "${RPC_METHODS[@]}"; do
    echo "  Testing: $method (via HTTP)..."
    
    # Determine parameters based on method
    PARAMS="[]"
    case "$method" in
        "getblockhash")
            # Get block hash for block 0 (genesis)
            PARAMS="[0]"
            ;;
        "getblock")
            # Get block 0 (genesis) - verbosity 1 (JSON)
            GENESIS_HASH=$(curl -s -X POST -u "$RPC_USER:$RPC_PASS" -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"getblockhash","params":[0],"id":1}' "http://$RPC_HOST:$RPC_PORT" | jq -r '.result' 2>/dev/null || echo "0f9188f13cb7b2c71f2a335e3a4fc328bf5beb436012afca590b1a11466e2206")
            PARAMS="[\"$GENESIS_HASH\", 1]"
            ;;
        "getblockheader")
            # Get block header for block 0
            GENESIS_HASH=$(curl -s -X POST -u "$RPC_USER:$RPC_PASS" -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"getblockhash","params":[0],"id":1}' "http://$RPC_HOST:$RPC_PORT" | jq -r '.result' 2>/dev/null || echo "0f9188f13cb7b2c71f2a335e3a4fc328bf5beb436012afca590b1a11466e2206")
            PARAMS="[\"$GENESIS_HASH\"]"
            ;;
        "getmempoolentry")
            # Skip if mempool is empty, otherwise get first txid
            TXID=$(curl -s -X POST -u "$RPC_USER:$RPC_PASS" -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"getrawmempool","params":[],"id":1}' "http://$RPC_HOST:$RPC_PORT" | jq -r '.result[0]' 2>/dev/null || echo "")
            if [ -z "$TXID" ] || [ "$TXID" = "null" ]; then
                echo "    ⏭️  Skipping (mempool empty)"
                continue
            fi
            PARAMS="[\"$TXID\"]"
            ;;
        "getchaintxstats")
            # Get chain tx stats with no parameters (uses default)
            PARAMS="[]"
            ;;
    esac
    
    # Run multiple times and take average
    TIMES=()
    SKIP_METHOD=false
    for i in {1..20}; do
        START=$(date +%s%N)
        RESPONSE=$(curl -s -X POST \
            -u "$RPC_USER:$RPC_PASS" \
            -H "Content-Type: application/json" \
            -d "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":$PARAMS,\"id\":1}" \
            "http://$RPC_HOST:$RPC_PORT" 2>&1)
        END=$(date +%s%N)
        DURATION=$(( (END - START) / 1000000 ))
        
        # Check if method failed (not available or error)
        if echo "$RESPONSE" | jq -e '.error' > /dev/null 2>&1; then
            ERROR_CODE=$(echo "$RESPONSE" | jq -r '.error.code' 2>/dev/null || echo "")
            if [ "$ERROR_CODE" = "-32601" ] || [ "$ERROR_CODE" = "-1" ]; then
                echo "    ⏭️  Skipping (method not available or requires different parameters)"
                SKIP_METHOD=true
                break
            fi
        fi
        
        TIMES+=($DURATION)
    done
    
    if [ "$SKIP_METHOD" = true ]; then
        continue
    fi
    
    if [ "$FIRST" = false ]; then
        echo "," >> "$OUTPUT_FILE"
    fi
    FIRST=false
    
    # Sort times for percentile calculation
    IFS=$'\n' SORTED_TIMES=($(sort -n <<<"${TIMES[*]}"))
    unset IFS
    
    # Calculate average
    TOTAL=0
    for t in "${TIMES[@]}"; do
        TOTAL=$((TOTAL + t))
    done
    AVG=$(awk "BEGIN {printf \"%.2f\", $TOTAL / ${#TIMES[@]}}")
    
    # Calculate min and max
    MIN=${SORTED_TIMES[0]}
    MAX=${SORTED_TIMES[-1]}
    
    # Calculate percentiles (50th = median, 90th, 95th)
    PERCENTILE_50_INDEX=$(( ${#SORTED_TIMES[@]} * 50 / 100 ))
    PERCENTILE_90_INDEX=$(( ${#SORTED_TIMES[@]} * 90 / 100 ))
    PERCENTILE_95_INDEX=$(( ${#SORTED_TIMES[@]} * 95 / 100 ))
    
    PERCENTILE_50=${SORTED_TIMES[$PERCENTILE_50_INDEX]}
    PERCENTILE_90=${SORTED_TIMES[$PERCENTILE_90_INDEX]}
    PERCENTILE_95=${SORTED_TIMES[$PERCENTILE_95_INDEX]}
    
    cat >> "$OUTPUT_FILE" << EOF
    "$method": {
      "average_ms": $AVG,
      "min_ms": $MIN,
      "max_ms": $MAX,
      "median_ms": $PERCENTILE_50,
      "p90_ms": $PERCENTILE_90,
      "p95_ms": $PERCENTILE_95,
      "samples": ${#TIMES[@]}
    }
EOF
done

cat >> "$OUTPUT_FILE" << EOF
  },
  "comparison_note": "This benchmark measures Core via HTTP/JSON-RPC to match Commons methodology. Includes network overhead, HTTP parsing, and JSON-RPC overhead like Commons measurements."
}
EOF

echo ""
echo "Results saved to: $OUTPUT_FILE"
echo ""
echo "✅ Core RPC performance measured via HTTP (fair comparison)"
cat "$OUTPUT_FILE" | jq '.' 2>/dev/null || cat "$OUTPUT_FILE"

