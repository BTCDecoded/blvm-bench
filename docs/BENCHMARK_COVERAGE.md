# Benchmark Coverage Analysis

## Comparison: What We Have vs What We Had

### Original node-comparison Benchmarks
Total: ~40 benchmark scripts covering:
- Block validation
- Transaction validation
- Mempool operations
- Script verification
- SegWit operations
- Merkle tree operations
- UTXO caching
- Hash operations
- Serialization
- RPC performance
- Concurrent operations
- Memory efficiency
- Node sync

### Ported to bllvm-bench
- **Core benchmarks**: 17 scripts
- **Commons benchmarks**: 21 scripts
- **Total**: 38 scripts

## Bitcoin Core bench_bitcoin Standard Benchmarks

Bitcoin Core's `bench_bitcoin` includes:

### Core Consensus Operations
1. **Block Validation**
   - `ConnectBlock` (all ECDSA, all Schnorr, mixed)
   - `DeserializeAndCheckBlockTest`
   - Block assembly
   - Block serialization

2. **Transaction Operations**
   - Transaction validation
   - Transaction serialization
   - Transaction ID calculation
   - Transaction sighash calculation

3. **Script Verification**
   - Script verification (P2PKH, P2WPKH, P2SH, etc.)
   - Script evaluation
   - Signature verification (ECDSA, Schnorr)

4. **Mempool Operations**
   - Mempool acceptance
   - Mempool eviction
   - RBF (Replace-by-Fee) checks
   - Standard transaction checks

5. **Cryptographic Operations**
   - SHA256 hashing
   - RIPEMD160 hashing
   - ECDSA signing/verification
   - Schnorr signing/verification

6. **Data Structures**
   - Merkle root calculation
   - UTXO set operations
   - Base58/Bech32 encoding

7. **Serialization**
   - Block serialization
   - Transaction serialization
   - Compact block serialization

## What We're Missing

### Low-Level Micro-benchmarks
- [ ] Individual hash function performance (SHA256, RIPEMD160)
- [ ] Signature verification isolated (ECDSA vs Schnorr)
- [ ] Memory allocation patterns
- [ ] Cache performance (L1/L2/L3 misses)
- [ ] CPU instruction counts

### Integration Benchmarks
- [ ] Full node sync (IBD - Initial Block Download)
- [ ] Block propagation
- [ ] Peer connection handling
- [ ] Database operations (LevelDB, etc.)
- [ ] Network I/O

### Stress Tests
- [ ] High transaction throughput
- [ ] Large mempool (10k+ transactions)
- [ ] Concurrent block validation
- [ ] Memory pressure tests

### Regression Testing
- [ ] Version-to-version comparisons
- [ ] Performance regression detection
- [ ] Historical benchmark tracking

## Standard Benchmarking Features

### Metrics We Should Add
1. **Latency Distribution**
   - p50, p95, p99 percentiles
   - Min/max values
   - Standard deviation

2. **Throughput**
   - Operations per second
   - Transactions per second
   - Blocks per second

3. **Resource Usage**
   - Memory (RSS, heap, stack)
   - CPU utilization
   - I/O operations
   - Cache performance

4. **Statistical Analysis**
   - Confidence intervals
   - Outlier detection
   - Warmup detection

### Output Formats
- [x] JSON (individual benchmarks)
- [x] JSON (consolidated)
- [ ] CSV (for spreadsheet analysis)
- [ ] JUnit XML (for CI integration)
- [ ] Benchmark.js format (for web visualization)

### Comparison Features
- [x] Core vs Commons comparison
- [ ] Historical comparison (version tracking)
- [ ] Regression detection
- [ ] Performance trends

## Custom C++ Benchmarks

If there are custom C++ benchmarks in `../bitcoin`:
1. Identify which operations they benchmark
2. Ensure we have equivalent Rust benchmarks in `bllvm-bench`
3. Port the comparison logic to our system

## Recommendations

### High Priority
1. **Add low-level micro-benchmarks**
   - Individual hash functions
   - Signature verification isolated
   - Memory allocation patterns

2. **Add statistical analysis**
   - Percentiles (p50, p95, p99)
   - Confidence intervals
   - Outlier detection

3. **Add CSV output**
   - For spreadsheet analysis
   - For historical tracking

### Medium Priority
1. **Add stress tests**
   - High transaction throughput
   - Large mempool operations
   - Concurrent operations

2. **Add integration benchmarks**
   - Full node sync
   - Block propagation
   - Database operations

### Low Priority
1. **Add regression testing**
   - Version tracking
   - Performance regression detection
   - Historical comparisons

2. **Add more output formats**
   - JUnit XML
   - Benchmark.js format

