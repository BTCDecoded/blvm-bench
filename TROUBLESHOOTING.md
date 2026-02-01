# Troubleshooting Guide

Common issues and solutions for `bllvm-bench`.

## Dependencies Not Found

### Bitcoin Core not found

**Symptoms:**
```
❌ Bitcoin Core: Not found
```

**Solutions:**
1. Clone Bitcoin Core:
   ```bash
   git clone https://github.com/bitcoin/bitcoin.git ~/src/bitcoin
   cd ~/src/bitcoin
   ./autogen.sh
   ./configure
   make bench_bitcoin
   ```

2. Or set path in `config/config.toml`:
   ```toml
   [paths]
   core_path = "/path/to/bitcoin"
   ```

3. Or use auto-setup:
   ```bash
   make setup-auto
   ```

### bench_bitcoin not built

**Symptoms:**
```
⚠️  bench_bitcoin: Not built
```

**Solution:**
```bash
cd $CORE_PATH
make bench_bitcoin
```

### bllvm-consensus not found

**Symptoms:**
```
❌ bllvm-consensus: Not found
```

**Solutions:**
1. Clone bllvm-consensus:
   ```bash
   git clone https://github.com/BTCDecoded/bllvm-consensus.git ~/src/bllvm-consensus
   ```

2. Or set path in `config/config.toml`:
   ```toml
   [paths]
   commons_consensus_path = "/path/to/bllvm-consensus"
   ```

3. Or use auto-setup:
   ```bash
   make setup-auto
   ```

## Benchmark Failures

### "No benchmark suite found"

**Symptoms:**
```
❌ No benchmark suite found. Run benchmarks first.
```

**Solution:**
Run benchmarks first:
```bash
make bench
```

### "No benchmark JSON files found"

**Symptoms:**
```
❌ No benchmark JSON files found
```

**Solution:**
1. Check if benchmarks ran successfully
2. Check `results/` directory for JSON files
3. Re-run benchmarks:
   ```bash
   make bench
   ```

### Invalid JSON output

**Symptoms:**
```
❌ Invalid JSON
```

**Solution:**
1. Validate the JSON:
   ```bash
   make validate FILE=path/to/file.json
   ```

2. Check benchmark logs in `results/suite-*/`

3. Re-run the specific benchmark

### Benchmark Execution Failures

**Symptoms:**
- Benchmarks showing `"error": "Benchmark execution failed"` with empty `benchmarks` arrays
- Commons benchmarks failing to run

**Likely Causes:**
1. **Compilation Errors**: `cargo bench` is failing to compile the benchmarks
2. **Missing Dependencies**: Path dependencies (bllvm-consensus, bllvm-node) not found
3. **Wrong Benchmark Names**: Scripts trying non-existent benchmarks
4. **Feature Flags**: `--features production` causing compilation issues

**How to Debug:**
1. **Check log files** (mentioned in JSON):
   - `/tmp/block_validation_bench.log`
   - `/tmp/commons-mempool.log`
   - `/tmp/commons-tx-validation.log`

2. **Run a benchmark manually**:
   ```bash
   cd /path/to/bllvm-bench
   cargo bench --bench block_validation_realistic
   ```

3. **Check if dependencies exist**:
   ```bash
   ls -la ../bllvm-consensus/Cargo.toml
   ls -la ../bllvm-node/Cargo.toml
   ```

4. **List available benchmarks**:
   ```bash
   cargo bench --list
   ```

**Fixes Applied:**
- Fixed `CRITERION_DIR` used before definition in `block-validation-bench.sh`
- Removed non-existent `connect_block` benchmark name
- Made scripts try without `--features production` first, then with it as fallback
- Updated benchmark names to match `Cargo.toml` exactly

## Performance Issues

### Benchmarks running very slowly

**Possible causes:**
1. Not using production mode
2. CPU throttling
3. Background processes

**Solutions:**
1. Use production mode:
   ```bash
   cargo bench --features production
   ```

2. Check CPU frequency:
   ```bash
   # Linux
   cat /proc/cpuinfo | grep MHz
   ```

3. Close unnecessary applications

### Out of memory errors

**Symptoms:**
```
error: process didn't exit successfully
```

**Solutions:**
1. Run fewer benchmarks at once
2. Use `fair-fast` suite instead of `fair`
3. Increase system swap space

## GitHub Actions Issues

### Self-hosted runner not found

**Symptoms:**
```
Error: No runners found
```

**Solution:**
1. Set up self-hosted runner (see `.github/workflows/README.md`)
2. Ensure runner is online
3. Check runner labels match workflow requirements

### Path discovery fails in CI

**Symptoms:**
```
❌ Bitcoin Core: Not found
```

**Solution:**
1. Set paths in `config/config.toml` on the runner
2. Or ensure standard paths exist:
   - `~/src/bitcoin`
   - `~/src/bllvm-consensus`

## GitHub Pages Issues

### Site not updating

**Symptoms:**
- Site shows old data
- "Loading benchmark data..." never finishes

**Solutions:**
1. Check if workflow ran successfully
2. Verify `docs/data/benchmark-results-consolidated-latest.json` exists
3. Check browser console for errors
4. Try clearing browser cache

### JSON not loading

**Symptoms:**
```
Failed to load benchmark data: HTTP 404
```

**Solutions:**
1. Check file exists: `docs/data/benchmark-results-consolidated-latest.json`
2. Verify GitHub Pages is enabled
3. Check CNAME file if using custom domain

## Getting Help

1. Check logs in `results/suite-*/`
2. Run `make check` to verify dependencies
3. Validate JSON: `make validate FILE=path/to/file.json`
4. Check GitHub Actions logs for CI issues

