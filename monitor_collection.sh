#!/bin/bash
# Real-time monitoring for block collection process
# Shows: blocks collected, rate, ETA, disk usage, chunk status

set -euo pipefail

TEMP_FILE="$HOME/.cache/blvm-bench/blvm-bench-blocks-temp.bin"
SECONDARY_DIR="/run/media/acolyte/Extra/blockchain"
CACHE_DIR="$HOME/.cache/blvm-bench"
CHUNKS_DIR="$CACHE_DIR/chunks"
METADATA_FILE="$CACHE_DIR/blvm-bench-blocks-temp.bin.count"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
RED='\033[0;31m'
NC='\033[0m'

# Get block count from metadata file
get_block_count() {
    if [ -f "$METADATA_FILE" ]; then
        # Read 8-byte little-endian u64
        python3 -c "
import struct
with open('$METADATA_FILE', 'rb') as f:
    data = f.read(8)
    if len(data) == 8:
        count = struct.unpack('<Q', data)[0]
        print(count)
    else:
        print(0)
" 2>/dev/null || echo "0"
    else
        echo "0"
    fi
}

# Get temp file size
get_temp_size() {
    if [ -f "$TEMP_FILE" ]; then
        stat -c%s "$TEMP_FILE" 2>/dev/null || echo "0"
    else
        echo "0"
    fi
}

# Count chunks on secondary drive
count_chunks() {
    ls -1 "$SECONDARY_DIR"/chunk_*.bin.zst 2>/dev/null | wc -l
}

# Get total size of chunks on secondary drive
get_chunks_size() {
    du -sb "$SECONDARY_DIR"/chunk_*.bin.zst 2>/dev/null 2>/dev/null | awk '{sum+=$1} END {print sum+0}' || echo "0"
}

# Get disk usage
get_disk_usage() {
    local path="$1"
    df -h "$path" 2>/dev/null | tail -1 | awk '{print $5}' || echo "N/A"
}

# Format bytes
format_bytes() {
    local bytes=$1
    if [ $bytes -gt 1099511627776 ]; then
        echo "$(echo "scale=2; $bytes / 1099511627776" | bc) TB"
    elif [ $bytes -gt 1073741824 ]; then
        echo "$(echo "scale=2; $bytes / 1073741824" | bc) GB"
    elif [ $bytes -gt 1048576 ]; then
        echo "$(echo "scale=2; $bytes / 1048576" | bc) MB"
    else
        echo "$(echo "scale=2; $bytes / 1024" | bc) KB"
    fi
}

# Check if process is running
is_running() {
    pgrep -f "cargo.*start9_test\|cargo.*differential" >/dev/null 2>&1
}

# Get process info
get_process_info() {
    local pid=$(pgrep -f "cargo.*start9_test\|cargo.*differential" | head -1)
    if [ -n "$pid" ]; then
        local cpu=$(ps -p $pid -o %cpu= 2>/dev/null | tr -d ' ' || echo "0")
        local mem=$(ps -p $pid -o %mem= 2>/dev/null | tr -d ' ' || echo "0")
        local runtime=$(ps -p $pid -o etime= 2>/dev/null | tr -d ' ' || echo "N/A")
        echo "$pid|$cpu|$mem|$runtime"
    else
        echo "N/A|N/A|N/A|N/A"
    fi
}

