# bllvm-bench: Standalone Benchmarking System

This directory contains a **standalone benchmarking system** that can be cloned and run on any computer to compare Bitcoin Core and Bitcoin Commons performance.

## Quick Start

### Simplest (Recommended)

```bash
# Clone and setup
git clone https://github.com/BTCDecoded/bllvm-bench
cd bllvm-bench
make setup-auto    # Auto-clones dependencies if needed

# Run everything
make all           # Setup + Bench + Report
```

### Alternative: Manual Steps

```bash
# Clone bllvm-bench
git clone https://github.com/BTCDecoded/bllvm-bench
cd bllvm-bench

# Setup (auto-clones if needed)
./setup.sh

# Run benchmarks
make bench         # or: ./run-benchmarks.sh fair

# Generate report
make report        # or: ./scripts/report/generate-report.sh
```

### Using Make (Standard)

```bash
make help          # Show all available commands
make setup         # Interactive setup
make setup-auto   # Auto-setup (clones dependencies)
make bench         # Run benchmarks
make bench-fast    # Run fast benchmarks only
make report        # Generate report
make all           # Full workflow (setup + bench + report)
make check         # Check if dependencies are found
make clean         # Clean results
```

## Dependency Management

bllvm-bench supports **multiple ways** to handle Bitcoin Core and Bitcoin Commons:

### 1. Auto-Discovery (Default)
The system automatically searches for existing installations in common locations:
- Core: `~/src/bitcoin`, `~/src/bitcoin-core`, `../core`
- Commons: `~/src/bllvm-consensus`, `../bllvm-consensus`

**No cloning required** - just ensure Core/Commons are already installed.

### 2. Automatic Setup (Recommended)
Run `make setup-auto` or `./setup.sh`:
- **Auto-clones** Core/Commons if not found (non-interactive)
- Clones to `../dependencies/` by default (configurable via `BLLVM_BENCH_CLONE_DIR`)
- Updates `config/config.toml` automatically
- **Easiest option** - just run one command

### 3. Interactive Setup Script
Run `./scripts/setup-dependencies.sh` to interactively clone dependencies:
- Prompts to clone Core/Commons if not found
- Clones to `../dependencies/` by default (configurable via `BLLVM_BENCH_CLONE_DIR`)
- Updates `config/config.toml` automatically

### 4. Manual Configuration
Create `config/config.toml` with explicit paths (see below).

**Note**: bllvm-bench does **NOT** use git submodules. Dependencies are either:
- Discovered from existing installations, or
- Cloned separately (via setup script or manually)

## Requirements

- **Bitcoin Core**: Built with `bench_bitcoin` target (optional, for Core benchmarks)
- **Bitcoin Commons**: `bllvm-consensus` and `bllvm-node` (optional, for Commons benchmarks)
- **Tools**: `bash`, `jq`, `cargo` (Rust), `timeout` command, `git` (for cloning)

## Path Discovery

The system automatically discovers Bitcoin Core and Bitcoin Commons:

1. **Configuration file** (optional): `config/config.toml`
2. **Auto-discovery**: Searches common locations:
   - Core: `~/src/bitcoin`, `~/src/bitcoin-core`, `../core`
   - Commons: `~/src/bllvm-consensus`, `../bllvm-consensus`

### Manual Configuration

Create `config/config.toml`:

```toml
[paths]
core_path = "/path/to/bitcoin-core"
commons_consensus_path = "/path/to/bllvm-consensus"
commons_node_path = "/path/to/bllvm-node"
```

## Directory Structure

```
bllvm-bench/
├── scripts/
│   ├── core/              # Core benchmark scripts
│   ├── commons/            # Commons benchmark scripts
│   ├── shared/            # Common functions
│   ├── discover-paths.sh  # Path discovery
│   └── config.toml.example
├── report/
│   └── generate-report.sh # Report generator
├── results/               # Benchmark results (generated)
├── benches/               # Rust Criterion benchmarks
└── run-benchmarks.sh      # Main entry point
```

## Running Benchmarks

### All Fair Benchmarks

```bash
./run-benchmarks.sh fair
```

### Specific Suite

```bash
./run-benchmarks.sh fair-fast    # Fast benchmarks only
./run-benchmarks.sh all          # All benchmarks
./run-benchmarks.sh core-only    # Core benchmarks only
./run-benchmarks.sh commons-only # Commons benchmarks only
```

### Individual Benchmarks

```bash
# Core block validation
./scripts/core/block-validation-bench.sh

# Commons block validation
./scripts/commons/block-validation-bench.sh
```

## Results

Results are stored in `results/` directory:

- `results/suite-{suite}-{timestamp}/` - Individual benchmark results
- `results/performance-summary.html` - Generated HTML report

## Report Generation

```bash
./scripts/report/generate-report.sh
```

This generates `results/performance-summary.html` with all benchmark comparisons.

## Migration from node-comparison

The benchmarking system has been migrated from `node-comparison/benchmarks/` to be standalone:

- ✅ Path discovery (no hardcoded paths)
- ✅ Portable scripts (work from any directory)
- ✅ Self-contained (all dependencies in bllvm-bench)
- ✅ Configuration support (optional config.toml)

## Adding New Benchmarks

1. Create script in `scripts/core/` or `scripts/commons/`
2. Source `scripts/shared/common.sh` for path discovery
3. Use `get_output_dir` helper for results directory
4. Add to `run-benchmarks.sh` suite definitions

## Troubleshooting

### "Core path not found"
- Build Core with `make bench_bitcoin`
- Set `core_path` in `config/config.toml`
- Or place Core in `~/src/bitcoin` or `../core`

### "Commons path not found"
- Clone `bllvm-consensus` and `bllvm-node`
- Set paths in `config/config.toml`
- Or place in `~/src/bllvm-consensus` or `../bllvm-consensus`

### "bench_bitcoin not found"
- Build Core: `cd $CORE_PATH && make bench_bitcoin`
- Or ensure `bench_bitcoin` is in PATH

