# GitHub Actions Workflows

## Benchmarks Workflow

Automated benchmarking that runs on a self-hosted runner and updates the GitHub Pages site.

### Triggers

1. **Scheduled**: Daily at 2 AM UTC
2. **Manual**: Via `workflow_dispatch`
3. **On Push**: When benchmark scripts or benches change

### Improvements

The benchmarks workflow has been improved to use the same Core detection patterns as differential tests:
- Better Core path detection (checks multiple locations)
- Improved `bench_bitcoin` detection
- Better error messages and diagnostics
- Optional Rust infrastructure verification

### Requirements

- **Self-hosted runner** with:
  - Bitcoin Core built with `bench_bitcoin`
  - bllvm-consensus and bllvm-node cloned and built
  - Rust toolchain installed
  - All dependencies available

### What It Does

1. Runs benchmarks (`make bench`)
2. Generates consolidated JSON (`make json`)
3. Updates GitHub Pages data (`make update-gh-pages`)
4. Commits and pushes updated data
5. Creates a release (on scheduled runs)
6. Uploads artifacts

### Setup Self-Hosted Runner

1. Install GitHub Actions runner on your machine:
   ```bash
   mkdir actions-runner && cd actions-runner
   curl -o actions-runner-linux-x64-2.311.0.tar.gz -L https://github.com/actions/runner/releases/download/v2.311.0/actions-runner-linux-x64-2.311.0.tar.gz
   tar xzf ./actions-runner-linux-x64-2.311.0.tar.gz
   ```

2. Configure:
   ```bash
   ./config.sh --url https://github.com/BTCDecoded/bllvm-bench --token <TOKEN>
   ```

3. Install as service:
   ```bash
   sudo ./svc.sh install
   sudo ./svc.sh start
   ```

4. Ensure paths are discoverable:
   - Bitcoin Core in `~/src/bitcoin` or set in `config/config.toml`
   - bllvm-consensus in `~/src/bllvm-consensus` or set in `config/config.toml`

### Manual Workflow

Use `benchmarks-manual.yml` to run specific suites manually:
- `fair` - All fair benchmarks
- `fair-fast` - Fast benchmarks only
- `all` - All benchmarks
- `core-only` - Core benchmarks only
- `commons-only` - Commons benchmarks only

## Differential Tests Workflow

Automated differential testing that compares BLLVM validation against Bitcoin Core to catch consensus divergences.

### Triggers

1. **Scheduled**: Daily at 3 AM UTC (after benchmarks)
2. **Manual**: Via `workflow_dispatch`
3. **On Push**: When differential test code changes
4. **On Pull Request**: When differential test code changes

### What It Does

1. Checks for Bitcoin Core availability (gracefully skips if not found)
2. Runs differential tests with `--features differential`
3. Compares BLLVM vs Core validation results
4. Reports any consensus divergences
5. Uploads test results as artifacts

### Requirements

- **Self-hosted runner** with:
  - Bitcoin Core binaries (optional - tests gracefully skip if not found)
  - bllvm-consensus, bllvm-node, bllvm-protocol cloned
  - Rust toolchain installed

### Core Detection

The workflow checks for Core in multiple locations:
1. `CORE_PATH` environment variable
2. `/opt/bitcoin-core/binaries/v25.0/` (cache directory)
3. `~/bitcoin/src/` (common build location)
4. `~/src/bitcoin/` (alternative location)
5. `~/bitcoin-core/` (alternative location)

If Core is not found, tests will skip Core comparisons but still run BLLVM-only validation tests.

### Test Results

Test results are uploaded as artifacts and include:
- Test summary
- Core availability status
- Any divergence reports

See [README_DIFFERENTIAL_TESTING.md](../../README_DIFFERENTIAL_TESTING.md) for more details on differential testing.

