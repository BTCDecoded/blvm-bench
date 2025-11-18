# Comparison: bllvm-bench vs Bitcoin Core bench_bitcoin

## Overview

This document compares our benchmarking system (`bllvm-bench`) with Bitcoin Core's `bench_bitcoin` tool.

## bench_bitcoin (Bitcoin Core)

### Framework
- **Tool**: `bench_bitcoin` (C++ executable)
- **Framework**: nanobench (C++ benchmarking library)
- **Location**: `core/src/bench/`
- **Benchmarks**: ~60+ C++ benchmark files

### Output Formats

#### 1. Default Table Format
```
|               ns/op |                op/s |    err% |     total | benchmark
|--------------------:|--------------------:|--------:|----------:|:----------
|       57,927,463.00 |               17.26 |    3.6% |      0.66 | `AddrManAdd`
|          677,816.00 |            1,475.33 |    4.9% |      0.01 | `AddrManGetAddr`
```

#### 2. JSON Output (Optional)
```bash
bench_bitcoin -output-json=results.json
```

**JSON Structure** (nanobench format):
```json
{
  "context": {
    "cpu": "...",
    "date": "...",
    "host_name": "...",
    "os": "..."
  },
  "results": [
    {
      "name": "AddrManAdd",
      "unit": "ns/op",
      "batch": 1,
      "complexityN": 0,
      "median(elapsed)": 57927463.00,
      "medianAbsolutePercentError(elapsed)": 3.6,
      "median(instructions)": ...,
      "median(cpucycles)": ...,
      "totalTime": 0.66,
      "epochs": ...
    }
  ]
}
```

#### 3. CSV Output (Optional)
```bash
bench_bitcoin -output-csv=results.csv
```

**CSV Format**:
```
# Benchmark, evals, iterations, total, min, max, median
AddrManAdd, 1, 1000, 0.66, 57000000, 59000000, 57927463
```

### Statistical Data
- **Mean/Median**: Point estimates
- **Error**: Median absolute percent error
- **Confidence Intervals**: Not explicitly in JSON (calculated from data)
- **Standard Deviation**: Available in nanobench but not always in JSON
- **CPU Metrics**: Instructions, CPU cycles (if available)

### Features
- ✅ Single implementation (Bitcoin Core only)
- ✅ Comprehensive benchmark coverage (~60+ benchmarks)
- ✅ Low-level metrics (CPU cycles, instructions)
- ✅ JSON output (nanobench format)
- ✅ CSV output
- ✅ Filtering (`-filter="regex"`)
- ✅ Minimum time control (`-min-time=ms`)
- ✅ List benchmarks (`-list`)

## bllvm-bench (Our System)

### Framework
- **Tool**: `bllvm-bench` (Rust + Shell scripts)
- **Framework**: Criterion (Rust) + bench_bitcoin (Core)
- **Location**: `commons/bllvm-bench/`
- **Benchmarks**: 38 benchmark scripts + 22 Rust/Criterion benchmarks

### Output Formats

#### 1. Individual JSON Files
**Format**: `{type}-{benchmark}-{timestamp}.json`

**Structure**:
```json
{
  "timestamp": "2025-11-18T12:34:56Z",
  "measurement_method": "Criterion benchmarks",
  "benchmarks": [
    {
      "name": "check_block",
      "time_ms": 4.51,
      "time_ns": 4510000,
      "statistics": {
        "mean": {
          "point_estimate": 4510000,
          "confidence_interval": {
            "lower_bound": 4400000,
            "upper_bound": 4620000,
            "confidence_level": 0.95
          },
          "standard_error": 55000
        },
        "median": {
          "point_estimate": 4500000,
          "confidence_interval": {
            "lower_bound": 4450000,
            "upper_bound": 4550000
          }
        },
        "std_dev": 120000,
        "median_abs_dev": {
          "point_estimate": 50000
        }
      }
    }
  ]
}
```

#### 2. Consolidated JSON
**Format**: `benchmark-results-consolidated-{timestamp}.json`

**Structure**:
```json
{
  "timestamp": "2025-11-18T12:34:56Z",
  "suite_directory": "results/suite-fair-...",
  "benchmarks": {
    "block_validation": {
      "core": { /* Core benchmark data */ },
      "commons": { /* Commons benchmark data */ },
      "comparison": {
        "winner": "commons",
        "speedup": 6.1,
        "core_time_ms": 66.25,
        "commons_time_ms": 10.80,
        "core_statistics": { /* Statistical data */ },
        "commons_statistics": { /* Statistical data */ }
      }
    }
  },
  "summary": {
    "total_benchmarks": 38,
    "core_benchmarks": 17,
    "commons_benchmarks": 21,
    "comparisons": 10
  }
}
```

#### 3. CSV Output
**Format**: `benchmark-results-consolidated-{timestamp}.csv`

**Structure**:
```csv
Benchmark,Implementation,Time (ms),Time (ns),Mean (ns),Median (ns),Std Dev (ns),CI Lower (ns),CI Upper (ns),Confidence Level,Ops/sec,Winner,Speedup
block_validation,Core,66.25,66250000,66250000,66000000,500000,65000000,67500000,0.95,15.09,commons,6.1
block_validation,Commons,10.80,10800000,10800000,10750000,200000,10500000,11100000,0.95,92.61,commons,6.1
```

### Statistical Data
- **Mean**: Point estimate, confidence interval, standard error
- **Median**: Point estimate, confidence interval, standard error
- **Standard Deviation**: Explicit value
- **Median Absolute Deviation**: Explicit value
- **Confidence Intervals**: 95% CI with lower/upper bounds
- **CPU Metrics**: Not yet (could be added)

