#!/bin/bash
# Monitor cache build process and automatically restart if stuck
# Usage: ./monitor_cache_build.sh [check-interval-seconds] [stuck-threshold-seconds]

set -euo pipefail

# Configuration
CHECK_INTERVAL=${1:-300}  # Check every 5 minutes by default
STUCK_THRESHOLD=${2:-14400}  # Consider stuck if no activity for 4 hours (sparse regions are slow!)
LOG_FILE="/tmp/cache-build-monitor.log"
# Use latest cache build log (timestamped)
get_latest_cache_log() {
    ls -t /tmp/cache-build-test-*.log 2>/dev/null | head -1 || echo "/tmp/cache-build-test.log"
}
CACHE_BUILD_LOG=$(get_latest_cache_log)
TEMP_FILE="$HOME/.cache/blvm-bench/blvm-bench-blocks-temp.bin"
CACHE_DIR="$HOME/.cache/blvm-bench"
PROCESS_PATTERN="cargo test.*start9"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" | tee -a "$LOG_FILE"
}

log_info() {
    log "${GREEN}INFO${NC}: $*"
}

log_warn() {
    log "${YELLOW}WARN${NC}: $*"
}

log_error() {
    log "${RED}ERROR${NC}: $*"
}

# Check if process is running
is_process_running() {
    pgrep -f "$PROCESS_PATTERN" >/dev/null 2>&1
}

# Get process PID
get_process_pid() {
    pgrep -f "$PROCESS_PATTERN" | head -1
}

