# GitHub Actions Workflows

## Benchmarks Workflow

Automated benchmarking that runs on a self-hosted runner and updates the GitHub Pages site.

### Triggers

1. **Scheduled**: Daily at 2 AM UTC
2. **Manual**: Via `workflow_dispatch`
3. **On Push**: When benchmark scripts or benches change

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

