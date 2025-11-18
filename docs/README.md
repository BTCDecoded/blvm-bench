# GitHub Pages Site

This directory contains the static site for `benchmarks.thebitcoincommons.org`.

## Setup

1. **Enable GitHub Pages** in repository settings:
   - Go to Settings → Pages
   - Source: Deploy from a branch
   - Branch: `main` (or `gh-pages`)
   - Folder: `/docs`

2. **Update JSON path** in `index.html`:
   - Update `JSON_URL` to point to your repository's raw JSON file
   - Or use a local path if serving JSON from the same directory

3. **Deploy JSON data**:
   - Run `make json` to generate consolidated JSON
   - Copy `results/benchmark-results-consolidated-*.json` to `docs/data/`
   - Rename to `benchmark-results-consolidated-latest.json`
   - Commit and push

## Structure

```
docs/
├── index.html          # Main page (loads JSON dynamically)
├── data/               # JSON data files (optional - can use GitHub raw URLs)
│   └── benchmark-results-consolidated-latest.json
└── README.md           # This file
```

## Custom Domain

To use `benchmarks.thebitcoincommons.org`:

1. Add CNAME file in `docs/`:
   ```
   benchmarks.thebitcoincommons.org
   ```

2. Configure DNS:
   - Add CNAME record pointing to `BTCDecoded.github.io` (or your GitHub Pages URL)

## Updating Data

### Automated (Recommended)

The GitHub Actions workflow automatically runs benchmarks and updates the site:

1. **Scheduled**: Runs daily at 2 AM UTC
2. **Manual**: Trigger via Actions → Run Benchmarks → Run workflow
3. **On Push**: When benchmark code changes

The workflow:
- Runs benchmarks on self-hosted runner
- Generates consolidated JSON
- Updates `docs/data/`
- Commits and pushes automatically
- Creates releases for scheduled runs

### Manual Update

If you need to update manually:

```bash
# Generate consolidated JSON
make json

# Copy to docs/data/
make update-gh-pages

# Commit and push
git add docs/data/
git commit -m "Update benchmark data"
git push
```

## Data Sources

The site loads JSON in this priority:
1. **Local file**: `docs/data/benchmark-results-consolidated-latest.json` (from automated runs)
2. **Latest release**: JSON asset from the most recent release
3. **Main branch**: Fallback to main branch raw URL
