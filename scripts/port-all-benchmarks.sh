#!/bin/bash
# Port all remaining benchmarks from node-comparison to bllvm-bench
# This script systematically ports all benchmark scripts

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BLLVM_BENCH_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
NODE_COMPARISON_ROOT="${NODE_COMPARISON_ROOT:-$BLLVM_BENCH_ROOT/../../..}"

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  Porting All Benchmarks to bllvm-bench                        ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# Function to port a single benchmark
port_benchmark() {
    local type="$1"
    local bench_name="$2"
    local source_file="$NODE_COMPARISON_ROOT/benchmarks/${type}-${bench_name}.sh"
    local target_dir="$BLLVM_BENCH_ROOT/scripts/${type}"
    local target_file="$target_dir/${bench_name}.sh"
    
    if [ ! -f "$source_file" ]; then
        echo "⚠️  Source not found: ${type}-${bench_name}"
        return 1
    fi
    
    if [ -f "$target_file" ]; then
        echo "⏭️  Already exists: ${type}/${bench_name}.sh"
        return 0
    fi
    
    # Read source and make portable
    mkdir -p "$target_dir"
    
    # Create portable version
    {
        echo "#!/bin/bash"
        echo "# $(grep "^# " "$source_file" | head -1 | sed 's/^# //') (Portable)"
        echo ""
        echo "set -e"
        echo ""
        echo "# Source common functions"
        echo "SCRIPT_DIR=\"\$(cd \"\$(dirname \"\${BASH_SOURCE[0]}\")\" && pwd)\""
        echo "source \"\$SCRIPT_DIR/../shared/common.sh\""
        echo ""
        
        # Extract the main logic, replacing paths
        grep -v "^#!/bin/bash" "$source_file" | \
        grep -v "^set -e" | \
        grep -v "^OUTPUT_DIR=" | \
        grep -v "^OUTPUT_DIR=\$(cd" | \
        grep -v "^mkdir -p \"\$OUTPUT_DIR\"" | \
        grep -v "^SCRIPT_DIR=" | \
        grep -v "^PROJECT_ROOT=" | \
        grep -v "^BENCH_DIR=" | \
        grep -v "^CORE_DIR=" | \
        sed "s|\$PROJECT_ROOT/core|\$CORE_PATH|g" | \
        sed "s|\$PROJECT_ROOT/commons/bllvm-bench|\$BLLVM_BENCH_ROOT|g" | \
        sed "s|\$PROJECT_ROOT/commons/bllvm-consensus|\$COMMONS_CONSENSUS_PATH|g" | \
        sed "s|\$PROJECT_ROOT/commons/bllvm-node|\$COMMONS_NODE_PATH|g" | \
        sed "s|OUTPUT_DIR=\"\${1:-\$(dirname \"\$0\")/\.\./results\}\"|OUTPUT_DIR=\$(get_output_dir \"\${1:-\$RESULTS_DIR}\")|" | \
        sed "s|OUTPUT_DIR=\$(cd \"\$OUTPUT_DIR\" 2>/dev/null && pwd|# OUTPUT_DIR already set|" | \
        sed "s|OUTPUT_DIR=\$(cd \"\$OUTPUT_DIR\" && pwd|# OUTPUT_DIR already set|" | \
        sed "s|mkdir -p \"\$OUTPUT_DIR\"|# OUTPUT_DIR already created by get_output_dir|"
    } > "$target_file"
    
    chmod +x "$target_file"
    echo "✅ Ported: ${type}/${bench_name}.sh"
}

# Port all Core benchmarks
echo "Porting Core benchmarks..."
CORE_BENCHES=$(ls -1 "$NODE_COMPARISON_ROOT/benchmarks"/core-*-bench.sh 2>/dev/null | sed 's|.*/core-||' | sed 's|.sh||' | grep -v "^block-validation$" || true)
for bench in $CORE_BENCHES; do
    port_benchmark "core" "$bench"
done

echo ""

# Port all Commons benchmarks
echo "Porting Commons benchmarks..."
COMMONS_BENCHES=$(ls -1 "$NODE_COMPARISON_ROOT/benchmarks"/commons-*-bench.sh 2>/dev/null | sed 's|.*/commons-||' | sed 's|.sh||' | grep -v "^block-validation$" || true)
for bench in $COMMONS_BENCHES; do
    port_benchmark "commons" "$bench"
done

echo ""
echo "✅ Porting complete!"
echo ""
echo "Review ported scripts in:"
echo "  $BLLVM_BENCH_ROOT/scripts/core/"
echo "  $BLLVM_BENCH_ROOT/scripts/commons/"