# Main monitoring loop
main() {
    local last_count=0
    local last_time=$(date +%s)
    local last_temp_size=0
    
    echo -e "${CYAN}════════════════════════════════════════════════════════════${NC}"
    echo -e "${CYAN}  BLVM Block Collection Monitor${NC}"
    echo -e "${CYAN}════════════════════════════════════════════════════════════${NC}"
    echo ""
    
    while true; do
        clear
        echo -e "${CYAN}════════════════════════════════════════════════════════════${NC}"
        echo -e "${CYAN}  BLVM Block Collection Monitor - $(date '+%Y-%m-%d %H:%M:%S')${NC}"
        echo -e "${CYAN}════════════════════════════════════════════════════════════${NC}"
        echo ""
        
        # Process status
        if is_running; then
            local proc_info=$(get_process_info)
            local pid=$(echo "$proc_info" | cut -d'|' -f1)
            local cpu=$(echo "$proc_info" | cut -d'|' -f2)
            local mem=$(echo "$proc_info" | cut -d'|' -f3)
            local runtime=$(echo "$proc_info" | cut -d'|' -f4)
            echo -e "${GREEN}●${NC} Process Status: ${GREEN}RUNNING${NC}"
            echo "   PID: $pid | CPU: ${cpu}% | Memory: ${mem}% | Runtime: $runtime"
        else
            echo -e "${RED}●${NC} Process Status: ${RED}NOT RUNNING${NC}"
        fi
        echo ""
        
        # Block count
        local current_count=$(get_block_count)
        local current_time=$(date +%s)
        local temp_size=$(get_temp_size)
        
        echo -e "${BLUE}📊 Collection Progress:${NC}"
        echo "   Blocks collected: ${CYAN}$(printf "%'d" $current_count)${NC} / 500,000"
        if [ $current_count -gt 0 ]; then
            local progress=$(echo "scale=2; $current_count * 100 / 500000" | bc)
            echo "   Progress: ${CYAN}${progress}%${NC}"
        fi
        echo ""
        
        # Rate calculation
        if [ $last_count -gt 0 ] && [ $current_count -gt $last_count ]; then
            local time_diff=$((current_time - last_time))
            local count_diff=$((current_count - last_count))
            if [ $time_diff -gt 0 ]; then
                local rate=$(echo "scale=2; $count_diff / $time_diff" | bc)
                echo -e "${YELLOW}⚡ Collection Rate: ${CYAN}${rate}${NC} blocks/sec${YELLOW}${NC}"
                
                # ETA calculation
                if [ $(echo "$rate > 0" | bc) -eq 1 ]; then
                    local remaining=$((500000 - current_count))
                    local eta_seconds=$(echo "scale=0; $remaining / $rate" | bc)
                    local eta_hours=$(echo "scale=1; $eta_seconds / 3600" | bc)
                    echo "   ETA: ${CYAN}${eta_hours}${NC} hours (${CYAN}$(date -d "+${eta_seconds} seconds" '+%Y-%m-%d %H:%M:%S')${NC})"
                fi
            fi
        else
            echo -e "${YELLOW}⚡ Collection Rate: ${CYAN}Calculating...${NC}"
        fi
        echo ""
        
        # Disk usage
        echo -e "${BLUE}💾 Disk Usage:${NC}"
        local temp_size_fmt=$(format_bytes $temp_size)
        echo "   Temp file: ${CYAN}${temp_size_fmt}${NC}"
        
        local chunks_count=$(count_chunks)
        local chunks_size=$(get_chunks_size)
        local chunks_size_fmt=$(format_bytes $chunks_size)
        echo "   Chunks on secondary: ${CYAN}${chunks_count}${NC} chunks (${CYAN}${chunks_size_fmt}${NC})"
        
        local cache_usage=$(get_disk_usage "$CACHE_DIR")
        echo "   Cache directory: ${CYAN}${cache_usage}${NC} used"
        
        local secondary_usage=$(get_disk_usage "$SECONDARY_DIR")
        echo "   Secondary drive: ${CYAN}${secondary_usage}${NC} used"
        echo ""
        
        # Chunk status
        if [ $chunks_count -gt 0 ]; then
            echo -e "${BLUE}📦 Chunk Status:${NC}"
            ls -lh "$SECONDARY_DIR"/chunk_*.bin.zst 2>/dev/null | awk '{print "   " $9 " (" $5 ")"}' | head -5
            if [ $chunks_count -gt 5 ]; then
                echo "   ... and $((chunks_count - 5)) more"
            fi
            echo ""
        fi
        
        # Current chunk progress
        if [ $current_count -gt 0 ]; then
            local current_chunk=$((current_count / 125000))
            local blocks_in_chunk=$((current_count % 125000))
            local chunk_progress=$(echo "scale=2; $blocks_in_chunk * 100 / 125000" | bc)
            echo -e "${BLUE}📦 Current Chunk: ${CYAN}${current_chunk}${NC} (${CYAN}${blocks_in_chunk}${NC} / 125,000 blocks - ${CYAN}${chunk_progress}%${NC})"
            echo ""
        fi
        
        # Recent activity
        echo -e "${BLUE}📝 Recent Activity:${NC}"
        if [ -f "$TEMP_FILE" ]; then
            local temp_age=$(($(date +%s) - $(stat -c %Y "$TEMP_FILE" 2>/dev/null || echo 0)))
            if [ $temp_age -lt 60 ]; then
                echo -e "   Temp file: ${GREEN}Updated ${temp_age}s ago${NC}"
            elif [ $temp_age -lt 300 ]; then
                echo -e "   Temp file: ${YELLOW}Updated ${temp_age}s ago${NC}"
            else
                echo -e "   Temp file: ${RED}Not updated for ${temp_age}s${NC}"
            fi
        else
            echo -e "   Temp file: ${YELLOW}Not created yet${NC}"
        fi
        
        local size_diff=$((temp_size - last_temp_size))
        if [ $size_diff -gt 0 ]; then
            echo -e "   Growth rate: ${GREEN}+$(format_bytes $size_diff)${NC} since last check"
        fi
        echo ""
        
        echo -e "${CYAN}════════════════════════════════════════════════════════════${NC}"
        echo "Press Ctrl+C to exit"
        echo ""
        
        # Update for next iteration
        last_count=$current_count
        last_time=$current_time
        last_temp_size=$temp_size
        
        sleep 5
    done
}

# Handle Ctrl+C gracefully
trap 'echo -e "\n\n${YELLOW}Monitoring stopped.${NC}"; exit 0' INT TERM

main