# Check if process is stuck based on criteria
is_process_stuck() {
    local pid=$1
    local stuck=false
    local reasons=()
    
    # Criteria 1: Log file hasn't been modified recently
    # Update to use latest log file
    local latest_log=$(get_latest_cache_log)
    if [ -f "$latest_log" ]; then
        local log_age=$(($(date +%s) - $(stat -c %Y "$latest_log" 2>/dev/null || echo 0)))
        local log_size=$(stat -c %s "$latest_log" 2>/dev/null || echo 0)
        
        # Check if log has recent progress updates
        local last_progress=$(grep "📊 Progress:" "$latest_log" 2>/dev/null | tail -1 || echo "")
        local last_progress_time=0
        if [ -n "$last_progress" ]; then
            # Extract timestamp from log line if available, or use file mtime
            last_progress_time=$(stat -c %Y "$latest_log" 2>/dev/null || echo 0)
        fi
        
        if [ $log_age -gt $STUCK_THRESHOLD ]; then
            stuck=true
            local last_block_count="unknown"
            if [ -n "$last_progress" ]; then
                last_block_count=$(echo "$last_progress" | grep -oE '[0-9]+/[0-9]+' | head -1 || echo "unknown")
            fi
            reasons+=("Log file ($(basename $latest_log)) not modified for ${log_age}s (>${STUCK_THRESHOLD}s)")
            reasons+=("Last progress: $last_block_count blocks")
            reasons+=("Log file size: $(($log_size / 1024))KB")
        fi
    else
        stuck=true
        reasons+=("Log file does not exist")
    fi
    
    # Criteria 2: Temp file hasn't been modified recently (if it exists)
    if [ -f "$TEMP_FILE" ]; then
        local temp_age=$(($(date +%s) - $(stat -c %Y "$TEMP_FILE" 2>/dev/null || echo 0)))
        if [ $temp_age -gt $STUCK_THRESHOLD ]; then
            stuck=true
            reasons+=("Temp file not modified for ${temp_age}s (>${STUCK_THRESHOLD}s)")
        fi
    fi
    
    # Criteria 3: Process is in uninterruptible sleep (D state) for too long
    if [ -n "$pid" ] && [ -d "/proc/$pid" ]; then
        local state=$(cat /proc/$pid/stat 2>/dev/null | awk '{print $3}')
        if [ "$state" = "D" ]; then
            # Check how long it's been in D state
            local etime=$(ps -p $pid -o etime= 2>/dev/null | awk '{print $1}')
            stuck=true
            reasons+=("Process in uninterruptible sleep (D state) for $etime")
        fi
        
        # Criteria 4: Process has very low I/O activity
        # BUT: Check if temp file is still growing (process is making progress even if I/O is low)
        local io_stats=$(cat /proc/$pid/io 2>/dev/null || echo "")
        if [ -n "$io_stats" ]; then
            local read_bytes=$(echo "$io_stats" | grep "^read_bytes:" | awk '{print $2}')
            local write_bytes=$(echo "$io_stats" | grep "^write_bytes:" | awk '{print $2}')
            local runtime=$(ps -p $pid -o etime= 2>/dev/null | awk -F: '{if (NF==3) print $1*3600+$2*60+$3; else if (NF==2) print $1*60+$2; else print $1}')
            
            # Check if temp file is growing (indicates progress even with low I/O)
            local temp_size_before=0
            local temp_size_after=0
            if [ -f "$TEMP_FILE" ]; then
                temp_size_before=$(stat -c %s "$TEMP_FILE" 2>/dev/null || echo 0)
                sleep 10  # Wait 10 seconds
                temp_size_after=$(stat -c %s "$TEMP_FILE" 2>/dev/null || echo 0)
            fi
            
            # Only consider stuck if:
            # 1. Running for > 1 hour
            # 2. Very low I/O (< 1MB)
            # 3. Temp file NOT growing (no progress)
            # 4. Log file not updating
            if [ -n "$runtime" ] && [ "$runtime" -gt 3600 ] && [ "${read_bytes:-0}" -lt 1000000 ]; then
                if [ $temp_size_after -le $temp_size_before ]; then
                    # Temp file not growing - might be stuck
                    stuck=true
                    reasons+=("Low I/O activity: read_bytes=$read_bytes after ${runtime}s runtime")
                    reasons+=("Temp file not growing: ${temp_size_before} -> ${temp_size_after} bytes")
                else
                    # Temp file is growing - process is making progress, just slow
                    log_info "Low I/O but temp file growing (${temp_size_before} -> ${temp_size_after} bytes) - process is slow but not stuck"
                fi
            fi
        fi
    
    # Criteria 5: Check if process is in a known slow stage (resume/counting)
    # Don't consider it stuck if it's actively counting blocks (this can take time)
    local latest_log=$(get_latest_cache_log)
    if [ -f "$latest_log" ]; then
        local last_lines=$(tail -10 "$latest_log" 2>/dev/null || echo "")
        if echo "$last_lines" | grep -q "counting blocks to resume\|Found existing temp file"; then
            # Process is counting blocks - this is normal and can take time
            # Only consider stuck if counting takes > 30 minutes
            local counting_start=$(grep -n "Found existing temp file\|counting blocks to resume" "$latest_log" 2>/dev/null | tail -1 | cut -d: -f1)
            if [ -n "$counting_start" ]; then
                local log_age=$(($(date +%s) - $(stat -c %Y "$latest_log" 2>/dev/null || echo 0)))
                if [ $log_age -lt 1800 ]; then  # Less than 30 minutes
                    # Still counting, not stuck yet
                    log_info "Process is counting blocks (normal, can take time) - not considering stuck"
                    return 1
                fi
            fi
        fi
        
        # Check if process is in sparse region (low rate but still making progress)
        local last_progress=$(grep "📊 Progress:" "$latest_log" 2>/dev/null | tail -1 || echo "")
        if [ -n "$last_progress" ]; then
            local rate=$(echo "$last_progress" | grep -oE 'Rate: [0-9.]+' | grep -oE '[0-9.]+' | head -1 || echo "0")
            if [ -n "$rate" ] && [ $(echo "$rate < 50" | bc 2>/dev/null || echo 0) -eq 1 ]; then
                # Rate is low (< 50 blocks/sec) - might be in sparse region
                # Check if progress is still being made (log updated recently)
                local log_age=$(($(date +%s) - $(stat -c %Y "$latest_log" 2>/dev/null || echo 0)))
                if [ $log_age -lt 600 ]; then  # Log updated in last 10 minutes
                    log_info "Process in sparse region (rate: ${rate} blocks/sec) but making progress - not considering stuck"
                    # Don't mark as stuck if making progress, even if slow
                    return 1
                fi
            fi
        fi
    fi
    fi
    
    if [ "$stuck" = true ]; then
        echo "${reasons[*]}"
        return 0
    else
        return 1
    fi
}

