#!/bin/bash
# bllvm-bench Main Entry Point
# Run all benchmarks and generate reports

set -e

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Ensure we're in the bllvm-bench root (where run-benchmarks.sh is located)
cd "$SCRIPT_DIR"
# BLLVM_BENCH_ROOT is the same as SCRIPT_DIR since run-benchmarks.sh is in the root
BLLVM_BENCH_ROOT="$SCRIPT_DIR"

# Source common functions (includes path discovery and RESULTS_DIR)
if [ -f "scripts/shared/common.sh" ]; then
    source "scripts/shared/common.sh"
else
    echo "❌ Error: scripts/shared/common.sh not found"
    exit 1
fi

# Display discovered paths
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  bllvm-bench: Bitcoin Core vs Commons Benchmark Suite          ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "Discovered paths:"
if [ -n "$CORE_PATH" ]; then
    echo "  ✅ Bitcoin Core: $CORE_PATH"
else
    echo "  ⚠️  Bitcoin Core: Not found (Core benchmarks will be skipped)"
fi
if [ -n "$COMMONS_CONSENSUS_PATH" ]; then
    echo "  ✅ Bitcoin Commons (consensus): $COMMONS_CONSENSUS_PATH"
else
    echo "  ⚠️  Bitcoin Commons (consensus): Not found"
fi
if [ -n "$COMMONS_NODE_PATH" ]; then
    echo "  ✅ Bitcoin Commons (node): $COMMONS_NODE_PATH"
else
    echo "  ⚠️  Bitcoin Commons (node): Not found"
fi
echo "  📁 Results: $RESULTS_DIR"
echo ""

# Check if we have at least one implementation
if [ -z "$CORE_PATH" ] && [ -z "$COMMONS_CONSENSUS_PATH" ]; then
    echo "❌ Error: Neither Bitcoin Core nor Bitcoin Commons found"
    echo ""
    echo "Please:"
    echo "  1. Set paths in config/config.toml, or"
    echo "  2. Ensure Core/Commons are in standard locations:"
    echo "     - Core: ~/src/bitcoin, ~/src/bitcoin-core, or ../core"
    echo "     - Commons: ~/src/bllvm-consensus or ../bllvm-consensus"
    exit 1
fi

# Parse command line arguments
SUITE="${1:-fair}"
TIMEOUT="${2:-300}"

echo "Running benchmark suite: $SUITE"
echo "Default timeout: ${TIMEOUT}s"
echo ""

# Create results directory
mkdir -p "$RESULTS_DIR"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
SUITE_DIR="$RESULTS_DIR/suite-$SUITE-$TIMESTAMP"
mkdir -p "$SUITE_DIR"

# Track benchmarks
BENCHMARKS_RUN=()
BENCHMARKS_FAILED=()

run_benchmark() {
    local name="$1"
    local script="$2"
    local timeout_sec="${3:-$TIMEOUT}"
    
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Running: $name (timeout: ${timeout_sec}s)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    if [ ! -f "$script" ]; then
        echo "⚠️  Script not found: $script"
        BENCHMARKS_FAILED+=("$name (missing)")
        echo ""
        return 1
    fi
    
    # Check if timeout command exists
    if ! command -v timeout >/dev/null 2>&1; then
        echo "⚠️  timeout command not found, running without timeout"
        if bash "$script" "$SUITE_DIR" 2>&1 | tee "$SUITE_DIR/${name}.log"; then
            BENCHMARKS_RUN+=("$name")
            echo "✅ $name completed"
        else
            BENCHMARKS_FAILED+=("$name")
            echo "❌ $name failed (check log: $SUITE_DIR/${name}.log)"
        fi
    else
        # Run with timeout
        if timeout "$timeout_sec" bash "$script" "$SUITE_DIR" 2>&1 | tee "$SUITE_DIR/${name}.log"; then
            BENCHMARKS_RUN+=("$name")
            echo "✅ $name completed"
        else
            EXIT_CODE=$?
            if [ $EXIT_CODE -eq 124 ]; then
                BENCHMARKS_FAILED+=("$name (timeout)")
                echo "⏱️  $name timed out after ${timeout_sec}s"
            else
                BENCHMARKS_FAILED+=("$name")
                echo "❌ $name failed (check log: $SUITE_DIR/${name}.log)"
            fi
        fi
    fi
    echo ""
}

