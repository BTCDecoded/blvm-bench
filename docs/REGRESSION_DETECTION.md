# Regression Detection & Historical Tracking

## Overview

The benchmarking system includes comprehensive regression detection and historical tracking to monitor performance over time.

## Features

### Historical Tracking
- **Time Series Data**: Stores all benchmark results with timestamps
- **Git Metadata**: Tracks git commit and branch for each run
- **Trend Analysis**: Generates trend reports showing performance changes over time

### Regression Detection
- **Baseline Comparison**: Compares current results to historical baseline
- **Statistical Significance**: Uses configurable thresholds (default: 10% slowdown)
- **Automatic Detection**: Integrated into GitHub Actions workflows
- **Detailed Reports**: JSON reports with regression details

## Usage

### Track History

After running benchmarks:

```bash
make history
```

This will:
1. Store current results in `results/history/history-*.json`
2. Update time series data in `results/history/timeseries.json`
3. Generate trend analysis in `results/history/trends-*.json`

### Detect Regressions

Compare current results to baseline:

```bash
make regressions
```

This will:
1. Compare current results to most recent baseline
2. Identify regressions (>10% slowdown by default)
3. Identify improvements (>10% speedup)
4. Generate report in `results/regression-report-*.json`
5. Update baseline if no regressions found

### Configuration

Set environment variables to customize behavior:

```bash
# Regression threshold (default: 0.10 = 10%)
export REGRESSION_THRESHOLD=0.15  # 15% slowdown

# Significance level (default: 0.05 = 5%)
export SIGNIFICANCE_LEVEL=0.01  # 1% significance

# Force baseline update
export UPDATE_BASELINE=true

# Custom history directory
export HISTORY_DIR=/path/to/history
```

## Baseline Management

### Creating Initial Baseline

The first time you run regression detection, a baseline is automatically created from current results.

### Updating Baseline

The baseline is automatically updated when:
- No regressions are detected, OR
- `UPDATE_BASELINE=true` is set

### Manual Baseline Update

```bash
# Copy current results as new baseline
cp results/benchmark-results-consolidated-*.json \
   results/history/baseline-$(date +%Y%m%d-%H%M%S).json
```

## Regression Report Format

```json
{
  "timestamp": "2025-01-20T12:00:00Z",
  "current_results": "benchmark-results-consolidated-20250120-120000.json",
  "baseline": "baseline-20250119-120000.json",
  "regression_threshold_percent": 10.0,
  "summary": {
    "regressions": 2,
    "improvements": 5,
    "unchanged": 10
  },
  "regressions": [
    {
      "benchmark": "block-validation",
      "current_time_ms": 75.5,
      "baseline_time_ms": 66.2,
      "change_percent": 14.05,
      "speedup": "0.88x",
      "status": "regression"
    }
  ],
  "improvements": [...],
  "unchanged": [...]
}
```

## Time Series Data

Time series data is stored in `results/history/timeseries.json`:

```json
{
  "benchmarks": {
    "block-validation": [
      {
        "timestamp": "2025-01-20T12:00:00Z",
        "git_commit": "abc123...",
        "core_time_ms": 66.2,
        "commons_time_ms": 10.8,
        "comparison": {...}
      },
      ...
    ]
  },
  "entries": [...]
}
```

## Trend Analysis

Trend reports show performance changes over time:

```json
[
  {
    "benchmark": "block-validation",
    "data_points": 10,
    "first_timestamp": "2025-01-10T12:00:00Z",
    "last_timestamp": "2025-01-20T12:00:00Z",
    "trends": {
      "core": {
        "first": 66.2,
        "last": 75.5,
        "change_percent": 14.05
      },
      "commons": {
        "first": 10.8,
        "last": 9.2,
        "change_percent": -14.81
      }
    }
  }
]
```

## GitHub Actions Integration

Regression detection is automatically integrated into GitHub Actions:

1. **History Tracking**: Runs after each benchmark suite
2. **Regression Detection**: Runs after history tracking
3. **Non-Blocking**: Regressions don't fail the workflow (use `continue-on-error`)
4. **Reports**: Regression reports are uploaded as artifacts

## Best Practices

1. **Regular Baselines**: Update baseline after major changes or releases
2. **Threshold Tuning**: Adjust `REGRESSION_THRESHOLD` based on benchmark variance
3. **Review Trends**: Regularly review trend analysis to identify patterns
4. **Git Integration**: Use git tags/releases to mark significant baselines
5. **Historical Data**: Keep historical data for long-term trend analysis

## Troubleshooting

### "No baseline found"

First run - baseline will be created automatically.

### "Too many regressions"

- Check if benchmark environment changed
- Verify Core/Commons versions
- Review system load during benchmarks
- Consider adjusting `REGRESSION_THRESHOLD`

### "History not updating"

- Check `results/history/` directory permissions
- Verify JSON files are valid
- Check disk space

