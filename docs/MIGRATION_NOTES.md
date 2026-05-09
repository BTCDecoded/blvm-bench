# Benchmark Migration Notes

## Overview

All benchmarking code has been consolidated into the `blvm-bench` crate. This is a development-only crate that supports testing in both development and production modes.

## What Was Moved

### From `blvm-consensus/benches/`:
- `hash_operations.rs`
- `block_validation.rs`
- `block_validation_realistic.rs`
- `mempool_operations.rs`
- `segwit_operations.rs`
- `transaction_validation.rs`
- `utxo_commitments.rs`
- `blvm_optimizations.rs`
- `performance_focused.rs`

### From `blvm-node/benches/`:
- `compact_blocks.rs`
- `dandelion_bench.rs`
- `storage_operations.rs`
- `transport_comparison.rs`

## Changes Made

1. **Updated imports**: Changed `reference_node` → `blvm_node`
2. **Fixed type mismatches**: Updated to use `tx_inputs!` and `tx_outputs!` macros for SmallVec compatibility
3. **Fixed Block structure**: Updated `transactions` field to use `Box<[Transaction]>`
4. **Enabled features**: Added `dandelion` and `utxo-commitments` features where needed

## Usage

### Running Rust Benchmarks

```bash
# Development mode (default)
cargo bench

# Production mode
cargo bench --features production

# Specific benchmark
cargo bench --bench hash_operations

# Using the CLI tool
cargo run --bin blvm-bench -- rust
cargo run --bin blvm-bench -- rust --production
cargo run --bin blvm-bench -- rust hash_operations
```

### Running Shell Benchmarks

```bash
# Using the CLI tool
cargo run --bin blvm-bench -- shell --all
cargo run --bin blvm-bench -- shell run-all-fair-fast-benchmarks
```

### Running All Benchmarks

```bash
cargo run --bin blvm-bench -- all
cargo run --bin blvm-bench -- all --production
```

## Notes

- The original `benches/` directories in `blvm-consensus` and `blvm-node` still exist but are no longer used
- Bench definitions have been removed from both `Cargo.toml` files
- All benchmarks compile successfully in `blvm-bench`

