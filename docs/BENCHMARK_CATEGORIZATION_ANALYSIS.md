# Benchmark Categorization Analysis

## Summary

**JSON Summary Claims:**
- Total: 76 benchmarks
- Core: 14 benchmarks
- Commons: 24 benchmarks
- Comparisons: 12

**Actual Top-Level Benchmarks:**
- Total: 25 benchmarks
- Should be comparisons: **6**
- Should be core-only: **4**
- Should be commons-only: **15**
- Should be failed: **3**

## Issue: Summary Counts vs. Top-Level Benchmarks

The summary counts (76 total, 14 core, 24 commons, 12 comparisons) are counting **individual measurements**, not top-level benchmarks. For example:
- `block-validation-bench` has multiple measurements (all_schnorr, mixed, all_ecdsa) = 3 measurements
- `segwit-bench` has multiple measurements = multiple counts

**The page should display all 25 top-level benchmarks correctly categorized.**

## Correct Categorization

### Comparisons (6 benchmarks)
1. **block-serialization-bench** ✅
   - Core: Has `benchmarks` array with data
   - Commons: Has structure but error (still has data keys)
   
2. **compact-block-encoding-bench** ✅
   - Core: Has `bitcoin_core` structure
   - Commons: Has `bitcoin_commons` structure

3. **mempool-rbf-bench** ✅
   - Core: Has `benchmarks` array
   - Commons: Has `benchmarks` array

4. **ripemd160-bench** ✅
   - Core: Has `benchmarks` array
   - Commons: Has `benchmarks` array

5. **segwit-bench** ✅
   - Core: Has `bitcoin_core_segwit_operations` with `primary_comparison.time_per_block_ms`
   - Commons: Has `bitcoin_commons_segwit_operations` (but empty benchmarks array - needs fix)

6. **transaction-validation-bench** ✅
   - Core: Has `benchmarks` array
   - Commons: Has `benchmarks` array (but error - still has data)

### Core-Only (4 benchmarks)
1. **base58-bech32-bench** ✅
   - Core: Has `benchmarks` array
   - Commons: No data

2. **block-validation-bench** ✅
   - Core: Has `bitcoin_core_block_validation` with `connect_block_mixed_ecdsa_schnorr.time_per_block_ms`
   - Commons: Error only

3. **duplicate-inputs-bench** ✅
   - Core: Has `benchmarks` array
   - Commons: Error only

4. **transaction-sighash-bench** ✅
   - Core: Has `benchmarks` array (but error - still has data)
   - Commons: Error only

### Commons-Only (15 benchmarks)
1. **connectblock-bench** ✅
2. **deep-analysis** ✅
3. **hash-micro-bench** ✅
4. **mempool-acceptance-bench** ✅
5. **mempool-bench** ✅
6. **mempool-operations-bench** ✅
7. **merkle-root-bench** ✅
8. **node-sync-rpc** ✅
9. **standard-tx-bench** ✅
10. **transaction-id-bench** ✅
11. **transaction-serialization-bench** ✅
12. **utxo-caching-bench** ✅
13. (and 3 more)

### Failed (3 benchmarks)
1. **block-assembly-bench** ✅
   - Commons: Error only, no data keys

2. **merkle-tree-bench** ✅
   - Commons: Error only, no data keys

3. **script-verification-bench** ✅
   - Commons: Error only, no data keys

## Issues Found

### 1. Empty `benchmarks` Arrays
Some benchmarks have `benchmarks: []` (empty array) which was being treated as valid data.

**Fixed:** Now only treats arrays as valid if `length > 0`.

### 2. `primary_comparison` Not Checked First
`segwit-bench` has `bitcoin_core_segwit_operations.primary_comparison.time_per_block_ms` but it wasn't being found.

**Fixed:** Now checks `primary_comparison` before recursing into other nested objects.

### 3. Too Many Metadata Keys
Some keys like `implementation`, `benchmark`, `comparison_note` were being treated as data.

**Fixed:** Added these to the metadata keys list.

## Expected Page Display

After fixes, the page should show:
- **6 comparisons** (not 5, not 12)
- **4 core-only** (not 5)
- **15 commons-only** (not 10)
- **3 failed** (not 5)

**Total: 25 benchmarks** (all top-level benchmarks displayed)

## Why Summary Says 12 Comparisons

The summary is counting **individual measurements**, not top-level benchmarks. For example:
- `block-validation-bench` has 3 measurements (all_schnorr, mixed, all_ecdsa)
- `segwit-bench` has multiple measurements
- Each measurement is counted separately in the summary

The page correctly displays **top-level benchmarks** (25 total), not individual measurements (76 total).