### Features
- ✅ **Dual implementation** (Core vs Commons comparison)
- ✅ Comprehensive benchmark coverage (38 scripts + 22 Rust benchmarks)
- ✅ Statistical analysis (mean, median, CI, std dev, MAD)
- ✅ JSON output (individual + consolidated)
- ✅ CSV output (generated from consolidated JSON)
- ✅ Comparison logic (winners, speedups)
- ✅ Suite management (timestamped runs)
- ✅ Path discovery (auto-finds Core/Commons)
- ✅ Makefile targets (`make json`, `make csv`, `make all`)

## Key Differences

### Advantages of bench_bitcoin
1. **Mature tooling**: Well-established, used by Core developers
2. **Single focus**: Optimized for Core benchmarking only
3. **Direct nanobench integration**: Native JSON/CSV from framework
4. **Low-level metrics**: CPU cycles, instructions (via nanobench, if hardware counters available)

### Advantages of bllvm-bench
1. **Comparison capability**: Core vs Commons side-by-side
2. **Enhanced statistics**: More detailed statistical analysis (CI bounds, MAD)
3. **Consolidated output**: All benchmarks in one JSON file
4. **Comparison logic**: Automatic winner detection and speedup calculation
5. **Deep analysis**: Comprehensive low-level metrics (CPU cycles, IPC, cache, branch prediction)
6. **Portable**: Can be cloned and run anywhere
7. **Suite management**: Organized by timestamped runs
8. **Dual framework**: Uses both Criterion (Rust) and bench_bitcoin (Core)

## Statistical Comparison

### bench_bitcoin (nanobench)
- Mean/Median: ✅ Point estimates
- Confidence Intervals: ⚠️ Calculated but not always in JSON
- Standard Deviation: ⚠️ Available but not always exposed
- Error: ✅ Median absolute percent error
- CPU Metrics: ✅ Instructions, cycles (if available)

### bllvm-bench (Criterion + our extraction)
- Mean: ✅ Point estimate, CI, standard error
- Median: ✅ Point estimate, CI, standard error
- Standard Deviation: ✅ Explicit value
- Median Absolute Deviation: ✅ Explicit value
- Confidence Intervals: ✅ 95% CI with explicit bounds
- CPU Metrics: ✅ Yes (via perf - cycles, instructions, IPC, cache, branch prediction)

## Output Format Comparison

### bench_bitcoin JSON
```json
{
  "results": [
    {
      "name": "ConnectBlock",
      "median(elapsed)": 66250000,
      "medianAbsolutePercentError(elapsed)": 3.6,
      "totalTime": 0.66
    }
  ]
}
```

### bllvm-bench JSON
```json
{
  "benchmarks": [
    {
      "name": "connect_block",
      "time_ms": 66.25,
      "time_ns": 66250000,
      "statistics": {
        "mean": {
          "point_estimate": 66250000,
          "confidence_interval": {
            "lower_bound": 65000000,
            "upper_bound": 67500000,
            "confidence_level": 0.95
          },
          "standard_error": 625000
        },
        "median": { /* similar structure */ },
        "std_dev": 1250000
      }
    }
  ]
}
```

## Coverage Comparison

### bench_bitcoin
- **Total**: ~60+ benchmark files
- **Categories**: All Core operations (consensus, mempool, network, etc.)
- **Scope**: Bitcoin Core only

### bllvm-bench
- **Total**: 38 benchmark scripts + 22 Rust benchmarks
- **Categories**: Core vs Commons comparisons
- **Scope**: Dual implementation comparison

## Recommendations

### What We Should Add (to match bench_bitcoin)
1. **CPU Metrics**: Instructions, CPU cycles (via Criterion or perf)
2. **Low-level micro-benchmarks**: More isolated operations
3. **Direct nanobench JSON parsing**: For better Core data extraction

### What We Have That bench_bitcoin Doesn't
1. **Comparison capability**: Core vs Commons
2. **Enhanced statistics**: More detailed CI and MAD
3. **Consolidated output**: All benchmarks in one file
4. **Comparison logic**: Automatic winner/speedup calculation

## Deep Analysis Comparison

### bench_bitcoin Deep Core Analysis
- CPU cycles: ✅ (via nanobench)
- Instructions: ✅ (via nanobench)
- Statistical analysis: ✅
- Cache metrics: ⚠️ Limited

### bllvm-bench Deep Commons Analysis
- CPU cycles: ✅ (via perf)
- Instructions: ✅ (via perf)
- IPC (Instructions Per Cycle): ✅ (calculated)
- Cache performance: ✅ (L1/L2/L3 via perf)
- Branch prediction: ✅ (via perf)
- Statistical analysis: ✅ (Criterion)
- HTML reports: ✅

**Verdict**: bllvm-bench provides **more comprehensive** low-level metrics than bench_bitcoin.

## Conclusion

**bllvm-bench** is **complementary** to `bench_bitcoin`:
- `bench_bitcoin`: Optimized for Core-only benchmarking with mature tooling
- `bllvm-bench`: Optimized for Core vs Commons comparison with enhanced statistics and deep analysis

Our system provides:
- **Better statistical analysis** (CI bounds, MAD, std dev)
- **Comparison capabilities** (Core vs Commons)
- **Deeper low-level metrics** (cache, branch prediction, IPC)
- **Comprehensive tooling** (JSON, CSV, HTML reports)

For **comparison purposes** and **deep Commons analysis**, our system is superior. For **established Core-only benchmarking**, `bench_bitcoin` remains the standard.