# Kill stuck process
kill_stuck_process() {
    local pid=$1
    log_warn "Killing stuck process (PID: $pid)"
    
    # Try graceful kill first
    kill -TERM "$pid" 2>/dev/null || true
    sleep 5
    
    # Force kill if still running
    if kill -0 "$pid" 2>/dev/null; then
        log_warn "Process still running, force killing..."
        kill -KILL "$pid" 2>/dev/null || true
        sleep 2
    fi
    
    # Verify it's dead
    if kill -0 "$pid" 2>/dev/null; then
        log_error "Failed to kill process $pid"
        return 1
    else
        log_info "Successfully killed process $pid"
        return 0
    fi
}

# Restart cache build process
restart_cache_build() {
    log_info "Restarting cache build process..."
    
    cd /home/acolyte/src/BitcoinCommons/blvm-bench || {
        log_error "Failed to change directory"
        return 1
    }
    
    # Remove bad cache file if it exists (smaller than expected)
    if [ -f "$CACHE_DIR/start9_ordered_blocks.bin" ]; then
        local cache_size=$(stat -c %s "$CACHE_DIR/start9_ordered_blocks.bin" 2>/dev/null || echo 0)
        if [ $cache_size -lt 1000000 ]; then  # Less than 1MB is definitely bad
            log_warn "Removing bad cache file (size: $cache_size bytes)"
            rm -f "$CACHE_DIR/start9_ordered_blocks.bin"
        fi
    fi
    
    # Start the cache build in background
    log_info "Starting cache build with HISTORICAL_BLOCK_START=0 HISTORICAL_BLOCK_END=1000000"
    nohup bash -c "HISTORICAL_BLOCK_START=0 HISTORICAL_BLOCK_END=1000000 timeout 86400 cargo test --release --features differential --test start9_test -- --nocapture --test-threads=1 2>&1 | tee /tmp/cache-build-test-\$(date +%Y%m%d-%H%M%S).log" >/dev/null 2>&1 &
    
    sleep 2
    
    # Verify it started
    if is_process_running; then
        local new_pid=$(get_process_pid)
        log_info "Cache build restarted successfully (PID: $new_pid)"
        return 0
    else
        log_error "Failed to restart cache build"
        return 1
    fi
}

# Main monitoring loop
main() {
    log_info "Starting cache build monitor (check interval: ${CHECK_INTERVAL}s, stuck threshold: ${STUCK_THRESHOLD}s)"
    log_info "Monitor PID: $$"
    log_info "Log file: $LOG_FILE"
    
    local check_count=0
    local restart_count=0
    
    while true; do
        check_count=$((check_count + 1))
        if [ $((check_count % 12)) -eq 0 ]; then  # Every 12 checks (1 hour with 5min intervals)
            log_info "Monitor still running - Check #$check_count, Restarts: $restart_count"
        fi
        if is_process_running; then
            local pid=$(get_process_pid)
            log_info "Process running (PID: $pid), checking if stuck..."
            
            if stuck_reasons=$(is_process_stuck "$pid"); then
                log_warn "Process appears to be STUCK!"
                log_warn "Process PID: $pid"
                log_warn "Process runtime: $(ps -p $pid -o etime= 2>/dev/null || echo 'unknown')"
                echo "$stuck_reasons" | while read -r reason; do
                    log_warn "  - $reason"
                done
                
                # Log recent process activity
                local latest_log=$(get_latest_cache_log)
                if [ -f "$latest_log" ]; then
                    log_info "Recent log activity (last 5 lines):"
                    tail -5 "$latest_log" 2>/dev/null | while read -r line; do
                        log_info "  $line"
                    done
                fi
                
                # Kill and restart
                restart_count=$((restart_count + 1))
                log_warn "Restart attempt #$restart_count"
                
                if kill_stuck_process "$pid"; then
                    sleep 5
                    if restart_cache_build; then
                        log_info "Process restarted successfully (restart #$restart_count)"
                        log_info "Previous PID: $pid, New PID: $(get_process_pid)"
                    else
                        log_error "Failed to restart process"
                    fi
                else
                    log_error "Failed to kill stuck process"
                fi
            else
                log_info "Process is healthy and making progress"
            fi
        else
            log_warn "Process is not running, attempting to start..."
            if restart_cache_build; then
                log_info "Process started successfully"
            else
                log_error "Failed to start process"
            fi
        fi
        
        sleep "$CHECK_INTERVAL"
    done
}

# Run main loop
main