# Run benchmarks based on suite
case "$SUITE" in
    "fair"|"fair-fast")
        echo "Running fair comparison benchmarks..."
        
        # Block Validation
        if [ -n "$CORE_PATH" ]; then
            BENCH_SCRIPT="$BLLVM_BENCH_ROOT/scripts/core/block-validation-bench.sh"
            if [ -f "$BENCH_SCRIPT" ]; then
                run_benchmark "core-block-validation-bench" "$BENCH_SCRIPT" 300
            else
                echo "⚠️  Script not found: $BENCH_SCRIPT"
            fi
        fi
        if [ -n "$COMMONS_CONSENSUS_PATH" ]; then
            BENCH_SCRIPT="$BLLVM_BENCH_ROOT/scripts/commons/block-validation-bench.sh"
            if [ -f "$BENCH_SCRIPT" ]; then
                run_benchmark "commons-block-validation-bench" "$BENCH_SCRIPT" 300
            else
                echo "⚠️  Script not found: $BENCH_SCRIPT"
            fi
        fi
        
        # Transaction Validation
        if [ -n "$CORE_PATH" ]; then
            BENCH_SCRIPT="$BLLVM_BENCH_ROOT/scripts/core/transaction-validation-bench.sh"
            if [ -f "$BENCH_SCRIPT" ]; then
                run_benchmark "core-transaction-validation-bench" "$BENCH_SCRIPT" 300
            else
                echo "⚠️  Script not found: $BENCH_SCRIPT"
            fi
        fi
        if [ -n "$COMMONS_CONSENSUS_PATH" ]; then
            BENCH_SCRIPT="$BLLVM_BENCH_ROOT/scripts/commons/transaction-validation-bench.sh"
            if [ -f "$BENCH_SCRIPT" ]; then
                run_benchmark "commons-transaction-validation-bench" "$BENCH_SCRIPT" 300
            else
                echo "⚠️  Script not found: $BENCH_SCRIPT"
            fi
        fi
        
        # Mempool Operations
        if [ -n "$CORE_PATH" ]; then
            BENCH_SCRIPT="$BLLVM_BENCH_ROOT/scripts/core/mempool-operations-bench.sh"
            if [ -f "$BENCH_SCRIPT" ]; then
                run_benchmark "core-mempool-operations-bench" "$BENCH_SCRIPT" 300
            else
                echo "⚠️  Script not found: $BENCH_SCRIPT"
            fi
        fi
        if [ -n "$COMMONS_CONSENSUS_PATH" ]; then
            BENCH_SCRIPT="$BLLVM_BENCH_ROOT/scripts/commons/mempool-operations-bench.sh"
            if [ -f "$BENCH_SCRIPT" ]; then
                run_benchmark "commons-mempool-operations-bench" "$BENCH_SCRIPT" 300
            else
                echo "⚠️  Script not found: $BENCH_SCRIPT"
            fi
        fi
        
        # Run all other ported benchmarks
        for bench_script in "$BLLVM_BENCH_ROOT/scripts/core"/*.sh "$BLLVM_BENCH_ROOT/scripts/commons"/*.sh; do
            if [ -f "$bench_script" ]; then
                bench_name=$(basename "$bench_script" .sh)
                # Skip already run benchmarks
                if [[ ! "$bench_name" =~ ^(block-validation-bench|transaction-validation-bench|mempool-operations-bench)$ ]]; then
                    # Determine if it's a core or commons benchmark based on directory
                    if echo "$bench_script" | grep -q "/core/"; then
                        if [ -n "$CORE_PATH" ]; then
                            run_benchmark "core-${bench_name}" "$bench_script" 300
                        fi
                    elif echo "$bench_script" | grep -q "/commons/"; then
                        if [ -n "$COMMONS_CONSENSUS_PATH" ]; then
                            run_benchmark "commons-${bench_name}" "$bench_script" 300
                        fi
                    fi
                fi
            fi
        done
        ;;
    
    "all")
        echo "Running all available benchmarks for maximum coverage..."
        
        # Run special combined benchmarks first (RPC, Concurrent, Memory, Parallel)
        if [ -d "$BLLVM_BENCH_ROOT/scripts/shared/benchmarks" ]; then
            echo ""
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            echo "Running Special Combined Benchmarks"
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            for bench_script in "$BLLVM_BENCH_ROOT/scripts/shared/benchmarks"/*.sh; do
                if [ -f "$bench_script" ]; then
                    bench_name=$(basename "$bench_script" .sh)
                    run_benchmark "${bench_name}" "$bench_script" 600
                fi
            done
        fi
        
        # Run all Core benchmarks
        if [ -n "$CORE_PATH" ]; then
            echo ""
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            echo "Running Core Benchmarks"
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            for bench_script in "$BLLVM_BENCH_ROOT/scripts/core"/*.sh; do
                if [ -f "$bench_script" ]; then
                    bench_name=$(basename "$bench_script" .sh)
                    run_benchmark "core-${bench_name}" "$bench_script" 600
                fi
            done
        else
            echo "⚠️  Core path not found, skipping Core benchmarks"
        fi
        
        # Run all Commons benchmarks
        if [ -n "$COMMONS_CONSENSUS_PATH" ]; then
            echo ""
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            echo "Running Commons Benchmarks"
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            for bench_script in "$BLLVM_BENCH_ROOT/scripts/commons"/*.sh; do
                if [ -f "$bench_script" ]; then
                    bench_name=$(basename "$bench_script" .sh)
                    run_benchmark "commons-${bench_name}" "$bench_script" 600
                fi
            done
        else
            echo "⚠️  Commons path not found, skipping Commons benchmarks"
        fi
        ;;
    
    "core-only")
        if [ -z "$CORE_PATH" ]; then
            echo "❌ Error: Core path not found, cannot run core-only benchmarks"
            exit 1
        fi
        echo "Running Core-only benchmarks..."
        # Run only Core benchmarks
        ;;
    
    "commons-only")
        if [ -z "$COMMONS_CONSENSUS_PATH" ]; then
            echo "❌ Error: Commons path not found, cannot run commons-only benchmarks"
            exit 1
        fi
        echo "Running Commons-only benchmarks..."
        
        # Run all Commons benchmarks
        for bench_script in "$BLLVM_BENCH_ROOT/scripts/commons"/*.sh; do
            if [ -f "$bench_script" ]; then
                bench_name=$(basename "$bench_script" .sh)
                run_benchmark "commons-${bench_name}" "$bench_script" 300
            fi
        done
        ;;
    
    *)
        echo "Unknown suite: $SUITE"
        echo "Available suites: fair, fair-fast, all, core-only, commons-only"
        exit 1
        ;;
esac

# Generate summary
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  Benchmark Summary                                            ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "Results directory: $SUITE_DIR"
echo ""
echo "✅ Completed: ${#BENCHMARKS_RUN[@]} benchmarks"
for bench in "${BENCHMARKS_RUN[@]}"; do
    echo "   • $bench"
done
echo ""
if [ ${#BENCHMARKS_FAILED[@]} -gt 0 ]; then
    echo "❌ Failed/Timed Out: ${#BENCHMARKS_FAILED[@]} benchmarks"
    for bench in "${BENCHMARKS_FAILED[@]}"; do
        echo "   • $bench"
    done
    echo ""
fi

# Generate summary JSON
cat > "$SUITE_DIR/summary.json" << EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "suite_directory": "$SUITE_DIR",
  "suite_type": "$SUITE",
  "benchmarks_completed": ${#BENCHMARKS_RUN[@]},
  "benchmarks_failed": ${#BENCHMARKS_FAILED[@]},
  "completed": $(printf '%s\n' "${BENCHMARKS_RUN[@]}" | jq -R . | jq -s . 2>/dev/null || echo "[]"),
  "failed": $(printf '%s\n' "${BENCHMARKS_FAILED[@]}" | jq -R . | jq -s . 2>/dev/null || echo "[]")
}
EOF

echo "Summary saved to: $SUITE_DIR/summary.json"
echo ""
echo "✅ Benchmarks complete!"
echo ""

# Collect codebase metrics if enabled
# Check both COLLECT_METRICS env var and BENCH_SUITE variable (not parameter)
COLLECT_METRICS_FLAG="${COLLECT_METRICS:-false}"
BENCH_SUITE_VAL="${BENCH_SUITE:-$SUITE}"

if [ "$COLLECT_METRICS_FLAG" = "true" ] || [ "$BENCH_SUITE_VAL" = "all" ]; then
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Collecting Codebase Metrics"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "COLLECT_METRICS: $COLLECT_METRICS_FLAG"
    echo "BENCH_SUITE: $BENCH_SUITE_VAL"
    
    # Use suite directory if available, otherwise fall back to RESULTS_DIR
    METRICS_OUTPUT_DIR="${SUITE_DIR:-$RESULTS_DIR}"
    echo "Metrics output directory: $METRICS_OUTPUT_DIR"
    
    if [ -f "scripts/metrics/code-size.sh" ]; then
        echo "Running code size metrics..."
        bash -x ./scripts/metrics/code-size.sh "$METRICS_OUTPUT_DIR" 2>&1 || {
            EXIT_CODE=$?
            echo "⚠️  Code size metrics failed with exit code $EXIT_CODE"
        }
    else
        echo "⚠️  Code size metrics script not found"
    fi
    
    if [ -f "scripts/metrics/features.sh" ]; then
        echo "Running feature flags analysis..."
        bash -x ./scripts/metrics/features.sh "$METRICS_OUTPUT_DIR" 2>&1 || {
            EXIT_CODE=$?
            echo "⚠️  Feature flags analysis failed with exit code $EXIT_CODE"
        }
    else
        echo "⚠️  Features metrics script not found"
    fi
    
    if [ -f "scripts/metrics/tests.sh" ]; then
        echo "Running test metrics..."
        bash -x ./scripts/metrics/tests.sh "$METRICS_OUTPUT_DIR" 2>&1 || {
            EXIT_CODE=$?
            echo "⚠️  Test metrics failed with exit code $EXIT_CODE"
        }
    else
        echo "⚠️  Tests metrics script not found"
    fi
    
    # Phase 2: Combined views
    if [ -f "scripts/metrics/combined-view.sh" ]; then
        echo "Generating combined view..."
        bash -x ./scripts/metrics/combined-view.sh "$METRICS_OUTPUT_DIR" 2>&1 || {
            EXIT_CODE=$?
            echo "⚠️  Combined view failed with exit code $EXIT_CODE"
        }
    else
        echo "⚠️  Combined view script not found"
    fi
    
    if [ -f "scripts/metrics/full-view.sh" ]; then
        echo "Generating full view..."
        bash -x ./scripts/metrics/full-view.sh "$METRICS_OUTPUT_DIR" 2>&1 || {
            EXIT_CODE=$?
            echo "⚠️  Full view failed with exit code $EXIT_CODE"
        }
    else
        echo "⚠️  Full view script not found"
    fi
    
    echo "✅ Codebase metrics collection complete"
    echo ""
fi

echo "To generate a report, run:"
echo "  ./scripts/report/generate-report.sh"

