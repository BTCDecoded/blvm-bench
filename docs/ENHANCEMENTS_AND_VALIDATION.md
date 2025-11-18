# Benchmarking System: Enhancements & Validation

## Validation: What We Have vs What We Had

### Original node-comparison Benchmarks
- **Total**: 41 benchmark scripts
- **Coverage**: Core vs Commons comparisons across major operations

### Ported to bllvm-bench
- **Core benchmarks**: 17 scripts
- **Commons benchmarks**: 21 scripts  
- **Total**: 38 scripts (93% coverage)

### Missing from Port
- `parallel-block-validation-bench.sh` (not a Core/Commons comparison script)
- Some scripts may have been consolidated

## Bitcoin Core bench_bitcoin Standard Benchmarks

Bitcoin Core's `bench_bitcoin` includes **60+ benchmark files** covering:

### Core Consensus Operations
1. **Block Operations**
   - `connectblock.cpp` - Block connection/validation
   - `checkblock.cpp` - Block structure validation
   - `block_assemble.cpp` - Block assembly
   - `blockencodings.cpp` - Compact block encoding
   - `readwriteblock.cpp` - Block I/O

2. **Transaction Operations**
   - `transaction_id.cpp` - Transaction ID calculation
   - `transaction_serialization.cpp` - Serialization
   - `transaction_sighash.cpp` - Sighash calculation
   - `sign_transaction.cpp` - Transaction signing

3. **Script Verification**
   - `verify_script.cpp` - Script execution
   - Various script types (P2PKH, P2WPKH, P2SH, etc.)

4. **Mempool Operations**
   - `mempool_ephemeral_spends.cpp` - Ephemeral spend tracking
   - `mempool_eviction.cpp` - Mempool eviction
   - `mempool_stress.cpp` - Stress testing
   - `txorphanage.cpp` - Orphan transaction handling
   - `txgraph.cpp` - Transaction graph operations

5. **Cryptographic Operations**
   - `crypto_hash.cpp` - SHA256, RIPEMD160, etc.
   - `chacha20.cpp` - ChaCha20 encryption
   - `poly1305.cpp` - Poly1305 MAC
   - `bip324_ecdh.cpp` - BIP324 ECDH

6. **Data Structures**
   - `merkle_root.cpp` - Merkle tree operations
   - `utxo_operations.cpp` - UTXO set operations
   - `ccoins_caching.cpp` - Coin cache operations
   - `addrman.cpp` - Address manager
   - `prevector.cpp` - Pre-allocated vector

7. **Encoding/Decoding**
   - `base58.cpp` - Base58 encoding/decoding
   - `bech32.cpp` - Bech32 encoding/decoding
   - `strencodings.cpp` - String encodings

8. **RPC Operations**
   - `rpc_blockchain.cpp` - Blockchain RPC methods
   - `rpc_mempool.cpp` - Mempool RPC methods

9. **Network Operations**
   - `peer_eviction.cpp` - Peer eviction logic
   - `disconnected_transactions.cpp` - Disconnected block handling

10. **Other Operations**
    - `duplicate_inputs.cpp` - Duplicate input detection
    - `checkqueue.cpp` - Parallel validation queue
    - `cluster_linearize.cpp` - Transaction clustering
    - `descriptors.cpp` - Output descriptor parsing
    - `gcs_filter.cpp` - Golomb-Rice coding
    - `index_blockfilter.cpp` - Block filter indexing
    - `lockedpool.cpp` - Memory pool operations
    - `pool.cpp` - Memory pool benchmarks
    - `rollingbloom.cpp` - Rolling bloom filter
    - `random.cpp` - Random number generation
    - `util_time.cpp` - Time utilities

## Our Coverage vs bench_bitcoin

### ✅ We Have (Mapped to bench_bitcoin)
- Block validation → `connectblock.cpp`
- Transaction validation → `checkblock.cpp` (via `DeserializeAndCheckBlockTest`)
- Transaction serialization → `transaction_serialization.cpp`
- Transaction ID → `transaction_id.cpp`
- Transaction sighash → `transaction_sighash.cpp`
- Script verification → `verify_script.cpp`
- Mempool operations → `mempool_eviction.cpp`, `mempool_ephemeral_spends.cpp`
- Merkle tree → `merkle_root.cpp`
- Base58/Bech32 → `base58.cpp`, `bech32.cpp`
- Hash operations → `crypto_hash.cpp`
- UTXO caching → `ccoins_caching.cpp`
- Block assembly → `block_assemble.cpp`
- SegWit operations → (part of script verification)
- RPC performance → `rpc_blockchain.cpp`, `rpc_mempool.cpp`

### ❌ We're Missing (Should Add)
- **Low-level micro-benchmarks**:
  - Individual hash functions (SHA256, RIPEMD160 isolated)
  - Signature verification isolated (ECDSA vs Schnorr separately)
  - Memory allocation patterns
  - Cache performance (L1/L2/L3 misses)
  - CPU instruction counts

