#!/bin/bash
# Helper script to port a benchmark from node-comparison to bllvm-bench
# Usage: ./port-benchmark.sh {core|commons} {benchmark-name}

set -e

if [ $# -lt 2 ]; then
    echo "Usage: $0 {core|commons} {benchmark-name}"
    echo "Example: $0 core transaction-validation-bench"
    exit 1
fi

TYPE="$1"
BENCH_NAME="$2"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BLLVM_BENCH_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
NODE_COMPARISON_ROOT="${NODE_COMPARISON_ROOT:-$BLLVM_BENCH_ROOT/../../..}"

SOURCE_FILE="$NODE_COMPARISON_ROOT/benchmarks/${TYPE}-${BENCH_NAME}.sh"
TARGET_DIR="$BLLVM_BENCH_ROOT/scripts/${TYPE}"
TARGET_FILE="$TARGET_DIR/${BENCH_NAME}.sh"

if [ ! -f "$SOURCE_FILE" ]; then
    echo "❌ Source file not found: $SOURCE_FILE"
    exit 1
fi

if [ -f "$TARGET_FILE" ]; then
    echo "⏭️  Already exists: ${TYPE}/${BENCH_NAME}.sh"
    exit 0
fi

echo "Porting ${TYPE}-${BENCH_NAME}..."

# Read source and create portable version
{
    echo "#!/bin/bash"
    echo "# $(grep "^# " "$SOURCE_FILE" | head -1 | sed 's/^# //') (Portable)"
    echo ""
    echo "set -e"
    echo ""
    echo "# Source common functions"
    echo "SCRIPT_DIR=\"\$(cd \"\$(dirname \"\${BASH_SOURCE[0]}\")\" && pwd)\""
    echo "source \"\$SCRIPT_DIR/../shared/common.sh\""
    echo ""
    echo "OUTPUT_DIR=\$(get_output_dir \"\${1:-\$RESULTS_DIR}\")"
    echo "OUTPUT_FILE=\"\$OUTPUT_DIR/${TYPE}-${BENCH_NAME}-\$(date +%Y%m%d-%H%M%S).json\""
    echo ""
    
    # Extract main logic, replacing paths
    grep -v "^#!/bin/bash" "$SOURCE_FILE" | \
    grep -v "^set -e" | \
    grep -v "^OUTPUT_DIR=" | \
    grep -v "^OUTPUT_DIR=\$(cd" | \
    grep -v "^mkdir -p \"\$OUTPUT_DIR\"" | \
    grep -v "^SCRIPT_DIR=" | \
    grep -v "^PROJECT_ROOT=" | \
    sed "s|\$PROJECT_ROOT/core|\$CORE_PATH|g" | \
    sed "s|\$PROJECT_ROOT/commons/bllvm-bench|\$BLLVM_BENCH_ROOT|g" | \
    sed "s|\$PROJECT_ROOT/commons/bllvm-consensus|\$COMMONS_CONSENSUS_PATH|g" | \
    sed "s|\$PROJECT_ROOT/commons/bllvm-node|\$COMMONS_NODE_PATH|g" | \
    sed "s|\$BENCH_DIR|\$BLLVM_BENCH_ROOT|g" | \
    sed "s|OUTPUT_DIR=\"\${1:-\$(dirname \"\$0\")/\.\./results\}\"|# OUTPUT_DIR already set|" | \
    sed "s|OUTPUT_DIR=\$(cd \"\$OUTPUT_DIR\" 2>/dev/null && pwd|# OUTPUT_DIR already set|" | \
    sed "s|OUTPUT_DIR=\$(cd \"\$OUTPUT_DIR\" && pwd|# OUTPUT_DIR already set|" | \
    sed "s|mkdir -p \"\$OUTPUT_DIR\"|# OUTPUT_DIR already created|" | \
    sed "s|\$OUTPUT_DIR/${TYPE}-${BENCH_NAME}-|\$OUTPUT_FILE|" | \
    sed "s|\$OUTPUT_DIR/commons-${BENCH_NAME}-|\$OUTPUT_FILE|" | \
    sed "s|\$OUTPUT_DIR/core-${BENCH_NAME}-|\$OUTPUT_FILE|"
} > "$TARGET_FILE"

chmod +x "$TARGET_FILE"
echo "✅ Ported to: $TARGET_FILE"