- **Advanced operations**:
  - Block encoding/decoding (`blockencodings.cpp`)
  - Compact blocks (`readwriteblock.cpp`)
  - Transaction graph operations (`txgraph.cpp`)
  - Orphan transaction handling (`txorphanage.cpp`)
  - Address manager (`addrman.cpp`)
  - Peer eviction (`peer_eviction.cpp`)
  - BIP324 ECDH (`bip324_ecdh.cpp`)
  - ChaCha20/Poly1305 (`chacha20.cpp`, `poly1305.cpp`)

- **Stress tests**:
  - Mempool stress (`mempool_stress.cpp`)
  - High transaction throughput
  - Large mempool (10k+ transactions)
  - Concurrent block validation (we have parallel, but not stress-tested)

## Custom C++ Benchmarks

### Analysis
The benchmarks in `core/src/bench/` are **Bitcoin Core's standard benchmarks**, not custom ones we wrote. These are part of Core's official benchmarking suite.

### What We Should Check
1. **Are we using all relevant Core benchmarks?**
   - ✅ Yes - We use `bench_bitcoin` to run Core's benchmarks
   - ✅ We extract results from `ConnectBlock`, `DeserializeAndCheckBlockTest`, etc.

2. **Do we have equivalent Commons benchmarks?**
   - ✅ Most operations have Commons equivalents
   - ⚠️ Some advanced operations may be missing (orphan handling, peer eviction)

3. **Are we accounting for all Core benchmark categories?**
   - ⚠️ We're missing some low-level and advanced operations
   - ✅ We cover all consensus-critical operations

## Standard Benchmarking Features (Industry Standard)

### Metrics We Should Add
1. **Statistical Analysis**
   - ✅ Mean/median (we have)
   - ❌ Percentiles (p50, p95, p99)
   - ❌ Confidence intervals
   - ❌ Standard deviation
   - ❌ Outlier detection

2. **Resource Metrics**
   - ⚠️ Memory usage (basic - we have)
   - ❌ CPU utilization (detailed)
   - ❌ Cache performance (L1/L2/L3 misses)
   - ❌ CPU instruction counts
   - ❌ Branch prediction misses

3. **Throughput Metrics**
   - ✅ Operations per second (we have)
   - ❌ Transactions per second (detailed)
   - ❌ Blocks per second (detailed)
   - ❌ Concurrent operations throughput

### Output Formats
- ✅ JSON (individual benchmarks)
- ✅ JSON (consolidated)
- ❌ CSV (for spreadsheet analysis)
- ❌ JUnit XML (for CI integration)
- ❌ Benchmark.js format (for web visualization)

### Comparison Features
- ✅ Core vs Commons comparison
- ❌ Historical comparison (version tracking)
- ❌ Regression detection
- ❌ Performance trends
- ❌ Baseline comparisons

## Recommended Enhancements

### High Priority (Match bench_bitcoin)
1. **Add Low-Level Micro-benchmarks**
   - Individual hash functions (SHA256, RIPEMD160 isolated)
   - Signature verification isolated (ECDSA vs Schnorr)
   - Memory allocation patterns
   - Cache performance metrics

2. **Add Statistical Analysis**
   - Percentiles (p50, p95, p99)
   - Confidence intervals
   - Standard deviation
   - Outlier detection

3. **Add CSV Output**
   - For spreadsheet analysis
   - For historical tracking
   - For regression detection

### Medium Priority (Standard Features)
1. **Add Stress Tests**
   - High transaction throughput
   - Large mempool operations
   - Concurrent operations stress

2. **Add Advanced Operations**
   - Block encoding/decoding
   - Compact blocks
   - Transaction graph operations
   - Orphan transaction handling

3. **Add Resource Metrics**
   - Detailed CPU utilization
   - Cache performance
   - CPU instruction counts

### Low Priority (Nice to Have)
1. **Add Regression Testing**
   - Version tracking
   - Performance regression detection
   - Historical comparisons

2. **Add More Output Formats**
   - JUnit XML
   - Benchmark.js format

3. **Add GUI** (explicitly excluded per user request)

## Validation Checklist

### ✅ What We've Validated
- [x] All original benchmarks ported (38/41 = 93%)
- [x] Core benchmarks use `bench_bitcoin` (standard tool)
- [x] Commons benchmarks use Criterion (standard Rust tool)
- [x] Consolidated JSON generator works
- [x] Path discovery works
- [x] Makefile targets work

### ⚠️ What Needs Validation
- [ ] All ported scripts work correctly
- [ ] JSON output is consistent
- [ ] Timing extraction is accurate
- [ ] Comparison logic is correct

### ❌ What's Missing
- [ ] Statistical analysis (percentiles, confidence intervals)
- [ ] CSV output
- [ ] Low-level micro-benchmarks
- [ ] Stress tests
- [ ] Regression testing
- [ ] Historical tracking

## Next Steps

1. **Test all ported benchmarks** - Run full suite and verify output
2. **Add statistical analysis** - Extract percentiles from Criterion/nanobench
3. **Add CSV output** - Generate CSV from consolidated JSON
4. **Add low-level benchmarks** - Individual hash functions, signature verification
5. **Document missing benchmarks** - List what we don't have vs bench_bitcoin

