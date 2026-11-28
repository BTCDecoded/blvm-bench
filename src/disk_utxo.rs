//! Disk-backed UTXO set via **heed3 / LMDB** (same engine as `blvm-node`).
//!
//! Architecture: **write-back overlay** in front of a single LMDB environment.
//!
//! - **Overlay** (`HashMap`): dirty write-back for the compare loop.
//!   `apply_block_delta` never opens an LMDB write txn. heed3 is a **checkpoint**:
//!   durable flush on Drop, SIGTERM, RSS/MemAvailable recovery, and explicit
//!   `flush_overlay(true)` only. Flushed batches stay in a generation cache
//!   (`DISK_UTXO_CACHE_CAP`, default **1M`).
//! - **Resident**: with `DISK_UTXO_WARM=1`, the newest `DISK_UTXO_HOT_MB` (default
//!   512) of coins stay in RAM. Older snapshot coins are sealed into spill once
//!   (`heed_off`); compare then does not fault the original heed file. Spend
//!   tombstones and evicted new coins also go to `heed3-spill/`. A manifest
//!   commits that overlay; the next open resumes from the manifest tip.
//! - **heed3**: snapshot source used at open (warm + one-time cold seal). Opens with
//!   `MDB_NOSYNC | MDB_NOMETASYNC | MDB_NORDAHEAD`. `data.mdb` is compacted on
//!   open when it has grown far past live data, then sequentially prefaulted.
//! - **No `MDB_WRITEMAP`**: write() pages can be dropped after fdatasync; mmap-writes cannot.
//!
//! Existing RocksDB `--disk-utxo-path` directories are copied into `{path}/heed3/` on first
//! open, then LMDB is used exclusively.

use anyhow::{Context, Result, bail};
use blvm_protocol::types::{OutPoint, UTXO, UtxoSet, utxo_set_with_capacity};
use heed3::types::Bytes;
use heed3::{Database as HeedDatabase, Env, EnvFlags, EnvOpenOptions, WithoutTls};
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use std::hash::Hasher;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::cmp::Ordering as CmpOrdering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

type ByteDb = HeedDatabase<Bytes, Bytes>;
type HeedEnv = Env<WithoutTls>;

const META_LEN_KEY: &[u8] = b"__disk_utxo_len__";
const META_TIP_KEY: &[u8] = b"__disk_utxo_tip__";
const UTRE_NAME: &str = "utxos";

#[inline]
fn is_disk_utxo_meta_key(k: &[u8]) -> bool {
    k == META_LEN_KEY || k == META_TIP_KEY
}

pub fn rehydrate_status_path(cache_root: &Path) -> PathBuf {
    std::env::var_os("CHUNK_UTXO_REHYDRATE_STATUS_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| cache_root.join(".chunk_utxo_rehydrate.status"))
}

fn write_rehydrate_progress(path: &Path, loaded: u64, total: u64, checkpoint_height: u64) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("{loaded} {total} {checkpoint_height} {ts}\n");
    let tmp = path.with_extension("rehydr.tmp");
    if fs::write(&tmp, line).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

pub fn clear_rehydrate_progress(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn compact_status_path(cache_root: &Path) -> PathBuf {
    std::env::var_os("CHUNK_UTXO_COMPACT_STATUS_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| cache_root.join(".chunk_utxo_compact.status"))
}

pub fn disk_tip_status_path(cache_root: &Path) -> PathBuf {
    std::env::var_os("CHUNK_UTXO_DISK_TIP_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| cache_root.join(".chunk_utxo_disk_tip"))
}

pub fn write_disk_tip_status_file(path: &Path, tip: u64) {
    let line = format!("{tip}\n");
    let tmp = path.with_extension("disk_tip.tmp");
    if fs::write(&tmp, line).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

fn write_compact_status(path: &Path, bytes_before: u64, start_ts: u64) {
    let line = format!("{bytes_before} {start_ts}\n");
    let tmp = path.with_extension("compact.tmp");
    if fs::write(&tmp, line).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

fn clear_compact_status(path: &Path) {
    let _ = fs::remove_file(path);
}

fn rehydrate_status_every_entries() -> u64 {
    std::env::var("DISK_UTXO_REHYDRATE_STATUS_EVERY")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(100_000)
}

fn warm_resident_enabled() -> bool {
    matches!(
        std::env::var("DISK_UTXO_WARM").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

fn overlay_cap() -> usize {
    std::env::var("DISK_UTXO_OVERLAY_CAP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000_000)
}

/// Flushed overlay generations retained for prefetch (newest first). Separate from the
/// dirty cap so LMDB write txns stay small while recently spilled coins stay in RAM.
fn cache_cap() -> usize {
    std::env::var("DISK_UTXO_CACHE_CAP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000_000)
}

/// Newest coins kept in RAM. The rest stay in the read-only heed file.
/// Cold inputs of one block do not depend on each other. Their page reads run
/// together, up to this many at once.
fn spill_read_threads() -> usize {
    std::env::var("DISK_UTXO_PREFETCH_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8)
        .clamp(1, 32)
}

fn par_map<T: Send>(
    n: usize,
    threads: usize,
    f: &(impl Fn(usize) -> Result<T> + Sync),
) -> Result<Vec<T>> {
    if n == 0 {
        return Ok(Vec::new());
    }
    let threads = threads.min(n).max(1);
    let mut parts = Vec::with_capacity(threads);
    std::thread::scope(|scope| -> Result<()> {
        let mut joins = Vec::with_capacity(threads);
        let chunk = n.div_ceil(threads);
        for t in 0..threads {
            let start = t * chunk;
            if start >= n {
                break;
            }
            let end = (start + chunk).min(n);
            joins.push(scope.spawn(move || -> Result<Vec<T>> {
                let mut out = Vec::with_capacity(end - start);
                for i in start..end {
                    out.push(f(i)?);
                }
                Ok(out)
            }));
        }
        for join in joins {
            parts.push(
                join.join()
                    .map_err(|_| anyhow::anyhow!("cold lookup thread panicked"))??,
            );
        }
        Ok(())
    })?;
    let mut out = Vec::with_capacity(n);
    for part in parts {
        out.extend(part);
    }
    Ok(out)
}

fn hot_coin_cap() -> usize {
    // Default 512 MiB keeps the slab resident on a 16 GB host next to Core.
    // A larger slab that sits in swap is slower than a smaller one plus sealed spill.
    let mb = std::env::var("DISK_UTXO_HOT_MB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n >= 64)
        .unwrap_or(512);
    let bytes = mb.saturating_mul(1024).saturating_mul(1024);
    (bytes / std::mem::size_of::<CoinRec>()).max(10_000)
}

/// Spill-db value meaning "spent, do not fall through to the original heed file".
const TOMB_REC: usize = 36;
/// Records per tomb page. 113 * 36 = 4068, which fits in one 4 KB page.
const TOMB_STRIDE: u64 = 113;
/// Records per coin-index page. 85 * 48 = 4080.
const COIN_STRIDE: u64 = 85;
/// Page reads performed by the sealed-run lookup. Tests use this to prove a hit is one read.
static PAGE_PROBES: AtomicU64 = AtomicU64::new(0);
static TOMB_PAGE_PROBES: AtomicU64 = AtomicU64::new(0);
static COIN_PAGE_PROBES: AtomicU64 = AtomicU64::new(0);
static BODY_READS: AtomicU64 = AtomicU64::new(0);
static BODY_NS: AtomicU64 = AtomicU64::new(0);

/// Prefetch counters for one progress window. Reset when the harness takes them.
#[derive(Clone, Debug, Default)]
pub struct DiskUtxoWindowStats {
    pub ov: u64,
    pub cache: u64,
    pub res: u64,
    pub db: u64,
    pub db_hit: u64,
    pub db_ms: f64,
    /// Wall time inside `prefetch` (includes cold spill / heed).
    pub prefetch_ms: f64,
    /// Wall time inside cold spill resolution only.
    pub cold_ms: f64,
    /// `coins.bin` body reads after a coin-index hit.
    pub body_reads: u64,
    pub body_ms: f64,
    pub tomb_pages: u64,
    pub coin_pages: u64,
    pub hot_len: u64,
    pub spill_script_bytes: u64,
    /// `spill.capacity()` — catches Vec high-water that `spill_script_bytes` hides.
    pub spill_cap: u64,
    pub tomb_runs: u64,
    pub coin_runs: u64,
    pub heed_off: bool,
}

struct TombSeal {
    file: File,
    count: u64,
    /// First key of every [`TOMB_STRIDE`] records.
    index: Vec<[u8; TOMB_REC]>,
    bloom: Vec<u64>,
}

#[derive(Clone)]
struct TombRun {
    path: PathBuf,
    seal: Arc<TombSeal>,
}

impl TombRun {
    fn count(&self) -> u64 {
        self.seal.count
    }
}

fn tomb_bloom_words() -> usize {
    let mb = std::env::var("DISK_UTXO_TOMB_BLOOM_MB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(128)
        .clamp(32, 512);
    mb * 1024 * 1024 / 8
}

fn tomb_hashes(key: &[u8; TOMB_REC]) -> (u64, u64) {
    let mut h = FxHasher::default();
    h.write(key);
    let a = h.finish();
    let mut h2 = FxHasher::default();
    h2.write_u64(a ^ 0x9e37_79b9_7f4a_7c15);
    h2.write(key);
    (a, h2.finish() | 1)
}

fn bloom_insert(bits: &mut [u64], key: &[u8; TOMB_REC]) {
    let (h1, h2) = tomb_hashes(key);
    let nbits = bits.len() * 64;
    for i in 0..4u64 {
        let bit = h1.wrapping_add(i.wrapping_mul(h2)) as usize % nbits;
        bits[bit / 64] |= 1u64 << (bit % 64);
    }
}

fn bloom_maybe(bits: &[u64], key: &[u8; TOMB_REC]) -> bool {
    let (h1, h2) = tomb_hashes(key);
    let nbits = bits.len() * 64;
    (0..4u64).all(|i| {
        let bit = h1.wrapping_add(i.wrapping_mul(h2)) as usize % nbits;
        bits[bit / 64] & (1u64 << (bit % 64)) != 0
    })
}

/// Random probes must not grow the kernel readahead window. A 36-byte read
/// otherwise pulls about a megabyte, and that cache is what gets the hot coin
/// table swapped out.
fn advise_random(file: &File) {
    unsafe {
        libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_RANDOM);
    }
}

/// Sealed spill read. No `DONTNEED`: tip compare re-reads the same pages, and
/// ejecting them forced major-fault storms. Files stay `FADV_RANDOM` so
/// readahead does not pin megabytes.
fn read_at(file: &File, buf: &mut [u8], at: u64) -> std::io::Result<()> {
    file.read_exact_at(buf, at)
}

/// 16 bits/key. Do not cap: a 32 MiB ceiling on ~250M-key runs raised FPR to
/// ~0.9 and exploded `tomb_pg` / majflt at tip.
fn tier_bloom_words(n: u64) -> usize {
    let bits = n.saturating_mul(16).max(64);
    ((bits + 63) / 64) as usize
}

fn tier_bloom(n: u64) -> Vec<u64> {
    vec![0u64; tier_bloom_words(n)]
}

/// Size tier. Boundaries grow by 4 from a 2 MB base, and a tier holds one run.
fn size_tier(bytes: u64) -> u32 {
    const BASE: u64 = 2 * 1024 * 1024;
    if bytes < BASE {
        return 0;
    }
    let mut tier = 0u32;
    let mut hi = BASE;
    while tier < 16 && hi.saturating_mul(4) <= bytes {
        hi = hi.saturating_mul(4);
        tier += 1;
    }
    tier
}

fn page_slot(index: &[[u8; TOMB_REC]], key: &[u8; TOMB_REC]) -> Option<usize> {
    if index.is_empty() || key.as_slice() < index[0].as_slice() {
        return None;
    }
    let mut lo = 0usize;
    let mut hi = index.len();
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        if index[mid].as_slice() <= key.as_slice() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(lo)
}

fn tomb_page_has(run: &TombRun, key: &[u8; TOMB_REC]) -> Result<bool> {
    let seal = &run.seal;
    if seal.count > 0 && seal.index.is_empty() {
        bail!("sealed tomb {} has no page index", run.path.display());
    }
    let Some(slot) = page_slot(&seal.index, key) else {
        return Ok(false);
    };
    let start = slot as u64 * TOMB_STRIDE;
    let n = TOMB_STRIDE.min(seal.count - start);
    let mut buf = vec![0u8; n as usize * TOMB_REC];
    PAGE_PROBES.fetch_add(1, Ordering::Relaxed);
    TOMB_PAGE_PROBES.fetch_add(1, Ordering::Relaxed);
    read_at(&seal.file, &mut buf, start * TOMB_REC as u64)
        .with_context(|| format!("read {}", run.path.display()))?;
    Ok(buf.chunks_exact(TOMB_REC).any(|rec| rec == key.as_slice()))
}

fn note_indexed_key(index: &mut Vec<[u8; TOMB_REC]>, n: u64, stride: u64, key: &[u8; TOMB_REC]) {
    if n % stride == 0 {
        index.push(*key);
    }
}

fn read_tomb_rec(r: &mut impl Read) -> Result<Option<[u8; TOMB_REC]>> {
    let mut buf = [0u8; TOMB_REC];
    let mut got = 0;
    while got < TOMB_REC {
        match r.read(&mut buf[got..])? {
            0 if got == 0 => return Ok(None),
            0 => bail!("short tombstone record"),
            n => got += n,
        }
    }
    Ok(Some(buf))
}

fn merge_tomb_runs(dir: &Path, id: u32, a_path: &Path, b_path: &Path) -> Result<TombRun> {
    use std::io::BufReader;
    let path = dir.join(format!("tombs-{id}.bin"));
    let mut out = File::create(&path).with_context(|| format!("create {}", path.display()))?;
    let mut ra = BufReader::new(File::open(a_path).with_context(|| format!("open {}", a_path.display()))?);
    let mut rb = BufReader::new(File::open(b_path).with_context(|| format!("open {}", b_path.display()))?);
    let mut left = read_tomb_rec(&mut ra)?;
    let mut right = read_tomb_rec(&mut rb)?;
    let mut count = 0u64;
    let mut prev: Option<[u8; TOMB_REC]> = None;
    loop {
        let pick = match (&left, &right) {
            (None, None) => break,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(l), Some(r)) => l.as_slice() <= r.as_slice(),
        };
        let rec = if pick { left.take().unwrap() } else { right.take().unwrap() };
        if prev.as_ref() != Some(&rec) {
            out.write_all(&rec)?;
            count += 1;
            prev = Some(rec);
        }
        if pick {
            left = read_tomb_rec(&mut ra)?;
        } else {
            right = read_tomb_rec(&mut rb)?;
        }
    }
    out.flush()?;
    out.sync_all().ok();
    let file = File::open(&path)?;
    seal_tomb_run(path, file, count, None)
}

fn seal_tomb_run(
    path: PathBuf,
    mut file: File,
    count: u64,
    mut global: Option<&mut [u64]>,
) -> Result<TombRun> {
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0))?;
    let mut index = Vec::new();
    let mut bloom = tier_bloom(count.max(1));
    let mut buf = [0u8; TOMB_REC];
    for n in 0..count {
        file.read_exact(&mut buf)
            .with_context(|| format!("seal {}", path.display()))?;
        note_indexed_key(&mut index, n, TOMB_STRIDE, &buf);
        bloom_insert(&mut bloom, &buf);
        if let Some(bits) = global.as_mut() {
            bloom_insert(bits, &buf);
        }
    }
    advise_random(&file);
    Ok(TombRun {
        path,
        seal: Arc::new(TombSeal {
            file,
            count,
            index,
            bloom,
        }),
    })
}

/// Sorted index record: 36-byte outpoint key, u64 offset, u32 length.
const COIN_IX: usize = 48;

struct CoinSeal {
    file: File,
    count: u64,
    index: Vec<[u8; TOMB_REC]>,
    bloom: Vec<u64>,
}

#[derive(Clone)]
struct CoinRun {
    path: PathBuf,
    seal: Arc<CoinSeal>,
}

impl CoinRun {
    fn count(&self) -> u64 {
        self.seal.count
    }
}

fn coin_ix_off(rec: &[u8; COIN_IX]) -> u64 {
    u64::from_le_bytes(rec[36..44].try_into().unwrap())
}

fn coin_ix_len(rec: &[u8; COIN_IX]) -> u32 {
    u32::from_le_bytes(rec[44..48].try_into().unwrap())
}

fn read_coin_ix(r: &mut impl Read) -> Result<Option<[u8; COIN_IX]>> {
    let mut buf = [0u8; COIN_IX];
    let mut got = 0;
    while got < COIN_IX {
        match r.read(&mut buf[got..])? {
            0 if got == 0 => return Ok(None),
            0 => bail!("short coin index record"),
            n => got += n,
        }
    }
    Ok(Some(buf))
}

fn coin_page_find(run: &CoinRun, key: &[u8; TOMB_REC]) -> Result<Option<(u64, u32)>> {
    let seal = &run.seal;
    if seal.count > 0 && seal.index.is_empty() {
        bail!("sealed coin index {} has no page index", run.path.display());
    }
    let Some(slot) = page_slot(&seal.index, key) else {
        return Ok(None);
    };
    let start = slot as u64 * COIN_STRIDE;
    let n = COIN_STRIDE.min(seal.count - start);
    let mut buf = vec![0u8; n as usize * COIN_IX];
    PAGE_PROBES.fetch_add(1, Ordering::Relaxed);
    COIN_PAGE_PROBES.fetch_add(1, Ordering::Relaxed);
    read_at(&seal.file, &mut buf, start * COIN_IX as u64)
        .with_context(|| format!("read {}", run.path.display()))?;
    for rec in buf.chunks_exact(COIN_IX) {
        match rec[..TOMB_REC].cmp(key.as_slice()) {
            CmpOrdering::Equal => {
                let off = u64::from_le_bytes(rec[36..44].try_into().unwrap());
                let len = u32::from_le_bytes(rec[44..48].try_into().unwrap());
                return Ok(Some((off, len)));
            }
            CmpOrdering::Greater => return Ok(None),
            CmpOrdering::Less => {}
        }
    }
    Ok(None)
}

fn seal_coin_run(
    path: PathBuf,
    mut file: File,
    count: u64,
    mut global: Option<&mut [u64]>,
) -> Result<CoinRun> {
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0))?;
    let mut index = Vec::new();
    let mut bloom = tier_bloom(count.max(1));
    let mut buf = [0u8; COIN_IX];
    for n in 0..count {
        file.read_exact(&mut buf)
            .with_context(|| format!("seal {}", path.display()))?;
        let mut key = [0u8; TOMB_REC];
        key.copy_from_slice(&buf[..TOMB_REC]);
        note_indexed_key(&mut index, n, COIN_STRIDE, &key);
        bloom_insert(&mut bloom, &key);
        if let Some(bits) = global.as_mut() {
            bloom_insert(bits, &key);
        }
    }
    advise_random(&file);
    Ok(CoinRun {
        path,
        seal: Arc::new(CoinSeal {
            file,
            count,
            index,
            bloom,
        }),
    })
}

fn merge_coin_runs(dir: &Path, id: u32, a_path: &Path, b_path: &Path) -> Result<CoinRun> {
    use std::io::BufReader;
    let path = dir.join(format!("coins-{id}.idx"));
    let mut out = File::create(&path).with_context(|| format!("create {}", path.display()))?;
    let mut ra = BufReader::new(File::open(a_path).with_context(|| format!("open {}", a_path.display()))?);
    let mut rb = BufReader::new(File::open(b_path).with_context(|| format!("open {}", b_path.display()))?);
    let mut left = read_coin_ix(&mut ra)?;
    let mut right = read_coin_ix(&mut rb)?;
    let mut count = 0u64;
    let mut prev_key: Option<[u8; TOMB_REC]> = None;
    loop {
        let pick_left = match (&left, &right) {
            (None, None) => break,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(l), Some(r)) => match l[..TOMB_REC].cmp(&r[..TOMB_REC]) {
                std::cmp::Ordering::Less => true,
                std::cmp::Ordering::Greater => false,
                std::cmp::Ordering::Equal => coin_ix_off(l) >= coin_ix_off(r),
            },
        };
        let rec = if pick_left {
            left.take().unwrap()
        } else {
            right.take().unwrap()
        };
        let mut key = [0u8; TOMB_REC];
        key.copy_from_slice(&rec[..TOMB_REC]);
        if prev_key.as_ref() != Some(&key) {
            out.write_all(&rec)?;
            count += 1;
            prev_key = Some(key);
        }
        if pick_left {
            left = read_coin_ix(&mut ra)?;
        } else {
            right = read_coin_ix(&mut rb)?;
        }
    }
    out.flush()?;
    out.sync_all().ok();
    let file = File::open(&path)?;
    seal_coin_run(path, file, count, None)
}

fn overlay_checkpoint_every() -> u64 {
    std::env::var("DISK_UTXO_OVERLAY_CHECKPOINT_EVERY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000)
}

/// Same interval as the spill manifest. The harness saves header files on this
/// cadence so a resume does not re-read the block chunks for the difficulty window.
pub fn overlay_resume_checkpoint_due(height: u64) -> bool {
    let every = overlay_checkpoint_every();
    every > 0 && height > 0 && height % every == 0
}

struct RunSet {
    tombs: Vec<TombRun>,
    coins: Vec<CoinRun>,
    next_tomb: u32,
    next_coin: u32,
    /// Merged-away files. Unlinked only after a manifest no longer names them.
    retired: Vec<PathBuf>,
    pause: bool,
    stop: bool,
    busy: bool,
}

impl RunSet {
    fn fresh() -> Self {
        Self {
            tombs: Vec::new(),
            coins: Vec::new(),
            next_tomb: 0,
            next_coin: 0,
            retired: Vec::new(),
            pause: false,
            stop: false,
            busy: false,
        }
    }
}

type CompactShared = Arc<(Mutex<RunSet>, Condvar)>;

fn tomb_tier_pair(runs: &RunSet) -> Option<(PathBuf, PathBuf)> {
    tier_pair(runs.tombs.len(), |i| {
        (runs.tombs[i].count().saturating_mul(TOMB_REC as u64), runs.tombs[i].path.clone())
    })
}

fn coin_tier_pair(runs: &RunSet) -> Option<(PathBuf, PathBuf)> {
    tier_pair(runs.coins.len(), |i| {
        (runs.coins[i].count().saturating_mul(COIN_IX as u64), runs.coins[i].path.clone())
    })
}

/// Two oldest runs that share a size tier. Write order is not size order.
fn tier_pair(n: usize, at: impl Fn(usize) -> (u64, PathBuf)) -> Option<(PathBuf, PathBuf)> {
    let mut best: Option<(u32, usize, usize)> = None;
    for i in 0..n {
        for j in (i + 1)..n {
            let (bi, _) = at(i);
            let (bj, _) = at(j);
            let ti = size_tier(bi);
            if ti != size_tier(bj) {
                continue;
            }
            match best {
                Some((tier, _, _)) if tier <= ti => {}
                _ => best = Some((ti, i, j)),
            }
        }
    }
    let (_, i, j) = best?;
    Some((at(i).1, at(j).1))
}

fn install_tomb(runs: &mut RunSet, a: &Path, b: &Path, merged: TombRun) -> bool {
    install_run(&mut runs.tombs, &mut runs.retired, a, b, merged, |r| &r.path)
}

fn install_coin(runs: &mut RunSet, a: &Path, b: &Path, merged: CoinRun) -> bool {
    install_run(&mut runs.coins, &mut runs.retired, a, b, merged, |r| &r.path)
}

fn install_run<T>(
    runs: &mut Vec<T>,
    retired: &mut Vec<PathBuf>,
    a: &Path,
    b: &Path,
    merged: T,
    path_of: impl Fn(&T) -> &Path,
) -> bool {
    let Some(ia) = runs.iter().position(|r| path_of(r) == a) else {
        return false;
    };
    let Some(ib) = runs.iter().position(|r| path_of(r) == b) else {
        return false;
    };
    if ia == ib {
        return false;
    }
    let newer = ia.max(ib);
    let (hi, lo) = if ia > ib { (ia, ib) } else { (ib, ia) };
    let high = runs.remove(hi);
    let low = runs.remove(lo);
    retired.push(path_of(&low).to_path_buf());
    retired.push(path_of(&high).to_path_buf());
    runs.insert(newer - 1, merged);
    true
}

enum ColdDisk {
    Spent,
    Body(Vec<u8>),
    Miss,
}

#[derive(Debug)]
enum SpillLookup {
    Spent,
    Coin(u64, u32),
    Miss,
}

/// Tomb runs are finished before any coin run. Coin runs are newest first,
/// and the first hit wins. A per-tier bloom miss does not touch that file.
fn spill_lookup(tombs: &[TombRun], coins: &[CoinRun], key: &[u8; TOMB_REC]) -> Result<SpillLookup> {
    for run in tombs.iter().rev() {
        if bloom_maybe(&run.seal.bloom, key) && tomb_page_has(run, key)? {
            return Ok(SpillLookup::Spent);
        }
    }
    for run in coins.iter().rev() {
        if bloom_maybe(&run.seal.bloom, key) {
            if let Some((off, len)) = coin_page_find(run, key)? {
                return Ok(SpillLookup::Coin(off, len));
            }
        }
    }
    Ok(SpillLookup::Miss)
}

enum CompactJob {
    Tomb { id: u32, a: PathBuf, b: PathBuf },
    Coin { id: u32, a: PathBuf, b: PathBuf },
}

/// Clears `busy` if a merge panics, so a checkpoint cannot wait forever.
struct ClearBusy<'a>(&'a CompactShared);

impl Drop for ClearBusy<'_> {
    fn drop(&mut self) {
        let (lock, cv) = &**self.0;
        if let Ok(mut guard) = lock.lock() {
            guard.busy = false;
            cv.notify_all();
        }
    }
}

fn compact_loop(shared: CompactShared, dir: PathBuf) {
    loop {
        let job = {
            let (lock, cv) = &*shared;
            let mut guard = lock.lock().unwrap();
            loop {
                if guard.stop {
                    return;
                }
                if !guard.pause {
                    if let Some((a, b)) = tomb_tier_pair(&guard) {
                        let id = guard.next_tomb;
                        guard.next_tomb = guard.next_tomb.saturating_add(1);
                        guard.busy = true;
                        break CompactJob::Tomb { id, a, b };
                    }
                    if let Some((a, b)) = coin_tier_pair(&guard) {
                        let id = guard.next_coin;
                        guard.next_coin = guard.next_coin.saturating_add(1);
                        guard.busy = true;
                        break CompactJob::Coin { id, a, b };
                    }
                }
                guard = cv.wait(guard).unwrap();
            }
        };
        let installed = {
            let _clear = ClearBusy(&shared);
            match &job {
            CompactJob::Tomb { id, a, b } => match merge_tomb_runs(&dir, *id, a, b) {
                Ok(merged) => {
                    let (lock, cv) = &*shared;
                    let mut guard = lock.lock().unwrap();
                    let ok = install_tomb(&mut guard, a, b, merged);
                    if !ok {
                        let _ = fs::remove_file(dir.join(format!("tombs-{id}.bin")));
                    }
                    guard.busy = false;
                    cv.notify_all();
                    ok
                }
                Err(e) => {
                    eprintln!("   [disk-utxo] tomb merge failed: {e:#}");
                    let (lock, cv) = &*shared;
                    let mut guard = lock.lock().unwrap();
                    guard.busy = false;
                    cv.notify_all();
                    false
                }
            },
            CompactJob::Coin { id, a, b } => match merge_coin_runs(&dir, *id, a, b) {
                Ok(merged) => {
                    let (lock, cv) = &*shared;
                    let mut guard = lock.lock().unwrap();
                    let ok = install_coin(&mut guard, a, b, merged);
                    if !ok {
                        let _ = fs::remove_file(dir.join(format!("coins-{id}.idx")));
                    }
                    guard.busy = false;
                    cv.notify_all();
                    ok
                }
                Err(e) => {
                    eprintln!("   [disk-utxo] coin-index merge failed: {e:#}");
                    let (lock, cv) = &*shared;
                    let mut guard = lock.lock().unwrap();
                    guard.busy = false;
                    cv.notify_all();
                    false
                }
            },
            }
        };
        if !installed {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
}

struct OverlayManifest {
    snapshot_tip: u64,
    tip: u64,
    len: u64,
    coins_len: u64,
    next_tomb: u32,
    next_coin: u32,
    /// Hot coins created after the snapshot. `None` means that set is empty.
    resident: Option<String>,
    /// Cold snapshot coins were copied into sealed spill; compare does not fault heed.
    heed_off: bool,
    /// Every snapshot coin is in sealed spill (including those that used to live
    /// only in the hot slab). Skip `warm_resident` on open when this is set with
    /// [`Self::heed_off`].
    seal_complete: bool,
    tombs: Vec<(String, u64)>,
    coins: Vec<(String, u64)>,
}

fn format_manifest(m: &OverlayManifest) -> String {
    let mut out = format!(
        "OVERLAY 1\nsnapshot {}\ntip {}\nlen {}\ncoins_len {}\nnext_tomb {}\nnext_coin {}\n",
        m.snapshot_tip, m.tip, m.len, m.coins_len, m.next_tomb, m.next_coin
    );
    if m.heed_off {
        out.push_str("heed_off 1\n");
    }
    if m.seal_complete {
        out.push_str("seal_complete 1\n");
    }
    if let Some(name) = &m.resident {
        out.push_str(&format!("resident {name}\n"));
    }
    for (name, count) in &m.tombs {
        out.push_str(&format!("tomb {name} {count}\n"));
    }
    for (name, count) in &m.coins {
        out.push_str(&format!("coin {name} {count}\n"));
    }
    out
}

fn spill_file_name(name: &str) -> Result<String> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.starts_with('.')
    {
        bail!("spill manifest names a file outside the spill directory");
    }
    Ok(name.to_string())
}

fn parse_manifest(text: &str) -> Result<OverlayManifest> {
    let mut lines = text.lines();
    if lines.next() != Some("OVERLAY 1") {
        bail!("spill manifest version is not OVERLAY 1");
    }
    let mut snapshot_tip = None;
    let mut tip = None;
    let mut len = None;
    let mut coins_len = None;
    let mut next_tomb = None;
    let mut next_coin = None;
    let mut resident = None;
    let mut heed_off = false;
    let mut seal_complete = false;
    let mut tombs = Vec::new();
    let mut coins = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let kind = parts.next().context("manifest field")?;
        match kind {
            "snapshot" => snapshot_tip = Some(parts.next().context("snapshot")?.parse()?),
            "tip" => tip = Some(parts.next().context("tip")?.parse()?),
            "len" => len = Some(parts.next().context("len")?.parse()?),
            "coins_len" => coins_len = Some(parts.next().context("coins_len")?.parse()?),
            "next_tomb" => next_tomb = Some(parts.next().context("next_tomb")?.parse()?),
            "next_coin" => next_coin = Some(parts.next().context("next_coin")?.parse()?),
            "heed_off" => {
                let v = parts.next().unwrap_or("1");
                heed_off = v == "1" || v.eq_ignore_ascii_case("true");
            }
            "seal_complete" => {
                let v = parts.next().unwrap_or("1");
                seal_complete = v == "1" || v.eq_ignore_ascii_case("true");
            }
            "resident" => {
                if resident.is_some() {
                    bail!("spill manifest names two hot-coin files");
                }
                resident = Some(spill_file_name(parts.next().context("resident name")?)?);
            }
            "tomb" => tombs.push((
                spill_file_name(parts.next().context("tomb name")?)?,
                parts.next().context("tomb count")?.parse()?,
            )),
            "coin" => coins.push((
                spill_file_name(parts.next().context("coin name")?)?,
                parts.next().context("coin count")?.parse()?,
            )),
            other => bail!("unknown manifest field {other}"),
        }
    }
    Ok(OverlayManifest {
        snapshot_tip: snapshot_tip.context("manifest snapshot")?,
        tip: tip.context("manifest tip")?,
        len: len.context("manifest len")?,
        coins_len: coins_len.context("manifest coins_len")?,
        next_tomb: next_tomb.context("manifest next_tomb")?,
        next_coin: next_coin.context("manifest next_coin")?,
        resident,
        heed_off,
        seal_complete,
        tombs,
        coins,
    })
}

fn read_exact_or_eof(r: &mut impl Read, buf: &mut [u8]) -> Result<bool> {
    let mut got = 0;
    while got < buf.len() {
        match r.read(&mut buf[got..])? {
            0 if got == 0 => return Ok(false),
            0 => bail!("short spill record"),
            n => got += n,
        }
    }
    Ok(true)
}

fn open_counted_file(path: &Path, count: u64, record: u64) -> Result<File> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let meta = file.metadata().with_context(|| format!("stat {}", path.display()))?;
    let expect = count.saturating_mul(record);
    if meta.len() != expect {
        bail!(
            "{} is {} bytes, manifest expects {expect}",
            path.display(),
            meta.len()
        );
    }
    Ok(file)
}

struct LoadedOverlay {
    tip: u64,
    len: u64,
    coins_len: u64,
    resident: Option<String>,
    heed_off: bool,
    seal_complete: bool,
    runs: RunSet,
    tomb_bloom: Vec<u64>,
    coin_bloom: Vec<u64>,
}

struct HeapKey {
    key: [u8; TOMB_REC],
    offset: u64,
    src: usize,
    coin: Option<[u8; COIN_IX]>,
}

impl PartialEq for HeapKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == CmpOrdering::Equal
    }
}
impl Eq for HeapKey {}
impl PartialOrd for HeapKey {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
impl Ord for HeapKey {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        other
            .key
            .as_slice()
            .cmp(self.key.as_slice())
            .then(self.offset.cmp(&other.offset))
    }
}

fn kmerge_tombs(dir: &Path, id: u32, inputs: &[TombRun]) -> Result<TombRun> {
    use std::io::BufReader;
    let path = dir.join(format!("tombs-{id}.bin"));
    let mut out = File::create(&path).with_context(|| format!("create {}", path.display()))?;
    let mut readers: Vec<BufReader<File>> = Vec::with_capacity(inputs.len());
    let mut heap = BinaryHeap::new();
    for (src, run) in inputs.iter().enumerate() {
        let mut cloned = run.seal.file.try_clone()?;
        use std::io::{Seek, SeekFrom};
        cloned.seek(SeekFrom::Start(0))?;
        let mut reader = BufReader::new(cloned);
        if let Some(rec) = read_tomb_rec(&mut reader)? {
            heap.push(HeapKey {
                key: rec,
                offset: 0,
                src,
                coin: None,
            });
        }
        readers.push(reader);
    }
    let mut index = Vec::new();
    let bound = inputs.iter().map(|r| r.count()).sum::<u64>().max(1);
    let mut bloom = tier_bloom(bound);
    let mut count = 0u64;
    let mut prev: Option<[u8; TOMB_REC]> = None;
    while let Some(item) = heap.pop() {
        if prev.as_ref() != Some(&item.key) {
            out.write_all(&item.key)?;
            note_indexed_key(&mut index, count, TOMB_STRIDE, &item.key);
            bloom_insert(&mut bloom, &item.key);
            count += 1;
            prev = Some(item.key);
        }
        if let Some(rec) = read_tomb_rec(&mut readers[item.src])? {
            heap.push(HeapKey {
                key: rec,
                offset: 0,
                src: item.src,
                coin: None,
            });
        }
    }
    out.flush()?;
    out.sync_all().ok();
    let file = File::open(&path)?;
    advise_random(&file);
    Ok(TombRun {
        path,
        seal: Arc::new(TombSeal {
            file,
            count,
            index,
            bloom,
        }),
    })
}

fn kmerge_coins(dir: &Path, id: u32, inputs: &[CoinRun]) -> Result<CoinRun> {
    use std::io::BufReader;
    let path = dir.join(format!("coins-{id}.idx"));
    let mut out = File::create(&path).with_context(|| format!("create {}", path.display()))?;
    let mut readers: Vec<BufReader<File>> = Vec::with_capacity(inputs.len());
    let mut heap = BinaryHeap::new();
    for (src, run) in inputs.iter().enumerate() {
        let mut cloned = run.seal.file.try_clone()?;
        use std::io::{Seek, SeekFrom};
        cloned.seek(SeekFrom::Start(0))?;
        let mut reader = BufReader::new(cloned);
        if let Some(rec) = read_coin_ix(&mut reader)? {
            let mut key = [0u8; TOMB_REC];
            key.copy_from_slice(&rec[..TOMB_REC]);
            heap.push(HeapKey {
                key,
                offset: coin_ix_off(&rec),
                src,
                coin: Some(rec),
            });
        }
        readers.push(reader);
    }
    let mut index = Vec::new();
    let bound = inputs.iter().map(|r| r.count()).sum::<u64>().max(1);
    let mut bloom = tier_bloom(bound);
    let mut count = 0u64;
    let mut prev: Option<[u8; TOMB_REC]> = None;
    while let Some(item) = heap.pop() {
        if prev.as_ref() != Some(&item.key) {
            let rec = item.coin.context("coin merge lost the index record")?;
            out.write_all(&rec)?;
            note_indexed_key(&mut index, count, COIN_STRIDE, &item.key);
            bloom_insert(&mut bloom, &item.key);
            count += 1;
            prev = Some(item.key);
        }
        if let Some(rec) = read_coin_ix(&mut readers[item.src])? {
            let mut key = [0u8; TOMB_REC];
            key.copy_from_slice(&rec[..TOMB_REC]);
            heap.push(HeapKey {
                key,
                offset: coin_ix_off(&rec),
                src: item.src,
                coin: Some(rec),
            });
        }
    }
    out.flush()?;
    out.sync_all().ok();
    let file = File::open(&path)?;
    advise_random(&file);
    Ok(CoinRun {
        path,
        seal: Arc::new(CoinSeal {
            file,
            count,
            index,
            bloom,
        }),
    })
}

fn write_manifest_file(dir: &Path, manifest: &OverlayManifest) -> Result<()> {
    let tmp = dir.join("MANIFEST.tmp");
    {
        let mut file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        file.write_all(format_manifest(manifest).as_bytes())?;
        file.sync_all()?;
    }
    let dest = dir.join("MANIFEST");
    fs::rename(&tmp, &dest).with_context(|| format!("rename {}", dest.display()))?;
    if let Ok(dir_file) = File::open(dir) {
        dir_file.sync_all().ok();
    }
    Ok(())
}

/// One k-way merge per kind when a manifest still names many runs. Replaced
/// files stay on disk until the manifest that omits them has been renamed
/// into place; the next open deletes anything that manifest does not name.
fn collapse_sealed_runs(dir: &Path, loaded: &mut LoadedOverlay, snapshot_tip: u64) -> Result<()> {
    if loaded.runs.tombs.len() <= 1 && loaded.runs.coins.len() <= 1 {
        return Ok(());
    }
    let old_tombs: Vec<PathBuf> = loaded.runs.tombs.iter().map(|r| r.path.clone()).collect();
    let old_coins: Vec<PathBuf> = loaded.runs.coins.iter().map(|r| r.path.clone()).collect();
    if loaded.runs.tombs.len() > 1 {
        let id = loaded.runs.next_tomb;
        loaded.runs.next_tomb = loaded.runs.next_tomb.saturating_add(1);
        let merged = kmerge_tombs(dir, id, &loaded.runs.tombs)?;
        loaded.runs.tombs = vec![merged];
    }
    if loaded.runs.coins.len() > 1 {
        let id = loaded.runs.next_coin;
        loaded.runs.next_coin = loaded.runs.next_coin.saturating_add(1);
        let merged = kmerge_coins(dir, id, &loaded.runs.coins)?;
        loaded.runs.coins = vec![merged];
    }
    let manifest = OverlayManifest {
        snapshot_tip,
        tip: loaded.tip,
        len: loaded.len,
        coins_len: loaded.coins_len,
        next_tomb: loaded.runs.next_tomb,
        next_coin: loaded.runs.next_coin,
        resident: loaded.resident.clone(),
        heed_off: loaded.heed_off,
        seal_complete: loaded.seal_complete,
        tombs: loaded
            .runs
            .tombs
            .iter()
            .map(|r| (run_file_name(&r.path), r.count()))
            .collect(),
        coins: loaded
            .runs
            .coins
            .iter()
            .map(|r| (run_file_name(&r.path), r.count()))
            .collect(),
    };
    write_manifest_file(dir, &manifest)?;
    let keep: std::collections::HashSet<String> = manifest
        .tombs
        .iter()
        .map(|(n, _)| n.clone())
        .chain(manifest.coins.iter().map(|(n, _)| n.clone()))
        .collect();
    for path in old_tombs.into_iter().chain(old_coins) {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if !keep.contains(name) {
                let _ = fs::remove_file(path);
            }
        }
    }
    Ok(())
}

fn load_overlay(dir: &Path, snapshot_tip: u64) -> Result<Option<LoadedOverlay>> {
    let manifest_path = dir.join("MANIFEST");
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let manifest = parse_manifest(&text)?;
    if manifest.snapshot_tip != snapshot_tip {
        eprintln!(
            "   [disk-utxo] spill manifest snapshot {} != heed tip {snapshot_tip}",
            manifest.snapshot_tip
        );
        return Ok(None);
    }
    let mut tomb_bloom = vec![0u64; tomb_bloom_words()];
    let mut tombs = Vec::with_capacity(manifest.tombs.len());
    for (name, count) in &manifest.tombs {
        let path = dir.join(name);
        let file = open_counted_file(&path, *count, TOMB_REC as u64)?;
        tombs.push(seal_tomb_run(path, file, *count, Some(&mut tomb_bloom))?);
    }
    let mut coin_bloom = vec![0u64; tomb_bloom_words()];
    let mut coins = Vec::with_capacity(manifest.coins.len());
    for (name, count) in &manifest.coins {
        let path = dir.join(name);
        let file = open_counted_file(&path, *count, COIN_IX as u64)?;
        coins.push(seal_coin_run(path, file, *count, Some(&mut coin_bloom))?);
    }
    if let Some(name) = &manifest.resident {
        if !dir.join(name).is_file() {
            bail!("spill manifest names missing hot-coin file {name}");
        }
    }
    let keep: std::collections::HashSet<String> = manifest
        .tombs
        .iter()
        .map(|(n, _)| n.clone())
        .chain(manifest.coins.iter().map(|(n, _)| n.clone()))
        .chain(manifest.resident.clone())
        .chain(["MANIFEST".to_string(), "coins.bin".to_string()])
        .collect();
    if let Ok(rd) = fs::read_dir(dir) {
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if !keep.contains(&name) {
                let _ = fs::remove_file(ent.path());
            }
        }
    }
    Ok(Some(LoadedOverlay {
        tip: manifest.tip,
        len: manifest.len,
        coins_len: manifest.coins_len,
        resident: manifest.resident,
        heed_off: manifest.heed_off,
        seal_complete: manifest.seal_complete,
        runs: RunSet {
            tombs,
            coins,
            next_tomb: manifest.next_tomb,
            next_coin: manifest.next_coin,
            retired: Vec::new(),
            pause: false,
            stop: false,
            busy: false,
        },
        tomb_bloom,
        coin_bloom,
    }))
}

fn alloc_spill(spill: &mut Vec<u8>, free: &mut Vec<(u32, u32)>, script: &[u8]) -> u32 {
    if let Some(pos) = free.iter().position(|&(_, n)| n as usize >= script.len()) {
        let (off, n) = free.swap_remove(pos);
        let start = off as usize;
        spill[start..start + script.len()].copy_from_slice(script);
        let slack = n as usize - script.len();
        if slack >= 8 {
            free.push((off + script.len() as u32, slack as u32));
        }
        return off;
    }
    let off = u32::try_from(spill.len()).unwrap_or(u32::MAX);
    spill.extend_from_slice(script);
    off
}

fn free_packed_spill(packed: &PackedCoin, free: &mut Vec<(u32, u32)>) {
    // spilled==2 lives in coins.bin; nothing to reclaim in the RAM arena.
    if packed.spilled != 1 {
        return;
    }
    let off = u32::from_le_bytes(packed.inline[..4].try_into().unwrap());
    let n = u32::from_le_bytes(packed.inline[4..8].try_into().unwrap());
    if n == 0 {
        return;
    }
    free.push((off, n));
}

fn rehydrate_batch_rows() -> u64 {
    std::env::var("DISK_UTXO_REHYDRATE_BATCH")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n >= 10_000)
        .unwrap_or(50_000)
}

fn rehydrate_single_tx() -> bool {
    matches!(
        std::env::var("DISK_UTXO_REHYDRATE_SINGLE_TX").as_deref(),
        Ok("1") | Ok("true")
    )
}

fn rehydrate_read_buf_capacity() -> usize {
    std::env::var("DISK_UTXO_REHYDRATE_READ_BUF_MB")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(|mb: usize| mb.clamp(1, 256) * 1024 * 1024)
        .unwrap_or(4 * 1024 * 1024)
}

fn checkpoint_write_buf_capacity() -> usize {
    std::env::var("DISK_UTXO_CHECKPOINT_WRITE_BUF_MB")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(|mb: usize| mb.clamp(1, 256) * 1024 * 1024)
        .unwrap_or(4 * 1024 * 1024)
}

/// Virtual LMDB map (not RSS). 32 GiB covers mainnet UTXO through the cache tip with headroom.
fn heed_map_size_bytes() -> usize {
    let mb = std::env::var("DISK_UTXO_HEED3_MAP_SIZE_MB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(32_768)
        .clamp(4_096, 262_144);
    mb.saturating_mul(1024).saturating_mul(1024)
}

fn heed_env_dir(root: &Path) -> PathBuf {
    root.join("heed3")
}

/// Compact `data.mdb` when copy-on-write free pages have inflated it this far.
fn compact_if_above_bytes() -> u64 {
    std::env::var("DISK_UTXO_COMPACT_IF_ABOVE_GB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(8)
        .saturating_mul(1024)
        .saturating_mul(1024)
        .saturating_mul(1024)
}

fn prefault_max_bytes() -> u64 {
    std::env::var("DISK_UTXO_PREFAULT_MAX_GB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(8)
        .min(32)
        .saturating_mul(1024)
        .saturating_mul(1024)
        .saturating_mul(1024)
}

fn compact_marker_path(env_path: &Path) -> PathBuf {
    env_path.join(".compact-ok")
}

fn packed_copy_path(env_path: &Path) -> PathBuf {
    env_path
        .parent()
        .unwrap_or(env_path)
        .join("data.mdb.compact")
}

fn should_compact_mdb(env_path: &Path, sz: u64) -> bool {
    if sz < compact_if_above_bytes() {
        return false;
    }
    if let Ok(prev) = fs::read_to_string(compact_marker_path(env_path)) {
        if let Ok(prev_sz) = prev.trim().parse::<u64>() {
            return sz > prev_sz.saturating_mul(115) / 100;
        }
    }
    true
}

fn prefault_mdb(path: &Path) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    let sz = meta.len();
    if sz == 0 {
        return;
    }
    let cap = prefault_max_bytes();
    if cap == 0 {
        return;
    }
    let to_read = sz.min(cap);
    eprintln!(
        "   [disk-utxo] prefault {:.1} GiB of {:.1} GiB {}",
        to_read as f64 / (1024.0 * 1024.0 * 1024.0),
        sz as f64 / (1024.0 * 1024.0 * 1024.0),
        path.display()
    );
    let t = Instant::now();
    let Ok(file) = File::open(path) else {
        return;
    };
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        let len = to_read as libc::off_t;
        unsafe {
            libc::posix_fadvise(fd, 0, len, libc::POSIX_FADV_SEQUENTIAL);
            libc::posix_fadvise(fd, 0, len, libc::POSIX_FADV_WILLNEED);
        }
    }
    let mut buf = vec![0u8; 8 * 1024 * 1024];
    let mut done = 0u64;
    let mut r = file;
    while done < to_read {
        let want = ((to_read - done) as usize).min(buf.len());
        match r.read(&mut buf[..want]) {
            Ok(0) => break,
            Ok(n) => done += n as u64,
            Err(_) => break,
        }
    }
    eprintln!(
        "   [disk-utxo] prefault done {:.1}s ({:.1} GiB)",
        t.elapsed().as_secs_f64(),
        done as f64 / (1024.0 * 1024.0 * 1024.0)
    );
}

fn looks_like_rocksdb(root: &Path) -> bool {
    root.join("CURRENT").is_file()
}

fn run_file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn outpoint_key(op: &OutPoint) -> [u8; 36] {
    let mut k = [0u8; 36];
    k[..32].copy_from_slice(&op.hash);
    k[32..36].copy_from_slice(&op.index.to_le_bytes());
    k
}

fn outpoint_from_key(k: &[u8]) -> OutPoint {
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&k[..32]);
    let index = u32::from_le_bytes(k[32..36].try_into().unwrap());
    OutPoint { hash, index }
}

fn utxo_to_bytes_into(u: &UTXO, v: &mut Vec<u8>) {
    let script: &[u8] = u.script_pubkey.as_ref();
    v.clear();
    v.reserve(8 + 8 + 1 + 4 + script.len());
    v.extend_from_slice(&u.value.to_le_bytes());
    v.extend_from_slice(&u.height.to_le_bytes());
    v.push(u.is_coinbase as u8);
    v.extend_from_slice(&(script.len() as u32).to_le_bytes());
    v.extend_from_slice(script);
}

fn utxo_from_bytes(data: &[u8]) -> Result<UTXO> {
    if data.len() < 21 {
        bail!("utxo value too short: {} bytes", data.len());
    }
    let value = i64::from_le_bytes(data[..8].try_into().unwrap());
    let height = u64::from_le_bytes(data[8..16].try_into().unwrap());
    let is_coinbase = data[16] != 0;
    let slen = u32::from_le_bytes(data[17..21].try_into().unwrap()) as usize;
    if data.len() < 21 + slen {
        bail!(
            "utxo value truncated: expected {} script bytes, got {}",
            slen,
            data.len() - 21
        );
    }
    Ok(UTXO {
        value,
        script_pubkey: data[21..21 + slen].to_vec().into(),
        height,
        is_coinbase,
    })
}

type OverlayEntry = Option<Arc<UTXO>>;
type PendingFlush = Arc<Mutex<Option<Arc<FxHashMap<OutPoint, OverlayEntry>>>>>;

/// Scripts this long stay in the hash slot. Covers P2PKH, P2SH, P2WPKH, P2WSH, and P2TR.
const INLINE_SCRIPT: usize = 34;

/// Unspent coin stored by value in the resident map.
/// Longer scripts: RAM `spill` (`spilled=1`) or `coins.bin` body (`spilled=2`).
#[derive(Clone, Copy)]
struct PackedCoin {
    value: i64,
    height: u32,
    coinbase: u8,
    /// 0: script is `inline[..script_len]` (`script_len` ≤ [`INLINE_SCRIPT`]).
    /// 1: `inline[0..4]` = RAM spill offset; `inline[4..8]` = u32 script length (`script_len` unused).
    /// 2: `inline[0..8]` = u64 `coins.bin` offset; `inline[8..12]` = u32 body length (`script_len` unused).
    spilled: u8,
    script_len: u16,
    inline: [u8; INLINE_SCRIPT],
}

impl PackedCoin {
    fn pack(utxo: &UTXO, spill: &mut Vec<u8>, free: &mut Vec<(u32, u32)>) -> Self {
        let script: &[u8] = utxo.script_pubkey.as_ref();
        let mut inline = [0u8; INLINE_SCRIPT];
        let (spilled, script_len) = if script.len() <= INLINE_SCRIPT {
            inline[..script.len()].copy_from_slice(script);
            (0, script.len() as u16)
        } else {
            let off = alloc_spill(spill, free, script);
            inline[..4].copy_from_slice(&off.to_le_bytes());
            let n = u32::try_from(script.len()).unwrap_or(u32::MAX);
            inline[4..8].copy_from_slice(&n.to_le_bytes());
            (1, 0)
        };
        Self {
            value: utxo.value,
            height: u32::try_from(utxo.height).unwrap_or(u32::MAX),
            coinbase: u8::from(utxo.is_coinbase),
            spilled,
            script_len,
            inline,
        }
    }

    /// Hot skip_lmdb coin whose full `utxo_to_bytes` body already lives at `off` in `coins.bin`.
    fn from_disk_body(utxo: &UTXO, off: u64, body_len: u32) -> Result<Self> {
        let mut inline = [0u8; INLINE_SCRIPT];
        inline[..8].copy_from_slice(&off.to_le_bytes());
        inline[8..12].copy_from_slice(&body_len.to_le_bytes());
        Ok(Self {
            value: utxo.value,
            height: u32::try_from(utxo.height).unwrap_or(u32::MAX),
            coinbase: u8::from(utxo.is_coinbase),
            spilled: 2,
            script_len: 0,
            inline,
        })
    }

    fn disk_off(self) -> Option<(u64, u32)> {
        if self.spilled != 2 {
            return None;
        }
        let off = u64::from_le_bytes(self.inline[..8].try_into().unwrap());
        let len = u32::from_le_bytes(self.inline[8..12].try_into().unwrap());
        Some((off, len))
    }

    fn to_utxo(self, spill: &[u8]) -> UTXO {
        let script: Vec<u8> = if self.spilled == 0 {
            let n = (self.script_len as usize).min(INLINE_SCRIPT);
            self.inline[..n].to_vec()
        } else if self.spilled == 1 {
            let off = u32::from_le_bytes(self.inline[..4].try_into().unwrap()) as usize;
            let n = u32::from_le_bytes(self.inline[4..8].try_into().unwrap()) as usize;
            let end = off.saturating_add(n).min(spill.len());
            spill[off.min(end)..end].to_vec()
        } else {
            // spilled==2 needs coins.bin — callers must use `packed_to_utxo`.
            Vec::new()
        };
        UTXO {
            value: self.value,
            script_pubkey: script.into(),
            height: u64::from(self.height),
            is_coinbase: self.coinbase != 0,
        }
    }
}

fn outpoint_fingerprint(op: &OutPoint) -> u64 {
    let mut hasher = FxHasher::default();
    hasher.write(&op.hash);
    hasher.write_u32(op.index);
    hasher.finish()
}

/// One unspent coin. The fingerprint index points here so the hash table stores
/// an 8-byte key instead of a 36-byte outpoint plus this record.
struct CoinRec {
    hash: [u8; 32],
    index: u32,
    value: i64,
    height: u32,
    script_len: u16,
    coinbase: u8,
    spilled: u8,
    script: [u8; INLINE_SCRIPT],
}

impl CoinRec {
    fn matches(&self, op: &OutPoint) -> bool {
        self.hash == op.hash && self.index == op.index
    }

    fn packed(&self) -> PackedCoin {
        PackedCoin {
            value: self.value,
            height: self.height,
            coinbase: self.coinbase,
            spilled: self.spilled,
            script_len: self.script_len,
            inline: self.script,
        }
    }
}

/// Hot coins only. Older coins stay in the read-only heed file so a lookup
/// does not touch a random page of the whole set.
struct ResidentCoins {
    /// Fingerprint → index into `recs`. A fingerprint that hits the wrong
    /// outpoint is recorded in `collisions` instead.
    index: FxHashMap<u64, u32>,
    recs: Vec<CoinRec>,
    free: Vec<u32>,
    collisions: Vec<(u64, u32)>,
    by_height: BTreeMap<u32, Vec<u32>>,
}

impl ResidentCoins {
    fn with_capacity(n: usize) -> Self {
        Self {
            index: FxHashMap::with_capacity_and_hasher(n, Default::default()),
            recs: Vec::with_capacity(n),
            free: Vec::new(),
            collisions: Vec::new(),
            by_height: BTreeMap::new(),
        }
    }

    fn oldest_height(&self) -> Option<u32> {
        self.by_height.keys().next().copied()
    }

    fn len(&self) -> usize {
        self.index.len() + self.collisions.len()
    }

    /// Coins created after the snapshot. The heed file does not have these.
    fn for_each_newer_than(
        &self,
        snapshot_tip: u64,
        mut f: impl FnMut(OutPoint, PackedCoin) -> Result<()>,
    ) -> Result<()> {
        for (&height, idxs) in &self.by_height {
            if u64::from(height) <= snapshot_tip {
                continue;
            }
            for &idx in idxs {
                let rec = &self.recs[idx as usize];
                let op = OutPoint {
                    hash: rec.hash,
                    index: rec.index,
                };
                f(op, rec.packed())?;
            }
        }
        Ok(())
    }

    /// Snapshot-era coins still sitting in the hot slab (seal-skipped set).
    fn for_each_at_most(
        &self,
        snapshot_tip: u64,
        mut f: impl FnMut(OutPoint, PackedCoin) -> Result<()>,
    ) -> Result<()> {
        for (&height, idxs) in &self.by_height {
            if u64::from(height) > snapshot_tip {
                continue;
            }
            for &idx in idxs {
                let rec = &self.recs[idx as usize];
                let op = OutPoint {
                    hash: rec.hash,
                    index: rec.index,
                };
                f(op, rec.packed())?;
            }
        }
        Ok(())
    }

    fn slot(&self, op: &OutPoint) -> Option<u32> {
        let fp = outpoint_fingerprint(op);
        if let Some(&i) = self.index.get(&fp) {
            if self.recs[i as usize].matches(op) {
                return Some(i);
            }
        }
        self.collisions
            .iter()
            .find(|(h, i)| *h == fp && self.recs[*i as usize].matches(op))
            .map(|(_, i)| *i)
    }

    fn contains(&self, op: &OutPoint) -> bool {
        self.slot(op).is_some()
    }

    fn packed(&self, op: &OutPoint) -> Option<PackedCoin> {
        self.slot(op).map(|i| self.recs[i as usize].packed())
    }

    fn insert(&mut self, op: OutPoint, packed: PackedCoin) {
        let fp = outpoint_fingerprint(&op);
        if let Some(i) = self.slot(&op) {
            let old_h = self.recs[i as usize].height;
            let rec = &mut self.recs[i as usize];
            rec.value = packed.value;
            rec.height = packed.height;
            rec.script_len = packed.script_len;
            rec.coinbase = packed.coinbase;
            rec.spilled = packed.spilled;
            rec.script = packed.inline;
            if old_h != packed.height {
                self.detach_height(old_h, i);
                self.by_height.entry(packed.height).or_default().push(i);
            }
            return;
        }
        let rec = CoinRec {
            hash: op.hash,
            index: op.index,
            value: packed.value,
            height: packed.height,
            script_len: packed.script_len,
            coinbase: packed.coinbase,
            spilled: packed.spilled,
            script: packed.inline,
        };
        let idx = if let Some(i) = self.free.pop() {
            self.recs[i as usize] = rec;
            i
        } else {
            let i = u32::try_from(self.recs.len()).unwrap_or(u32::MAX);
            self.recs.push(rec);
            i
        };
        self.by_height.entry(packed.height).or_default().push(idx);
        if self.index.contains_key(&fp) {
            self.collisions.push((fp, idx));
        } else {
            self.index.insert(fp, idx);
        }
    }

    fn detach_height(&mut self, height: u32, idx: u32) {
        let Some(v) = self.by_height.get_mut(&height) else {
            return;
        };
        if let Some(pos) = v.iter().position(|i| *i == idx) {
            v.swap_remove(pos);
        }
        if v.is_empty() {
            self.by_height.remove(&height);
        }
    }

    fn unlink_idx(&mut self, op: &OutPoint, idx: u32) {
        let fp = outpoint_fingerprint(op);
        if self.index.get(&fp) == Some(&idx) {
            self.index.remove(&fp);
            if let Some(pos) = self.collisions.iter().position(|(h, _)| *h == fp) {
                let (_, promoted) = self.collisions.swap_remove(pos);
                self.index.insert(fp, promoted);
            }
            return;
        }
        if let Some(pos) = self.collisions.iter().position(|(h, i)| *h == fp && *i == idx) {
            self.collisions.swap_remove(pos);
        }
    }

    fn remove(&mut self, op: &OutPoint) {
        let Some(idx) = self.slot(op) else {
            return;
        };
        let height = self.recs[idx as usize].height;
        self.unlink_idx(op, idx);
        self.detach_height(height, idx);
        self.free.push(idx);
    }

    /// Drop the oldest coin. Caller decides whether the heed copy is enough.
    fn pop_oldest(&mut self) -> Option<(OutPoint, PackedCoin)> {
        let h = *self.by_height.keys().next()?;
        let idx = {
            let v = self.by_height.get_mut(&h)?;
            let idx = v.pop()?;
            idx
        };
        if self.by_height.get(&h).is_some_and(|v| v.is_empty()) {
            self.by_height.remove(&h);
        }
        let rec = &self.recs[idx as usize];
        let op = OutPoint {
            hash: rec.hash,
            index: rec.index,
        };
        let packed = rec.packed();
        self.unlink_idx(&op, idx);
        self.free.push(idx);
        Some((op, packed))
    }
}

#[cfg(target_os = "linux")]
fn lmdb_madvise_dontneed(env_path: &Path) {
    use std::io::BufRead;
    let data_mdb = env_path.join("data.mdb");
    let target = std::fs::canonicalize(&data_mdb).unwrap_or(data_mdb);
    let target_str = target.to_string_lossy().into_owned();
    let Ok(maps) = std::fs::File::open("/proc/self/maps") else {
        return;
    };
    for line in std::io::BufReader::new(maps).lines().map_while(Result::ok) {
        if !line.ends_with(target_str.as_str()) {
            continue;
        }
        let Some(range) = line.split_whitespace().next() else {
            continue;
        };
        let mut parts = range.splitn(2, '-');
        let (Some(s), Some(e)) = (parts.next(), parts.next()) else {
            continue;
        };
        let (Ok(start), Ok(end)) = (usize::from_str_radix(s, 16), usize::from_str_radix(e, 16))
        else {
            continue;
        };
        if end <= start {
            continue;
        }
        unsafe {
            libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_DONTNEED);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn lmdb_madvise_dontneed(_env_path: &Path) {}

/// Drop clean page cache for `data.mdb`. Unlike `MADV_DONTNEED` on the heed
/// mapping, `POSIX_FADV_DONTNEED` does not discard dirty pages, so a committed
/// checkpoint stays durable while the 10+ GiB file stops evicting the UTXO heap.
fn drop_clean_mdb_cache(env_path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let path = env_path.join("data.mdb");
        let Ok(file) = File::open(&path) else {
            return;
        };
        let len = file.metadata().map(|m| m.len() as libc::off_t).unwrap_or(0);
        unsafe {
            libc::posix_fadvise(file.as_raw_fd(), 0, len, libc::POSIX_FADV_DONTNEED);
        }
    }
}

fn open_heed_env(env_path: &Path) -> Result<(Arc<HeedEnv>, ByteDb)> {
    std::fs::create_dir_all(env_path)
        .with_context(|| format!("create {}", env_path.display()))?;
    let map_size = heed_map_size_bytes();
    eprintln!(
        "   [disk-utxo] opening heed3/LMDB at {} (map {} MiB, NOSYNC, NORDAHEAD, no WRITEMAP)",
        env_path.display(),
        map_size / (1024 * 1024),
    );
    let mut options = EnvOpenOptions::new().read_txn_without_tls();
    // SAFETY: NOSYNC/NOMETASYNC match blvm-node IBD UTXO store. Durability is
    // `force_sync` on overlay flush(durable=true) / Drop. NORDAHEAD: outpoint keys
    // are hashes — kernel readahead of the 10GiB+ file thrashes page cache.
    unsafe {
        options.flags(
            EnvFlags::NO_SYNC | EnvFlags::NO_META_SYNC | EnvFlags::NO_READ_AHEAD | EnvFlags::NO_MEM_INIT,
        );
    }
    let env = unsafe {
        options
            .map_size(map_size)
            .max_readers(128)
            .max_dbs(4)
            .open(env_path)
            .with_context(|| format!("heed3 open {}", env_path.display()))?
    };
    let env = Arc::new(env);
    let utxos = {
        let mut wtxn = env.write_txn().context("heed3 create_database txn")?;
        let db = env
            .create_database(&mut wtxn, Some(UTRE_NAME))
            .context("heed3 create_database(utxos)")?;
        wtxn.commit().context("heed3 create_database commit")?;
        db
    };
    Ok((env, utxos))
}

fn compact_env_to_file(
    env: &HeedEnv,
    env_path: &Path,
    packed: &Path,
    before: u64,
) -> Result<u64> {
    let _ = fs::remove_file(packed);
    eprintln!(
        "   [disk-utxo] compacting {:.1} GiB {} → {}",
        before as f64 / (1024.0 * 1024.0 * 1024.0),
        env_path.join("data.mdb").display(),
        packed.display()
    );
    let t = Instant::now();
    env.force_sync().ok();
    let _f = env
        .copy_to_path(packed, heed3::CompactionOption::Enabled)
        .with_context(|| format!("LMDB compact copy {}", packed.display()))?;
    let packed_sz = fs::metadata(packed)
        .with_context(|| format!("stat packed {}", packed.display()))?
        .len();
    eprintln!(
        "   [disk-utxo] compact copy {:.1}s → {:.1} GiB",
        t.elapsed().as_secs_f64(),
        packed_sz as f64 / (1024.0 * 1024.0 * 1024.0)
    );
    Ok(packed_sz)
}

/// Pack copy-on-write free pages, then sequentially fault the live file into page cache.
fn compact_and_prefault(
    env: Arc<HeedEnv>,
    db: ByteDb,
    env_path: &Path,
) -> Result<(Arc<HeedEnv>, ByteDb)> {
    let mdb = env_path.join("data.mdb");
    let packed = packed_copy_path(env_path);
    let sz = mdb.metadata().map(|m| m.len()).unwrap_or(0);

    if packed.is_file() {
        let packed_sz = packed.metadata().map(|m| m.len()).unwrap_or(0);
        if packed_sz > 0 && packed_sz < sz {
            eprintln!(
                "   [disk-utxo] installing leftover compact copy ({:.1} GiB < {:.1} GiB live)",
                packed_sz as f64 / (1024.0 * 1024.0 * 1024.0),
                sz as f64 / (1024.0 * 1024.0 * 1024.0)
            );
            drop(env);
            fs::rename(&packed, &mdb).context("install leftover compact data.mdb")?;
            let _ = fs::write(compact_marker_path(env_path), format!("{packed_sz}\n"));
            let (env, db) = open_heed_env(env_path)?;
            if !warm_resident_enabled() {
                prefault_mdb(&mdb);
            }
            return Ok((env, db));
        }
        let _ = fs::remove_file(&packed);
    }

    if should_compact_mdb(env_path, sz) {
        match compact_env_to_file(&env, env_path, &packed, sz) {
            Ok(packed_sz) => {
                drop(env);
                fs::rename(&packed, &mdb).context("replace data.mdb with compact copy")?;
                let _ = fs::write(compact_marker_path(env_path), format!("{packed_sz}\n"));
                let (env, db) = open_heed_env(env_path)?;
                if !warm_resident_enabled() {
                    prefault_mdb(&mdb);
                }
                return Ok((env, db));
            }
            Err(e) => {
                eprintln!("   [disk-utxo] compact skipped: {e:#}");
                let _ = fs::remove_file(&packed);
            }
        }
    }

    if !warm_resident_enabled() && sz > 0 && sz <= prefault_max_bytes().saturating_mul(2) {
        prefault_mdb(&mdb);
    }
    Ok((env, db))
}

fn read_meta_u64(env: &HeedEnv, db: &ByteDb, key: &[u8]) -> Result<Option<u64>> {
    let rtxn = env.read_txn().context("heed3 meta read_txn")?;
    match db.get(&rtxn, key).context("heed3 meta get")? {
        Some(v) if v.len() == 8 => Ok(Some(u64::from_le_bytes(v.try_into().unwrap()))),
        Some(_) => bail!("corrupt disk-utxo meta key"),
        None => Ok(None),
    }
}

fn db_has_utxo_rows(env: &HeedEnv, db: &ByteDb) -> Result<bool> {
    let rtxn = env.read_txn().context("heed3 has_rows read_txn")?;
    for item in db.iter(&rtxn).context("heed3 has_rows iter")? {
        let (k, _) = item.context("heed3 has_rows item")?;
        if !is_disk_utxo_meta_key(k) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn resolve_open_len(env: &HeedEnv, db: &ByteDb) -> Result<u64> {
    match read_meta_u64(env, db, META_LEN_KEY)? {
        Some(n) => Ok(n),
        None => {
            if db_has_utxo_rows(env, db)? {
                bail!("disk-utxo LMDB has UTXO rows but no length metadata");
            }
            Ok(0)
        }
    }
}

fn migrate_rocksdb_into_heed(rocks_root: &Path, env: &HeedEnv, db: ByteDb) -> Result<()> {
    use rocksdb::{DB, IteratorMode, Options};
    eprintln!(
        "   [disk-utxo] migrating RocksDB at {} → heed3/LMDB …",
        rocks_root.display()
    );
    let mut opts = Options::default();
    opts.create_if_missing(false);
    let rdb = DB::open_for_read_only(&opts, rocks_root, false)
        .with_context(|| format!("open RocksDB read-only {}", rocks_root.display()))?;
    let mut loaded: u64 = 0;
    let mut tip: Option<u64> = None;
    let mut len_meta: Option<u64> = None;
    let batch = rehydrate_batch_rows();
    let overall = Instant::now();
    let mut wtxn = env.write_txn().context("heed3 migrate write_txn")?;
    let mut in_txn = 0u64;
    let mut iter = rdb.iterator(IteratorMode::Start);
    while let Some(res) = iter.next() {
        let (k, v) = res.context("rocksdb iterate")?;
        if k.as_ref() == META_TIP_KEY && v.len() == 8 {
            tip = Some(u64::from_le_bytes(v.as_ref().try_into().unwrap()));
            db.put(&mut wtxn, META_TIP_KEY, v.as_ref())?;
            continue;
        }
        if k.as_ref() == META_LEN_KEY && v.len() == 8 {
            len_meta = Some(u64::from_le_bytes(v.as_ref().try_into().unwrap()));
            continue;
        }
        if k.len() != 36 {
            continue;
        }
        db.put(&mut wtxn, k.as_ref(), v.as_ref())?;
        loaded += 1;
        in_txn += 1;
        if in_txn >= batch {
            wtxn.commit().context("heed3 migrate commit")?;
            eprintln!("   [disk-utxo] migrate {loaded} rows …");
            wtxn = env.write_txn().context("heed3 migrate write_txn")?;
            in_txn = 0;
        }
    }
    if len_meta.is_some_and(|n| n != loaded) {
        eprintln!(
            "   [disk-utxo] rocks length meta {len_meta:?} != {loaded} copied rows; storing the copied count"
        );
    }
    let final_len = loaded;
    db.put(&mut wtxn, META_LEN_KEY, &final_len.to_le_bytes())?;
    if let Some(h) = tip {
        db.put(&mut wtxn, META_TIP_KEY, &h.to_le_bytes())?;
    }
    wtxn.commit().context("heed3 migrate final commit")?;
    env.force_sync().context("heed3 migrate force_sync")?;
    let sec = overall.elapsed().as_secs_f64().max(1e-9);
    eprintln!(
        "   [disk-utxo] migrate done {loaded} rows (len meta {final_len}, tip {tip:?}) in {sec:.1}s ({:.0} k/s)",
        (loaded as f64 / sec) / 1000.0
    );
    Ok(())
}

pub struct DiskUtxoSet {
    env: Arc<HeedEnv>,
    db: ByteDb,
    env_path: PathBuf,
    utxo_tip_height: Option<u64>,
    inflight_tip: Option<u64>,
    len: u64,
    overlay: FxHashMap<OutPoint, OverlayEntry>,
    /// Newest-first flushed batches still held for prefetch. Not dirty.
    cache_gens: VecDeque<Arc<FxHashMap<OutPoint, OverlayEntry>>>,
    cache_entries: usize,
    cache_cap: usize,
    /// Hot coins live in `resident`. Cold coins are read from heed. The original
    /// heed file is not written; spill holds tombstones and evicted new coins.
    skip_lmdb: bool,
    /// Cold snapshot coins live in sealed spill. Compare does not fault heed.
    heed_off: bool,
    /// Every snapshot coin is indexed in sealed spill; `warm_resident` may skip.
    seal_complete: bool,
    /// Heed `META_LEN`. Warm compares against this, not [`Self::len`], which
    /// counts coins created after the snapshot.
    heed_len: u64,
    /// Newest unspent coins. Older coins are in sealed spill once `heed_off`.
    resident: ResidentCoins,
    snapshot_tip: u64,
    hot_cap: usize,
    spill_path: PathBuf,
    /// Coin bodies evicted from the hot slab (height above the snapshot).
    pending_coins: FxHashMap<OutPoint, Vec<u8>>,
    /// Disk-backed hot coins already in `coins.bin`; flush writes index only.
    pending_disk_ix: Vec<[u8; COIN_IX]>,
    coin_bloom: Vec<u64>,
    coins_file: Option<File>,
    coins_len: u64,
    /// Spends of coins that still live in the original heed file.
    pending_tombs: FxHashSet<OutPoint>,
    tomb_bloom: Vec<u64>,
    compact: CompactShared,
    compact_thread: Option<std::thread::JoinHandle<()>>,
    /// Script bytes longer than [`INLINE_SCRIPT`]. Offsets are stored in `PackedCoin::inline`.
    /// Freed ranges are reused so spent coins do not grow this buffer.
    spill: Vec<u8>,
    spill_free: Vec<(u32, u32)>,
    stats_ov: u64,
    stats_cache: u64,
    stats_res: u64,
    stats_db: u64,
    stats_db_hit: u64,
    stats_db_ms: f64,
    stats_prefetch_ms: f64,
    stats_cold_ms: f64,
    stats_body_reads: u64,
    stats_body_ms: f64,
    pending_flush: PendingFlush,
    flush_thread: Option<std::thread::JoinHandle<Result<()>>>,
    bg_checkpoint: Option<std::thread::JoinHandle<Result<()>>>,
    /// When false, [`Drop`] skips durable `flush_overlay` so a failed apply cannot
    /// seal a half-mutated tip. Cleared on apply `Err`; set again on success.
    checkpoint_on_drop: bool,
    /// Test-only: next `pack_for_hot` returns `Err` without mutating resident/tombs.
    #[cfg(test)]
    fail_next_pack_for_hot: bool,
}

impl DiskUtxoSet {
    pub fn open(path: &Path) -> Result<Self> {
        if path.exists() && path.is_file() {
            bail!(
                "{} is a file; disk-utxo needs a directory (heed3 env lives in {{path}}/heed3/).",
                path.display()
            );
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir for {}", path.display()))?;
        }
        std::fs::create_dir_all(path)?;
        let env_path = heed_env_dir(path);
        let (env, db) = open_heed_env(&env_path)?;
        let len = resolve_open_len(&env, &db)?;

        if len == 0 && looks_like_rocksdb(path) {
            migrate_rocksdb_into_heed(path, &env, db)?;
        }

        let (env, db) = compact_and_prefault(env, db, &env_path)?;
        let len = resolve_open_len(&env, &db)?;
        let utxo_tip_height = read_meta_u64(&env, &db, META_TIP_KEY)?;

        let cap = overlay_cap();
        let cache = cache_cap();
        let warm = warm_resident_enabled();
        let hot_cap = hot_coin_cap();
        let snapshot_tip = utxo_tip_height.unwrap_or(0);
        eprintln!(
            "   [disk-utxo] opened heed3 {} ({len} entries, RAM overlay, flush-cache {cache}, LMDB checkpoint-only, warm_resident={warm}, hot_cap={hot_cap}, tip {utxo_tip_height:?})",
            env_path.display()
        );
        let spill_path = path.join("heed3-spill");
        let loaded = if warm && len > 0 {
            match load_overlay(&spill_path, snapshot_tip) {
                Ok(Some(mut overlay)) => {
                    collapse_sealed_runs(&spill_path, &mut overlay, snapshot_tip)?;
                    Some(overlay)
                }
                Ok(None) => {
                    if spill_path.exists() {
                        let _ = fs::remove_dir_all(&spill_path);
                    }
                    None
                }
                Err(e) => {
                    eprintln!(
                        "   [disk-utxo] spill manifest unusable ({e:#}); resuming from heed tip {snapshot_tip}"
                    );
                    let _ = fs::remove_dir_all(&spill_path);
                    None
                }
            }
        } else {
            None
        };
        let logical_len = loaded.as_ref().map(|o| o.len).unwrap_or(len);
        let resume_tip = loaded
            .as_ref()
            .map(|o| Some(o.tip))
            .unwrap_or(utxo_tip_height);
        let coins_len = loaded.as_ref().map(|o| o.coins_len).unwrap_or(0);
        let tomb_bloom = loaded.as_ref().map(|o| o.tomb_bloom.clone()).unwrap_or_default();
        let coin_bloom = loaded.as_ref().map(|o| o.coin_bloom.clone()).unwrap_or_default();
        let restored = loaded.is_some();
        let resident_file = loaded.as_ref().and_then(|o| o.resident.clone());
        let heed_off = loaded.as_ref().map(|o| o.heed_off).unwrap_or(false);
        let seal_complete = loaded.as_ref().map(|o| o.seal_complete).unwrap_or(false);
        let runs = loaded.map(|o| o.runs).unwrap_or_else(RunSet::fresh);
        if restored {
            eprintln!(
                "   [disk-utxo] restored spill at tip {} (len {logical_len}, coins {coins_len} bytes); heed tip {snapshot_tip} stays read-only",
                resume_tip.unwrap_or(snapshot_tip)
            );
        }
        let mut set = Self {
            env,
            db,
            env_path,
            inflight_tip: resume_tip,
            utxo_tip_height: resume_tip,
            len: logical_len,
            overlay: FxHashMap::with_capacity_and_hasher(cap.min(200_000), Default::default()),
            cache_gens: VecDeque::new(),
            cache_entries: 0,
            cache_cap: cache,
            skip_lmdb: warm,
            heed_off,
            seal_complete,
            heed_len: len,
            resident: ResidentCoins::with_capacity(if warm {
                hot_cap
            } else {
                cache.min(200_000)
            }),
            snapshot_tip,
            hot_cap,
            spill_path,
            pending_coins: FxHashMap::default(),
            pending_disk_ix: Vec::new(),
            coin_bloom,
            coins_file: None,
            coins_len,
            pending_tombs: FxHashSet::default(),
            tomb_bloom,
            compact: Arc::new((Mutex::new(runs), Condvar::new())),
            compact_thread: None,
            spill: Vec::new(),
            spill_free: Vec::new(),
            stats_ov: 0,
            stats_cache: 0,
            stats_res: 0,
            stats_db: 0,
            stats_db_hit: 0,
            stats_db_ms: 0.0,
            stats_prefetch_ms: 0.0,
            stats_cold_ms: 0.0,
            stats_body_reads: 0,
            stats_body_ms: 0.0,
            pending_flush: Arc::new(Mutex::new(None)),
            flush_thread: None,
            bg_checkpoint: None,
            checkpoint_on_drop: true,
            #[cfg(test)]
            fail_next_pack_for_hot: false,
        };
        if warm && len > 0 {
            set.open_spill(!restored)?;
            set.spawn_compact();
            let skip_warm = set.heed_off && set.seal_complete;
            if skip_warm {
                eprintln!(
                    "   [disk-utxo] heed off + seal_complete: skipping warm ({} heed rows)",
                    set.heed_len
                );
            } else {
                set.warm_resident()?;
                if set.heed_off && !set.seal_complete {
                    set.reseal_hot_snapshot_coins()?;
                } else if !set.heed_off {
                    set.seal_cold_from_heed()?;
                }
            }
            if restored {
                if let Some(name) = resident_file {
                    set.load_resident_file(&name)?;
                    set.flush_spill()?;
                }
            }
            set.release_ram_spill_arena();
            if set.heed_off {
                eprintln!(
                    "   [disk-utxo] heed off (cold snapshot coins already in sealed spill)"
                );
                lmdb_madvise_dontneed(&set.env_path);
                drop_clean_mdb_cache(&set.env_path);
            }
        }
        Ok(set)
    }

    fn open_spill(&mut self, truncate_coins: bool) -> Result<()> {
        use std::io::Seek;
        std::fs::create_dir_all(&self.spill_path)
            .with_context(|| format!("create {}", self.spill_path.display()))?;
        let coins = self.spill_path.join("coins.bin");
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true).write(true).create(true);
        if truncate_coins {
            opts.truncate(true);
        }
        let mut file = opts
            .open(&coins)
            .with_context(|| format!("create {}", coins.display()))?;
        advise_random(&file);
        if truncate_coins {
            self.coins_len = 0;
        } else {
            file.set_len(self.coins_len)
                .with_context(|| format!("truncate {}", coins.display()))?;
            file.seek(std::io::SeekFrom::Start(self.coins_len))?;
        }
        self.coins_file = Some(file);
        if self.tomb_bloom.is_empty() {
            let words = tomb_bloom_words();
            self.tomb_bloom = vec![0u64; words];
            self.coin_bloom = vec![0u64; words];
        }
        eprintln!(
            "   [disk-utxo] spill {} (tomb+coin bloom {} MiB each, append-only)",
            self.spill_path.display(),
            self.tomb_bloom.len() * 8 / (1024 * 1024)
        );
        Ok(())
    }

    fn spawn_compact(&mut self) {
        let shared = Arc::clone(&self.compact);
        let dir = self.spill_path.clone();
        self.compact_thread = Some(
            std::thread::Builder::new()
                .name("spill-compact".into())
                .spawn(move || compact_loop(shared, dir))
                .expect("spawn spill-compact"),
        );
    }

    fn wake_compact(&self) {
        self.compact.1.notify_one();
    }

    /// A flush may leave one extra run in a tier. The next flush waits until
    /// the compactor has folded that tier back to a single run.
    fn wait_tiers_settled(&self) {
        let (lock, cv) = &*self.compact;
        let mut guard = lock.lock().unwrap();
        self.wake_compact();
        while !guard.stop && !guard.pause && (guard.busy || tomb_tier_pair(&guard).is_some() || coin_tier_pair(&guard).is_some())
        {
            guard = cv.wait(guard).unwrap();
        }
    }

    fn pause_compact(&self) {
        let (lock, cv) = &*self.compact;
        let mut guard = lock.lock().unwrap();
        guard.pause = true;
        while guard.busy {
            guard = cv.wait(guard).unwrap();
        }
    }

    fn resume_compact(&self) {
        let (lock, cv) = &*self.compact;
        let mut guard = lock.lock().unwrap();
        guard.pause = false;
        cv.notify_one();
    }

    fn stop_compact(&mut self) {
        {
            let (lock, cv) = &*self.compact;
            let mut guard = lock.lock().unwrap();
            guard.stop = true;
            guard.pause = false;
            cv.notify_one();
        }
        if let Some(handle) = self.compact_thread.take() {
            let _ = handle.join();
        }
    }

    fn maybe_checkpoint(&mut self) -> Result<()> {
        let every = overlay_checkpoint_every();
        if every == 0 {
            return Ok(());
        }
        let height = self.inflight_tip.unwrap_or(0);
        if height > self.snapshot_tip && height % every == 0 {
            self.checkpoint_spill()?;
        }
        Ok(())
    }

    fn checkpoint_spill(&mut self) -> Result<()> {
        if !self.skip_lmdb {
            return Ok(());
        }
        self.pause_compact();
        let result = self.write_spill_checkpoint();
        self.resume_compact();
        result
    }

    fn write_spill_checkpoint(&mut self) -> Result<()> {
        self.flush_spill()?;
        let resident_name = format!("resident-{}.bin", self.inflight_tip.unwrap_or(self.snapshot_tip));
        self.write_resident_file(&resident_name)?;
        if let Some(file) = self.coins_file.as_mut() {
            file.flush()?;
            file.sync_all().ok();
        }
        let manifest = {
            let guard = self.compact.0.lock().unwrap();
            OverlayManifest {
                snapshot_tip: self.snapshot_tip,
                tip: self.inflight_tip.unwrap_or(self.snapshot_tip),
                len: self.len,
                coins_len: self.coins_len,
                next_tomb: guard.next_tomb,
                next_coin: guard.next_coin,
                resident: Some(resident_name.clone()),
                heed_off: self.heed_off,
                seal_complete: self.seal_complete,
                tombs: guard
                    .tombs
                    .iter()
                    .map(|run| (run_file_name(&run.path), run.count()))
                    .collect(),
                coins: guard
                    .coins
                    .iter()
                    .map(|run| (run_file_name(&run.path), run.count()))
                    .collect(),
            }
        };
        let tmp = self.spill_path.join("MANIFEST.tmp");
        {
            let mut file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
            file.write_all(format_manifest(&manifest).as_bytes())?;
            file.sync_all()?;
        }
        let dest = self.spill_path.join("MANIFEST");
        fs::rename(&tmp, &dest).with_context(|| format!("rename {}", dest.display()))?;
        if let Ok(dir) = File::open(&self.spill_path) {
            dir.sync_all().ok();
        }
        let keep: std::collections::HashSet<String> = manifest
            .tombs
            .iter()
            .map(|(name, _)| name.clone())
            .chain(manifest.coins.iter().map(|(name, _)| name.clone()))
            .collect();
        let mut guard = self.compact.0.lock().unwrap();
        guard.retired.retain(|path| {
            let name = run_file_name(path);
            if keep.contains(&name) {
                return true;
            }
            let _ = fs::remove_file(path);
            false
        });
        drop(guard);
        // The manifest now names `resident_name`. Older hot-coin files are uncommitted.
        if let Ok(rd) = fs::read_dir(&self.spill_path) {
            for ent in rd.flatten() {
                let name = ent.file_name().to_string_lossy().into_owned();
                let stale_hot = name != resident_name
                    && (name.starts_with("resident-") || name == "resident-new.bin");
                if stale_hot {
                    let _ = fs::remove_file(ent.path());
                }
            }
        }
        Ok(())
    }

    fn write_resident_file(&self, name: &str) -> Result<()> {
        let tmp = self.spill_path.join(format!("{name}.tmp"));
        let dest = self.spill_path.join(name);
        let mut out = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        self.resident
            .for_each_newer_than(self.snapshot_tip, |op, packed| {
                let mut body = Vec::new();
                utxo_to_bytes_into(&self.packed_to_utxo(packed)?, &mut body);
                out.write_all(&outpoint_key(&op))?;
                out.write_all(&(body.len() as u32).to_le_bytes())?;
                out.write_all(&body)?;
                Ok(())
            })?;
        out.flush()?;
        out.sync_all()?;
        fs::rename(&tmp, &dest)?;
        Ok(())
    }

    fn load_resident_file(&mut self, name: &str) -> Result<()> {
        let path = self.spill_path.join(name);
        if !path.is_file() {
            return Ok(());
        }
        let mut file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
        let mut loaded = 0u64;
        loop {
            let mut key = [0u8; TOMB_REC];
            if !read_exact_or_eof(&mut file, &mut key)? {
                break;
            }
            let mut len_buf = [0u8; 4];
            file.read_exact(&mut len_buf)?;
            let n = u32::from_le_bytes(len_buf) as usize;
            let mut body = vec![0u8; n];
            file.read_exact(&mut body)
                .with_context(|| format!("read {}", path.display()))?;
            let op = outpoint_from_key(&key);
            let utxo = utxo_from_bytes(&body)?;
            self.consider_hot(op, &utxo, true)?;
            loaded += 1;
        }
        eprintln!("   [disk-utxo] restored {loaded} post-snapshot hot coins");
        Ok(())
    }

    fn release_ram_spill_arena(&mut self) {
        self.spill.clear();
        self.spill.shrink_to_fit();
        self.spill_free.clear();
        self.spill_free.shrink_to_fit();
        #[cfg(feature = "low-mem-alloc")]
        unsafe {
            libmimalloc_sys::mi_collect(true);
        }
    }

    /// Append full UTXO body to `coins.bin` for skip_lmdb hot coins with long scripts.
    fn pack_for_hot(&mut self, utxo: &UTXO) -> Result<PackedCoin> {
        #[cfg(test)]
        if self.fail_next_pack_for_hot {
            self.fail_next_pack_for_hot = false;
            bail!("test-injected pack_for_hot failure");
        }
        if !self.skip_lmdb {
            return Ok(PackedCoin::pack(utxo, &mut self.spill, &mut self.spill_free));
        }
        let script: &[u8] = utxo.script_pubkey.as_ref();
        if script.len() <= INLINE_SCRIPT || utxo.height <= self.snapshot_tip {
            // Snapshot coins should already be sealed; keep metadata inline/RAM only
            // for the rare case they sit in hot before reseal clears them.
            return Ok(PackedCoin::pack(utxo, &mut self.spill, &mut self.spill_free));
        }
        let mut body = Vec::new();
        utxo_to_bytes_into(utxo, &mut body);
        let len = u32::try_from(body.len()).context("coin longer than 4 GiB")?;
        let off = self.coins_len;
        {
            let file = self.coins_file.as_mut().context("coins file")?;
            file.write_all(&body)?;
        }
        self.coins_len = self.coins_len.saturating_add(body.len() as u64);
        PackedCoin::from_disk_body(utxo, off, len)
    }

    fn packed_to_utxo(&self, packed: PackedCoin) -> Result<UTXO> {
        if packed.spilled != 2 {
            return Ok(packed.to_utxo(&self.spill));
        }
        let (off, len) = packed.disk_off().context("disk-backed packed coin")?;
        let bytes = self.read_coin_body(off, len)?;
        utxo_from_bytes(&bytes)
    }

    /// Read one `coins.bin` body. Counters move only after the read succeeds.
    fn read_coin_body(&self, off: u64, len: u32) -> Result<Vec<u8>> {
        let mut bytes = vec![0u8; len as usize];
        let file = self.coins_file.as_ref().context("coins file")?;
        let t_body = Instant::now();
        read_at(file, &mut bytes, off)?;
        BODY_READS.fetch_add(1, Ordering::Relaxed);
        BODY_NS.fetch_add(
            t_body.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
        Ok(bytes)
    }

    fn coin_ix_rec(op: &OutPoint, off: u64, len: u32) -> [u8; COIN_IX] {
        let mut rec = [0u8; COIN_IX];
        rec[..TOMB_REC].copy_from_slice(&outpoint_key(op));
        rec[36..44].copy_from_slice(&off.to_le_bytes());
        rec[44..48].copy_from_slice(&len.to_le_bytes());
        rec
    }

    /// Existing heed_off trees skipped hot coins at first seal. Warm restored
    /// that set into the hot slab; publish them into sealed spill once.
    fn reseal_hot_snapshot_coins(&mut self) -> Result<()> {
        let t0 = Instant::now();
        let mut batch: Vec<[u8; COIN_IX]> = Vec::new();
        let mut sealed = 0u64;
        let mut bodies: Vec<(OutPoint, Vec<u8>)> = Vec::new();
        self.resident.for_each_at_most(self.snapshot_tip, |op, packed| {
            let utxo = packed.to_utxo(&self.spill);
            let mut body = Vec::new();
            utxo_to_bytes_into(&utxo, &mut body);
            bodies.push((op, body));
            Ok(())
        })?;
        for (op, body) in bodies {
            let len = u32::try_from(body.len()).context("coin longer than 4 GiB")?;
            let off = self.coins_len;
            {
                let file = self.coins_file.as_mut().context("coins file")?;
                file.write_all(&body)?;
            }
            self.coins_len = self.coins_len.saturating_add(body.len() as u64);
            let key = outpoint_key(&op);
            bloom_insert(&mut self.coin_bloom, &key);
            batch.push(Self::coin_ix_rec(&op, off, len));
            sealed += 1;
            if batch.len() >= 1_000_000 {
                self.publish_coin_batch(&mut batch)?;
            }
        }
        if !batch.is_empty() {
            self.publish_coin_batch(&mut batch)?;
        }
        if let Some(file) = self.coins_file.as_mut() {
            file.flush()?;
            file.sync_all().ok();
        }
        // Snapshot coins are now in sealed spill; drop the hot slab so load_resident
        // does not thrash them through pack/evict.
        self.resident = ResidentCoins::with_capacity(self.hot_cap);
        self.spill.clear();
        self.spill_free.clear();
        self.seal_complete = true;
        self.persist_overlay_manifest()?;
        eprintln!(
            "   [disk-utxo] resealed {sealed} hot snapshot coins into spill in {:.1}s (seal_complete)",
            t0.elapsed().as_secs_f64()
        );
        Ok(())
    }

    pub fn default_db_path() -> PathBuf {
        if let Ok(p) = std::env::var("DISK_UTXO_DB_PATH") {
            return PathBuf::from(p);
        }
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("blvm-bench")
            .join("utxo_heed3")
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn utxo_tip_height(&self) -> Option<u64> {
        self.utxo_tip_height
    }

    pub fn inflight_tip(&self) -> Option<u64> {
        self.inflight_tip
    }

    pub fn set_inflight_tip(&mut self, height: u64) {
        self.inflight_tip = Some(height);
    }

    pub fn persist_utxo_tip_height(&mut self, height: u64) -> Result<()> {
        let mut wtxn = self.env.write_txn().context("heed3 persist tip")?;
        self.db
            .put(&mut wtxn, META_TIP_KEY, &height.to_le_bytes())?;
        wtxn.commit()?;
        self.utxo_tip_height = Some(height);
        self.inflight_tip = Some(height);
        Ok(())
    }

    pub fn overlay_len(&self) -> usize {
        self.overlay.len()
    }

    pub fn take_prefetch_window_stats(&mut self) -> DiskUtxoWindowStats {
        let (tomb_runs, coin_runs) = {
            let guard = self.compact.0.lock().unwrap();
            (guard.tombs.len() as u64, guard.coins.len() as u64)
        };
        let out = DiskUtxoWindowStats {
            ov: self.stats_ov,
            cache: self.stats_cache,
            res: self.stats_res,
            db: self.stats_db,
            db_hit: self.stats_db_hit,
            db_ms: self.stats_db_ms,
            prefetch_ms: self.stats_prefetch_ms,
            cold_ms: self.stats_cold_ms,
            body_reads: BODY_READS.swap(0, Ordering::Relaxed)
                .saturating_add(self.stats_body_reads),
            body_ms: self.stats_body_ms
                + (BODY_NS.swap(0, Ordering::Relaxed) as f64 / 1_000_000.0),
            tomb_pages: TOMB_PAGE_PROBES.swap(0, Ordering::Relaxed),
            coin_pages: COIN_PAGE_PROBES.swap(0, Ordering::Relaxed),
            hot_len: self.resident.len() as u64,
            spill_script_bytes: self.spill.len() as u64,
            spill_cap: self.spill.capacity() as u64,
            tomb_runs,
            coin_runs,
            heed_off: self.heed_off,
        };
        self.stats_ov = 0;
        self.stats_cache = 0;
        self.stats_res = 0;
        self.stats_db = 0;
        self.stats_db_hit = 0;
        self.stats_db_ms = 0.0;
        self.stats_prefetch_ms = 0.0;
        self.stats_cold_ms = 0.0;
        self.stats_body_reads = 0;
        self.stats_body_ms = 0.0;
        let _ = PAGE_PROBES.swap(0, Ordering::Relaxed);
        out
    }

    /// Sequential LMDB scan into `resident`. Compare-path lookups then never hit heed.
    fn warm_resident(&mut self) -> Result<()> {
        let t0 = Instant::now();
        eprintln!(
            "   [disk-utxo] warming resident UTXO map from heed ({} rows) — sequential, once",
            self.heed_len
        );
        let env = Arc::clone(&self.env);
        let db = self.db;
        let rtxn = env.read_txn().context("heed3 warm read_txn")?;
        let mut n = 0u64;
        for item in db.iter(&rtxn).context("heed3 warm iter")? {
            let (k, v) = item.context("heed3 warm item")?;
            if is_disk_utxo_meta_key(k) {
                continue;
            }
            if k.len() < 36 {
                continue;
            }
            let op = outpoint_from_key(k);
            let utxo = utxo_from_bytes(v)?;
            // Spent snapshot coins are still in the heed file. Lookups answer
            // from this slab before they consult tombstones, so a coin the
            // bloom might have spent stays cold and the tomb lookup decides.
            if bloom_maybe(&self.tomb_bloom, &outpoint_key(&op)) {
                n += 1;
                continue;
            }
            self.consider_hot(op, &utxo, false)?;
            n += 1;
            if n % 5_000_000 == 0 {
                // One word per page, so the coin table stays on the active list
                // while the scan is the only thing the kernel otherwise sees.
                let stride = (4096 / std::mem::size_of::<CoinRec>()).max(1);
                let mut i = 0;
                let mut page_touch = 0u64;
                while i < self.resident.recs.len() {
                    page_touch = page_touch.wrapping_add(self.resident.recs[i].value as u64);
                    i += stride;
                }
                eprintln!(
                    "   [disk-utxo] warm {n}/{} ({:.0}s, touch {page_touch:#x})",
                    self.heed_len,
                    t0.elapsed().as_secs_f64()
                );
            }
        }
        // End the read txn before dropping the map. MADV_DONTNEED during the
        // cursor made LMDB skip about 2.1M rows and the next block missed a prevout.
        drop(rtxn);
        if n != self.heed_len {
            anyhow::bail!(
                "warm loaded {n} coins but heed length is {} — refusing to compare a partial set",
                self.heed_len
            );
        }
        lmdb_madvise_dontneed(&self.env_path);
        drop_clean_mdb_cache(&self.env_path);
        // File cache is gone. Touch every slot so the table is resident before compare.
        let mut touch = 0u64;
        for rec in &self.resident.recs {
            touch = touch.wrapping_add(rec.value as u64);
        }
        eprintln!(
            "   [disk-utxo] resident touch {touch:#x} after dropping heed cache"
        );
        eprintln!(
            "   [disk-utxo] warm done {n} coins in {:.1}s (hot {}, cold {}, spill {} bytes); original heed is read-only",
            t0.elapsed().as_secs_f64(),
            self.resident.len(),
            n.saturating_sub(self.resident.len() as u64),
            self.spill.len()
        );
        Ok(())
    }

    pub fn load_checkpoint_fixed_v1(
        &mut self,
        checkpoint_path: &Path,
        progress: Option<&Path>,
        checkpoint_height: u64,
    ) -> Result<u64> {
        let read_cap = rehydrate_read_buf_capacity();
        let file = std::fs::File::open(checkpoint_path)
            .with_context(|| format!("open {}", checkpoint_path.display()))?;
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsRawFd;
            unsafe {
                libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_SEQUENTIAL);
                libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
            }
        }
        let mut r = std::io::BufReader::with_capacity(read_cap, file);
        let mut header = [0u8; crate::utxo_snapshot_fixed_v1::HEADER_LEN];
        r.read_exact(&mut header)?;
        if header[..8] != crate::utxo_snapshot_fixed_v1::FIXED_V1_MAGIC[..] {
            bail!("not a fixed-v1 checkpoint");
        }
        let count = u64::from_le_bytes(header[20..28].try_into().unwrap());
        let batch_rows = rehydrate_batch_rows();
        let single_tx = rehydrate_single_tx();
        eprintln!(
            "   [disk-utxo] loading {} entries from {} into heed3 (batch={batch_rows})",
            count,
            checkpoint_path.display(),
        );
        if self.len > 0 || db_has_utxo_rows(&self.env, &self.db)? {
            bail!("disk-utxo DB is not empty; delete the DB directory first for a clean load");
        }
        let stat_every = rehydrate_status_every_entries();
        if let Some(p) = progress {
            write_rehydrate_progress(p, 0, count, checkpoint_height);
        }
        let mut loaded: u64 = 0;
        let mut next_status_at: u64 = stat_every;
        let mut vbuf = Vec::with_capacity(128);
        let overall = Instant::now();
        let bump = |loaded: u64, next_status_at: &mut u64| {
            if let Some(p) = progress {
                if loaded >= *next_status_at || loaded == count {
                    write_rehydrate_progress(p, loaded, count, checkpoint_height);
                    *next_status_at = loaded + stat_every;
                }
            }
        };

        let put_one =
            |wtxn: &mut heed3::RwTxn<'_>, db: ByteDb, op: &OutPoint, utxo: &UTXO, vbuf: &mut Vec<u8>| {
                let k = outpoint_key(op);
                utxo_to_bytes_into(utxo, vbuf);
                db.put(wtxn, k.as_slice(), vbuf.as_slice())
            };

        if single_tx {
            let mut wtxn = self.env.write_txn()?;
            while let Some((op, utxo)) = try_read_one_fixed_v1_entry(&mut r)? {
                put_one(&mut wtxn, self.db, &op, &utxo, &mut vbuf)?;
                loaded += 1;
                bump(loaded, &mut next_status_at);
            }
            self.db
                .put(&mut wtxn, META_LEN_KEY, &loaded.to_le_bytes())?;
            wtxn.commit()?;
        } else {
            'outer: loop {
                let mut wtxn = self.env.write_txn()?;
                let mut batch_len = 0u64;
                while batch_len < batch_rows {
                    match try_read_one_fixed_v1_entry(&mut r)? {
                        Some((op, utxo)) => {
                            put_one(&mut wtxn, self.db, &op, &utxo, &mut vbuf)?;
                            loaded += 1;
                            batch_len += 1;
                            bump(loaded, &mut next_status_at);
                        }
                        None => {
                            if batch_len > 0 {
                                self.db
                                    .put(&mut wtxn, META_LEN_KEY, &loaded.to_le_bytes())?;
                                wtxn.commit()?;
                            }
                            break 'outer;
                        }
                    }
                }
                self.db
                    .put(&mut wtxn, META_LEN_KEY, &loaded.to_le_bytes())?;
                wtxn.commit()?;
                eprintln!(
                    "   [disk-utxo] commit {:.0}%  total {loaded}/{}",
                    100.0 * loaded as f64 / count.max(loaded) as f64,
                    count
                );
            }
        }
        let mut wtxn = self.env.write_txn()?;
        self.db
            .put(&mut wtxn, META_LEN_KEY, &loaded.to_le_bytes())?;
        self.db
            .put(&mut wtxn, META_TIP_KEY, &checkpoint_height.to_le_bytes())?;
        wtxn.commit()?;
        self.env.force_sync()?;
        lmdb_madvise_dontneed(&self.env_path);
        if let Some(p) = progress {
            clear_rehydrate_progress(p);
        }
        eprintln!(
            "   [disk-utxo] import done in {:.1}s — loaded {loaded} (header {count})",
            overall.elapsed().as_secs_f64()
        );
        self.len = loaded;
        self.utxo_tip_height = Some(checkpoint_height);
        Ok(count)
    }

    fn push_flushed_cache(&mut self, batch: Arc<FxHashMap<OutPoint, OverlayEntry>>) {
        if batch.is_empty() || self.skip_lmdb {
            return;
        }
        self.cache_entries = self.cache_entries.saturating_add(batch.len());
        self.cache_gens.push_front(batch);
        while self.cache_entries > self.cache_cap && self.cache_gens.len() > 1 {
            if let Some(old) = self.cache_gens.pop_back() {
                self.cache_entries = self.cache_entries.saturating_sub(old.len());
            } else {
                break;
            }
        }
    }

    fn gen_live(&self, op: &OutPoint) -> Option<bool> {
        for g in &self.cache_gens {
            if let Some(e) = g.get(op) {
                return Some(e.is_some());
            }
        }
        None
    }

    pub fn prefetch(&mut self, outpoints: &[OutPoint]) -> Result<UtxoSet> {
        let t_all = Instant::now();
        let mut set = utxo_set_with_capacity(outpoints.len());
        let mut need_db: Vec<OutPoint> = Vec::with_capacity(outpoints.len());
        {
            let pending = self.pending_flush.lock().unwrap();
            let pf = pending.as_ref().map(|a| a.as_ref());
            for op in outpoints {
                if let Some(e) = self.overlay.get(op) {
                    self.stats_ov += 1;
                    if let Some(arc) = e {
                        set.insert(*op, Arc::clone(arc));
                    }
                    continue;
                }
                if let Some(p) = pf {
                    if let Some(e) = p.get(op) {
                        self.stats_cache += 1;
                        if let Some(arc) = e {
                            set.insert(*op, Arc::clone(arc));
                        }
                        continue;
                    }
                }
                if let Some(packed) = self.resident.packed(op) {
                    self.stats_res += 1;
                    set.insert(*op, Arc::new(self.packed_to_utxo(packed)?));
                    continue;
                }
                if self.skip_lmdb {
                    need_db.push(*op);
                    continue;
                }
                let mut hit_gen = false;
                for g in &self.cache_gens {
                    if let Some(e) = g.get(op) {
                        self.stats_cache += 1;
                        if let Some(arc) = e {
                            set.insert(*op, Arc::clone(arc));
                        }
                        hit_gen = true;
                        break;
                    }
                }
                if !hit_gen {
                    need_db.push(*op);
                }
            }
        }
        if !need_db.is_empty() && !self.skip_lmdb {
            need_db.sort_unstable_by(|a, b| outpoint_key(a).cmp(&outpoint_key(b)));
            need_db.dedup();
            self.stats_db = self.stats_db.saturating_add(need_db.len() as u64);
            let t0 = Instant::now();
            let rtxn = self.env.read_txn().context("heed3 prefetch read_txn")?;
            let recache = self.resident.len() < self.cache_cap;
            for op in &need_db {
                let k = outpoint_key(op);
                if let Some(data) = self.db.get(&rtxn, k.as_slice())? {
                    let utxo = utxo_from_bytes(data)?;
                    self.stats_db_hit += 1;
                    if recache {
                        // !skip_lmdb: RAM spill only (pack_for_hot would mut-borrow over rtxn).
                        let packed =
                            PackedCoin::pack(&utxo, &mut self.spill, &mut self.spill_free);
                        self.resident.insert(*op, packed);
                    }
                    set.insert(*op, Arc::new(utxo));
                }
            }
            self.stats_db_ms += t0.elapsed().as_secs_f64() * 1000.0;
        } else if !need_db.is_empty() && self.skip_lmdb {
            self.resolve_cold(&mut set, &mut need_db)?;
        }
        self.stats_prefetch_ms += t_all.elapsed().as_secs_f64() * 1000.0;
        Ok(set)
    }

    /// Coins that are not in the hot slab. Pending spill, then tombstones, then
    /// evicted coin bodies, then the original read-only heed file.
    ///
    /// The disk lookups for one block are independent. A handful run one after
    /// another; a larger set shares the sealed runs and reads several pages at once.
    fn resolve_cold(&mut self, set: &mut UtxoSet, need_db: &mut Vec<OutPoint>) -> Result<()> {
        let t_cold = Instant::now();
        let mut still = Vec::with_capacity(need_db.len());
        let mut disk: Vec<OutPoint> = Vec::new();
        // Evicted spilled=2 coins live here until flush_spill; must be visible to prefetch.
        let pending_disk: FxHashMap<[u8; TOMB_REC], (u64, u32)> = self
            .pending_disk_ix
            .iter()
            .map(|rec| {
                let mut key = [0u8; TOMB_REC];
                key.copy_from_slice(&rec[..TOMB_REC]);
                (key, (coin_ix_off(rec), coin_ix_len(rec)))
            })
            .collect();
        for op in need_db.iter() {
            if let Some(bytes) = self.pending_coins.get(op).cloned() {
                let utxo = utxo_from_bytes(&bytes)?;
                self.stats_cache += 1;
                set.insert(*op, Arc::new(utxo));
                continue;
            }
            if self.pending_tombs.contains(op) {
                continue;
            }
            if let Some(&(off, len)) = pending_disk.get(&outpoint_key(op)) {
                let bytes = self.read_coin_body(off, len)?;
                let utxo = utxo_from_bytes(&bytes)?;
                self.stats_cache += 1;
                set.insert(*op, Arc::new(utxo));
                continue;
            }
            disk.push(*op);
        }
        if !disk.is_empty() {
            let (tombs, coins) = {
                let guard = self.compact.0.lock().unwrap();
                (guard.tombs.clone(), guard.coins.clone())
            };
            let threads = spill_read_threads();
            let hits = if threads == 1 || disk.len() < 8 {
                let mut hits = Vec::with_capacity(disk.len());
                for op in &disk {
                    hits.push(self.cold_disk(op, &tombs, &coins)?);
                }
                hits
            } else {
                par_map(disk.len(), threads, &|i| {
                    self.cold_disk(&disk[i], &tombs, &coins)
                })?
            };
            for (op, hit) in disk.into_iter().zip(hits) {
                match hit {
                    ColdDisk::Spent => {}
                    ColdDisk::Body(bytes) => {
                        let utxo = utxo_from_bytes(&bytes)?;
                        self.stats_cache += 1;
                        set.insert(op, Arc::new(utxo));
                    }
                    ColdDisk::Miss => still.push(op),
                }
            }
        }
        *need_db = still;
        if need_db.is_empty() {
            self.stats_cold_ms += t_cold.elapsed().as_secs_f64() * 1000.0;
            return Ok(());
        }
        if self.heed_off {
            // Cold snapshot coins were sealed into spill. A miss here is absent.
            self.stats_cold_ms += t_cold.elapsed().as_secs_f64() * 1000.0;
            return Ok(());
        }
        need_db.sort_unstable_by(|a, b| outpoint_key(a).cmp(&outpoint_key(b)));
        need_db.dedup();
        self.stats_db = self.stats_db.saturating_add(need_db.len() as u64);
        let t0 = Instant::now();
        let env = Arc::clone(&self.env);
        let db = self.db;
        let rtxn = env.read_txn().context("heed3 cold read_txn")?;
        for op in need_db.iter() {
            let k = outpoint_key(op);
            if let Some(data) = db.get(&rtxn, k.as_slice())? {
                let utxo = utxo_from_bytes(data)?;
                self.stats_db_hit += 1;
                set.insert(*op, Arc::new(utxo));
            }
        }
        self.stats_db_ms += t0.elapsed().as_secs_f64() * 1000.0;
        self.stats_cold_ms += t_cold.elapsed().as_secs_f64() * 1000.0;
        Ok(())
    }

    /// Copy every snapshot coin into sealed spill (including current hot), then
    /// leave heed alone for the rest of the run.
    fn seal_cold_from_heed(&mut self) -> Result<()> {
        if self.heed_off || !self.skip_lmdb {
            return Ok(());
        }
        let t0 = Instant::now();
        eprintln!(
            "   [disk-utxo] sealing ALL snapshot coins into spill ({} heed rows, hot {})",
            self.heed_len,
            self.resident.len()
        );
        let env = Arc::clone(&self.env);
        let db = self.db;
        let rtxn = env.read_txn().context("heed3 seal read_txn")?;
        let mut batch: Vec<[u8; COIN_IX]> = Vec::with_capacity(1_000_000);
        let mut sealed = 0u64;
        let mut scanned = 0u64;
        for item in db.iter(&rtxn).context("heed3 seal iter")? {
            let (k, v) = item.context("heed3 seal item")?;
            if is_disk_utxo_meta_key(k) || k.len() < 36 {
                continue;
            }
            scanned += 1;
            let op = outpoint_from_key(k);
            let key = outpoint_key(&op);
            let len = u32::try_from(v.len()).context("coin longer than 4 GiB")?;
            let off = self.coins_len;
            {
                let file = self.coins_file.as_mut().context("coins file")?;
                file.write_all(v)?;
            }
            self.coins_len += v.len() as u64;
            let mut rec = [0u8; COIN_IX];
            rec[..TOMB_REC].copy_from_slice(&key);
            rec[36..44].copy_from_slice(&off.to_le_bytes());
            rec[44..48].copy_from_slice(&len.to_le_bytes());
            bloom_insert(&mut self.coin_bloom, &key);
            batch.push(rec);
            sealed += 1;
            if batch.len() >= 1_000_000 {
                self.publish_coin_batch(&mut batch)?;
            }
            if sealed % 5_000_000 == 0 && sealed > 0 {
                eprintln!(
                    "   [disk-utxo] sealed {sealed} cold coins ({:.0}s)",
                    t0.elapsed().as_secs_f64()
                );
            }
        }
        drop(rtxn);
        if !batch.is_empty() {
            self.publish_coin_batch(&mut batch)?;
        }
        if let Some(file) = self.coins_file.as_mut() {
            file.flush()?;
            file.sync_all().ok();
        }
        // Hot held only snapshot coins from warm; they are now in sealed spill.
        self.resident = ResidentCoins::with_capacity(self.hot_cap);
        self.spill.clear();
        self.spill_free.clear();
        self.heed_off = true;
        self.seal_complete = true;
        lmdb_madvise_dontneed(&self.env_path);
        drop_clean_mdb_cache(&self.env_path);
        self.persist_overlay_manifest()?;
        eprintln!(
            "   [disk-utxo] heed off: sealed {sealed} coins (scanned {scanned}) in {:.1}s (seal_complete)",
            t0.elapsed().as_secs_f64()
        );
        Ok(())
    }

    fn publish_coin_batch(&mut self, batch: &mut Vec<[u8; COIN_IX]>) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        batch.sort_unstable_by(|a, b| {
            a[..TOMB_REC]
                .cmp(&b[..TOMB_REC])
                .then(coin_ix_off(b).cmp(&coin_ix_off(a)))
        });
        batch.dedup_by(|a, b| a[..TOMB_REC] == b[..TOMB_REC]);
        let id = {
            let mut guard = self.compact.0.lock().unwrap();
            let id = guard.next_coin;
            guard.next_coin = guard.next_coin.saturating_add(1);
            id
        };
        let path = self.spill_path.join(format!("coins-{id}.idx"));
        {
            let mut out = File::create(&path).with_context(|| format!("create {}", path.display()))?;
            for rec in batch.iter() {
                out.write_all(rec)?;
            }
            out.flush()?;
            out.sync_all().ok();
        }
        let file = File::open(&path)?;
        let sealed = seal_coin_run(path, file, batch.len() as u64, None)?;
        {
            let mut guard = self.compact.0.lock().unwrap();
            guard.coins.push(sealed);
        }
        self.wake_compact();
        batch.clear();
        Ok(())
    }

    /// Write the current spill set (including `heed_off`) without rewriting the hot file.
    fn persist_overlay_manifest(&mut self) -> Result<()> {
        let resident = self
            .spill_path
            .join("MANIFEST")
            .exists()
            .then(|| fs::read_to_string(self.spill_path.join("MANIFEST")).ok())
            .flatten()
            .and_then(|text| parse_manifest(&text).ok())
            .and_then(|m| m.resident);
        let manifest = {
            let guard = self.compact.0.lock().unwrap();
            OverlayManifest {
                snapshot_tip: self.snapshot_tip,
                tip: self.inflight_tip.unwrap_or(self.snapshot_tip),
                len: self.len,
                coins_len: self.coins_len,
                next_tomb: guard.next_tomb,
                next_coin: guard.next_coin,
                resident,
                heed_off: self.heed_off,
                seal_complete: self.seal_complete,
                tombs: guard
                    .tombs
                    .iter()
                    .map(|run| (run_file_name(&run.path), run.count()))
                    .collect(),
                coins: guard
                    .coins
                    .iter()
                    .map(|run| (run_file_name(&run.path), run.count()))
                    .collect(),
            }
        };
        write_manifest_file(&self.spill_path, &manifest)
    }

    /// Tomb runs, then the coin index, then the coin body. `tombs` and `coins`
    /// are a snapshot so several inputs can read them at once.
    fn cold_disk(
        &self,
        op: &OutPoint,
        tombs: &[TombRun],
        coins: &[CoinRun],
    ) -> Result<ColdDisk> {
        let key = outpoint_key(op);
        if bloom_maybe(&self.tomb_bloom, &key)
            && matches!(spill_lookup(tombs, &[], &key)?, SpillLookup::Spent)
        {
            return Ok(ColdDisk::Spent);
        }
        if bloom_maybe(&self.coin_bloom, &key) {
            if let SpillLookup::Coin(off, len) = spill_lookup(&[], coins, &key)? {
                return Ok(ColdDisk::Body(self.read_coin_body(off, len)?));
            }
        }
        Ok(ColdDisk::Miss)
    }

    fn consider_hot(&mut self, op: OutPoint, utxo: &UTXO, spill_new: bool) -> Result<()> {
        let height = u32::try_from(utxo.height).unwrap_or(u32::MAX);
        let over = self.resident.len() >= self.hot_cap;
        if over && height <= self.resident.oldest_height().unwrap_or(0) {
            if spill_new && utxo.height > self.snapshot_tip {
                let mut buf = Vec::new();
                utxo_to_bytes_into(utxo, &mut buf);
                self.pending_coins.insert(op, buf);
            }
            return Ok(());
        }
        if let Some(old) = self.resident.packed(&op) {
            free_packed_spill(&old, &mut self.spill_free);
        }
        let packed = self.pack_for_hot(utxo)?;
        self.resident.insert(op, packed);
        while self.resident.len() > self.hot_cap {
            self.evict_oldest()?;
        }
        Ok(())
    }

    fn evict_oldest(&mut self) -> Result<()> {
        let Some((op, packed)) = self.resident.pop_oldest() else {
            return Ok(());
        };
        if u64::from(packed.height) > self.snapshot_tip {
            if let Some((off, len)) = packed.disk_off() {
                self.pending_disk_ix.push(Self::coin_ix_rec(&op, off, len));
            } else {
                let utxo = self.packed_to_utxo(packed)?;
                free_packed_spill(&packed, &mut self.spill_free);
                let mut buf = Vec::new();
                utxo_to_bytes_into(&utxo, &mut buf);
                self.pending_coins.insert(op, buf);
            }
        } else {
            free_packed_spill(&packed, &mut self.spill_free);
        }
        Ok(())
    }

    fn note_tomb(&mut self, op: OutPoint) {
        self.pending_coins.remove(&op);
        self.pending_disk_ix
            .retain(|rec| rec[..TOMB_REC] != outpoint_key(&op));
        self.pending_tombs.insert(op);
    }

    fn flush_spill(&mut self) -> Result<()> {
        if self.pending_tombs.is_empty()
            && self.pending_coins.is_empty()
            && self.pending_disk_ix.is_empty()
        {
            return Ok(());
        }
        self.wait_tiers_settled();
        if !self.pending_tombs.is_empty() {
            let mut keys: Vec<[u8; TOMB_REC]> = self
                .pending_tombs
                .drain()
                .map(|op| outpoint_key(&op))
                .collect();
            keys.sort_unstable();
            keys.dedup();
            let id = {
                let mut guard = self.compact.0.lock().unwrap();
                let id = guard.next_tomb;
                guard.next_tomb = guard.next_tomb.saturating_add(1);
                id
            };
            let path = self.spill_path.join(format!("tombs-{id}.bin"));
            let mut out = File::create(&path).with_context(|| format!("create {}", path.display()))?;
            for k in &keys {
                bloom_insert(&mut self.tomb_bloom, k);
                out.write_all(k)?;
            }
            out.flush()?;
            out.sync_all().ok();
            let file = File::open(&path)?;
            let sealed = seal_tomb_run(path, file, keys.len() as u64, None)?;
            {
                let mut guard = self.compact.0.lock().unwrap();
                guard.tombs.push(sealed);
            }
            self.wake_compact();
        }
        if !self.pending_coins.is_empty() || !self.pending_disk_ix.is_empty() {
            let coins = self.pending_coins.drain().collect::<Vec<_>>();
            let mut recs: Vec<[u8; COIN_IX]> =
                Vec::with_capacity(coins.len() + self.pending_disk_ix.len());
            recs.append(&mut self.pending_disk_ix);
            {
                let file = self.coins_file.as_mut().context("coins file")?;
                for (op, bytes) in coins {
                    if self.pending_tombs.contains(&op) {
                        continue;
                    }
                    let len = u32::try_from(bytes.len()).context("coin longer than 4 GiB")?;
                    let off = self.coins_len;
                    file.write_all(&bytes)?;
                    self.coins_len += bytes.len() as u64;
                    recs.push(Self::coin_ix_rec(&op, off, len));
                }
            }
            if !recs.is_empty() {
                recs.sort_unstable_by(|a, b| {
                    a[..TOMB_REC]
                        .cmp(&b[..TOMB_REC])
                        .then(coin_ix_off(b).cmp(&coin_ix_off(a)))
                });
                recs.dedup_by(|a, b| a[..TOMB_REC] == b[..TOMB_REC]);
                let id = {
                    let mut guard = self.compact.0.lock().unwrap();
                    let id = guard.next_coin;
                    guard.next_coin = guard.next_coin.saturating_add(1);
                    id
                };
                let path = self.spill_path.join(format!("coins-{id}.idx"));
                let mut out =
                    File::create(&path).with_context(|| format!("create {}", path.display()))?;
                for rec in &recs {
                    let mut key = [0u8; TOMB_REC];
                    key.copy_from_slice(&rec[..TOMB_REC]);
                    bloom_insert(&mut self.coin_bloom, &key);
                    out.write_all(rec)?;
                }
                out.flush()?;
                out.sync_all().ok();
                let file = File::open(&path)?;
                let sealed = seal_coin_run(path, file, recs.len() as u64, None)?;
                {
                    let mut guard = self.compact.0.lock().unwrap();
                    guard.coins.push(sealed);
                }
                self.wake_compact();
            }
        }
        Ok(())
    }

    pub fn apply_block_delta(
        &mut self,
        pre_outpoints: &[OutPoint],
        post_set: &UtxoSet,
        prefetched_keys: &FxHashSet<OutPoint>,
    ) -> Result<()> {
        let result = self.apply_block_delta_inner(pre_outpoints, post_set, prefetched_keys);
        if result.is_err() {
            // Do not Drop-checkpoint a half-applied tip (spends committed, creates missing).
            self.checkpoint_on_drop = false;
        } else {
            self.checkpoint_on_drop = true;
        }
        result
    }

    /// Apply one block, then record its height. The tip moves only after apply
    /// succeeds, so a failed apply cannot be resumed as a committed block.
    pub fn apply_block_and_advance_tip(
        &mut self,
        height: u64,
        pre_outpoints: &[OutPoint],
        post_set: &UtxoSet,
        prefetched_keys: &FxHashSet<OutPoint>,
    ) -> Result<()> {
        self.apply_block_delta(pre_outpoints, post_set, prefetched_keys)?;
        self.set_inflight_tip(height);
        Ok(())
    }

    fn apply_block_delta_inner(
        &mut self,
        pre_outpoints: &[OutPoint],
        post_set: &UtxoSet,
        prefetched_keys: &FxHashSet<OutPoint>,
    ) -> Result<()> {
        if self.skip_lmdb {
            // Pack all new coins first. Only then commit spends/tombs + inserts so a
            // pack failure cannot seal a tip that spent inputs but never created outputs.
            let mut sorted_pre: Vec<&OutPoint> = pre_outpoints.iter().collect();
            sorted_pre.sort_unstable_by(|a, b| a.hash.cmp(&b.hash).then(a.index.cmp(&b.index)));
            let mut packed_new: Vec<(OutPoint, PackedCoin)> = Vec::new();
            for (op, utxo) in post_set.iter() {
                if sorted_pre
                    .binary_search_by(|p| p.hash.cmp(&op.hash).then(p.index.cmp(&op.index)))
                    .is_ok()
                {
                    continue;
                }
                packed_new.push((*op, self.pack_for_hot(utxo)?));
            }
            for op in pre_outpoints {
                if post_set.contains_key(op) {
                    continue;
                }
                if let Some(packed) = self.resident.packed(op) {
                    let h = packed.height;
                    free_packed_spill(&packed, &mut self.spill_free);
                    self.resident.remove(op);
                    self.len = self.len.saturating_sub(1);
                    if u64::from(h) <= self.snapshot_tip {
                        self.note_tomb(*op);
                    }
                } else {
                    self.note_tomb(*op);
                    self.len = self.len.saturating_sub(1);
                }
            }
            for (op, packed) in packed_new {
                if let Some(old) = self.resident.packed(&op) {
                    free_packed_spill(&old, &mut self.spill_free);
                }
                self.resident.insert(op, packed);
                self.len += 1;
                while self.resident.len() > self.hot_cap {
                    self.evict_oldest()?;
                }
            }
            if self.pending_tombs.len() >= 65_536
                || self.pending_coins.len() + self.pending_disk_ix.len() >= 4_096
            {
                self.flush_spill()?;
            }
            self.maybe_checkpoint()?;
            return Ok(());
        }
        let pending = self.pending_flush.lock().unwrap();
        for op in pre_outpoints {
            if !post_set.contains_key(op) {
                let was_live = match self.overlay.insert(*op, None) {
                    Some(Some(_)) => true,
                    Some(None) => false,
                    None => {
                        let in_pending_live = pending
                            .as_ref()
                            .and_then(|p| p.get(op))
                            .map(|e| e.is_some());
                        in_pending_live
                            .or_else(|| self.gen_live(op))
                            .unwrap_or(false)
                            || self.resident.contains(op)
                            || prefetched_keys.contains(op)
                    }
                };
                if was_live {
                    self.len = self.len.saturating_sub(1);
                }
                if let Some(packed) = self.resident.packed(op) {
                    free_packed_spill(&packed, &mut self.spill_free);
                }
                self.resident.remove(op);
            }
        }
        drop(pending);

        let mut sorted_pre: Vec<&OutPoint> = pre_outpoints.iter().collect();
        sorted_pre.sort_unstable_by(|a, b| a.hash.cmp(&b.hash).then(a.index.cmp(&b.index)));
        for (op, utxo) in post_set.iter() {
            if sorted_pre
                .binary_search_by(|p| p.hash.cmp(&op.hash).then(p.index.cmp(&op.index)))
                .is_ok()
            {
                continue;
            }
            self.overlay.insert(*op, Some(Arc::clone(utxo)));
            if let Some(old) = self.resident.packed(op) {
                free_packed_spill(&old, &mut self.spill_free);
            }
            let packed = self.pack_for_hot(utxo)?;
            self.resident.insert(*op, packed);
            self.len += 1;
        }

        // Dirty overlay is persistence only; resident is the lookup map. Spill so
        // SIGTERM does not write tens of millions of keys in one txn.
        if self.overlay.len() >= overlay_cap() {
            self.flush_overlay(false)?;
        }

        Ok(())
    }

    fn join_pending_flush(&mut self) -> Result<()> {
        if let Some(handle) = self.flush_thread.take() {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("flush thread panicked"))??;
            *self.pending_flush.lock().unwrap() = None;
            #[cfg(feature = "low-mem-alloc")]
            unsafe {
                libmimalloc_sys::mi_collect(true);
            }
        }
        Ok(())
    }

    pub fn join_bg_checkpoint(&mut self) -> Result<()> {
        if let Some(handle) = self.bg_checkpoint.take() {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("bg checkpoint thread panicked"))??;
        }
        Ok(())
    }

    pub fn flush_overlay(&mut self, durable: bool) -> Result<()> {
        if self.skip_lmdb {
            if durable {
                self.checkpoint_spill()?;
            }
            return Ok(());
        }
        if self.overlay.is_empty() {
            if durable {
                self.env.force_sync().ok();
            }
            return Ok(());
        }
        if !durable {
            // Never join on the connect path. A 1M–2M heed write_txn is slower than
            // overlay production; waiting for it is what dropped cruise from ~5 blk/s
            // to ~1.7. Drain at most overlay_cap per spawn so batches stay bounded.
            if let Some(h) = &self.flush_thread {
                if !h.is_finished() {
                    return Ok(());
                }
            }
        }
        self.join_pending_flush()?;
        // Commit the whole dirty set in one transaction. A partial batch used
        // to store the in-memory length and tip while the rest of the overlay
        // was still uncommitted, so a crash resumed on a short UTXO set.
        let entries = std::mem::take(&mut self.overlay);
        let n_entries = entries.len();
        let len_snapshot = self.len;
        let tip_snapshot = self.inflight_tip;
        if let Some(h) = tip_snapshot {
            eprintln!(
                "   [disk-utxo] flush overlay: {n_entries} entries at height {h} (heed3 total {len_snapshot}, cache {})",
                self.cache_entries.saturating_add(n_entries)
            );
        }
        let shared = Arc::new(entries);
        self.push_flushed_cache(Arc::clone(&shared));
        if durable {
            Self::write_flush_batch(&self.env, self.db, &shared, len_snapshot, tip_snapshot)?;
            self.env.force_sync().context("heed3 durable force_sync")?;
            drop_clean_mdb_cache(&self.env_path);
            self.utxo_tip_height = tip_snapshot;
            #[cfg(feature = "low-mem-alloc")]
            unsafe {
                libmimalloc_sys::mi_collect(true);
            }
        } else {
            *self.pending_flush.lock().unwrap() = Some(Arc::clone(&shared));
            let env = Arc::clone(&self.env);
            let db = self.db;
            let pf = Arc::clone(&self.pending_flush);
            let cache_path = self.env_path.clone();
            self.flush_thread = Some(std::thread::spawn(move || {
                let result = Self::write_flush_batch(&env, db, &shared, len_snapshot, tip_snapshot);
                drop_clean_mdb_cache(&cache_path);
                *pf.lock().unwrap() = None;
                result
            }));
            self.utxo_tip_height = tip_snapshot;
        }
        Ok(())
    }

    fn write_flush_batch(
        env: &HeedEnv,
        db: ByteDb,
        entries: &FxHashMap<OutPoint, OverlayEntry>,
        len: u64,
        tip: Option<u64>,
    ) -> Result<()> {
        let mut wtxn = env.write_txn().context("heed3 flush write_txn")?;
        let mut vbuf = Vec::with_capacity(128);
        let mut ordered: Vec<(&OutPoint, &OverlayEntry)> = entries.iter().collect();
        ordered.sort_unstable_by_key(|(op, _)| outpoint_key(op));
        for (op, entry) in ordered {
            let k = outpoint_key(op);
            match entry {
                Some(utxo) => {
                    utxo_to_bytes_into(utxo.as_ref(), &mut vbuf);
                    db.put(&mut wtxn, k.as_slice(), vbuf.as_slice())?;
                }
                None => {
                    let _ = db.delete(&mut wtxn, k.as_slice())?;
                }
            }
        }
        db.put(&mut wtxn, META_LEN_KEY, &len.to_le_bytes())?;
        if let Some(h) = tip {
            db.put(&mut wtxn, META_TIP_KEY, &h.to_le_bytes())?;
        }
        wtxn.commit().context("heed3 flush commit")?;
        Ok(())
    }

    pub fn save_checkpoint_fixed_v1(&mut self, height: u64, path: &Path) -> Result<()> {
        self.join_bg_checkpoint()?;
        self.join_pending_flush()?;
        self.flush_overlay(true)?;
        let env = Arc::clone(&self.env);
        let db = self.db;
        let path = path.to_path_buf();
        let len = self.len;
        self.bg_checkpoint = Some(std::thread::spawn(move || {
            Self::write_checkpoint_file_from_db(&env, db, height, len, &path)
        }));
        Ok(())
    }

    fn write_checkpoint_file_from_db(
        env: &HeedEnv,
        db: ByteDb,
        height: u64,
        len: u64,
        path: &Path,
    ) -> Result<()> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("heed3.part");
        let file =
            std::fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        let mut w = std::io::BufWriter::with_capacity(checkpoint_write_buf_capacity(), file);
        w.write_all(crate::utxo_snapshot_fixed_v1::FIXED_V1_MAGIC)?;
        w.write_all(&crate::utxo_snapshot_fixed_v1::FIXED_V1_FORMAT_VERSION.to_le_bytes())?;
        w.write_all(&height.to_le_bytes())?;
        w.write_all(&0u64.to_le_bytes())?;
        let mut written: u64 = 0;
        {
            let rtxn = env.read_txn()?;
            for item in db.iter(&rtxn)? {
                let (k, v) = item?;
                if is_disk_utxo_meta_key(k) || k.len() != 36 {
                    continue;
                }
                let op = outpoint_from_key(k);
                let utxo = utxo_from_bytes(v)?;
                write_fixed_v1_entry_raw(&mut w, &op, &utxo)?;
                written += 1;
            }
        }
        w.flush()?;
        use std::io::{Seek, SeekFrom, Write as _};
        let mut inner = w.into_inner().map_err(|e| anyhow::anyhow!("flush: {e}"))?;
        inner.seek(SeekFrom::Start(20))?;
        inner.write_all(&written.to_le_bytes())?;
        drop(inner);
        if written != len {
            eprintln!(
                "   [disk-utxo] note: physical entries {written} vs tracked len {len}"
            );
        }
        if path.exists() {
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_readonly(false);
            std::fs::set_permissions(path, perms)?;
        }
        std::fs::rename(&tmp, path)?;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(path, perms)?;
        eprintln!(
            "   [disk-utxo] wrote {} (height {height}, {written} entries)",
            path.display()
        );
        Ok(())
    }

    pub fn compact(&mut self, compact_status: Option<&Path>) -> Result<()> {
        self.join_bg_checkpoint()?;
        self.join_pending_flush()?;
        self.flush_overlay(true)?;
        let before = self.db_file_bytes();
        let start_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Some(p) = compact_status {
            write_compact_status(p, before, start_ts);
        }
        let packed = packed_copy_path(&self.env_path);
        match compact_env_to_file(&self.env, &self.env_path, &packed, before) {
            Ok(packed_sz) => {
                eprintln!(
                    "   [disk-utxo] compacted copy ready ({:.1} GiB → {:.1} GiB at {}); installed on next open",
                    before as f64 / (1024.0 * 1024.0 * 1024.0),
                    packed_sz as f64 / (1024.0 * 1024.0 * 1024.0),
                    packed.display()
                );
            }
            Err(e) => {
                eprintln!("   [disk-utxo] compact copy failed: {e:#}");
            }
        }
        if let Some(p) = compact_status {
            clear_compact_status(p);
        }
        Ok(())
    }

    pub fn db_file_bytes(&self) -> u64 {
        self.env_path
            .join("data.mdb")
            .metadata()
            .map(|m| m.len())
            .unwrap_or(0)
    }
}

impl Drop for DiskUtxoSet {
    fn drop(&mut self) {
        self.stop_compact();
        let _ = self.join_bg_checkpoint();
        let _ = self.join_pending_flush();
        if self.checkpoint_on_drop {
            let _ = self.flush_overlay(true);
        } else {
            eprintln!(
                "   [disk-utxo] skip Drop checkpoint (apply failed; refuse to seal partial tip)"
            );
        }
    }
}

pub fn block_input_outpoints(block: &blvm_protocol::types::Block) -> Vec<OutPoint> {
    let total: usize = block
        .transactions
        .iter()
        .skip(1)
        .map(|tx| tx.inputs.len())
        .sum();
    let mut ops = Vec::with_capacity(total);
    for tx in block.transactions.iter().skip(1) {
        for input in &tx.inputs {
            ops.push(input.prevout);
        }
    }
    ops
}

fn write_fixed_v1_entry_raw<W: std::io::Write>(w: &mut W, op: &OutPoint, u: &UTXO) -> Result<()> {
    let script: &[u8] = u.script_pubkey.as_ref();
    w.write_all(&op.hash)?;
    w.write_all(&op.index.to_le_bytes())?;
    w.write_all(&u.value.to_le_bytes())?;
    w.write_all(&u.height.to_le_bytes())?;
    w.write_all(&[u.is_coinbase as u8])?;
    w.write_all(&(script.len() as u32).to_le_bytes())?;
    w.write_all(script)?;
    Ok(())
}

fn try_read_one_fixed_v1_entry<R: Read>(r: &mut R) -> Result<Option<(OutPoint, UTXO)>> {
    let mut hash = [0u8; 32];
    match r.read(&mut hash[..1]) {
        Ok(0) => return Ok(None),
        Ok(_) => r.read_exact(&mut hash[1..])?,
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let mut u4 = [0u8; 4];
    r.read_exact(&mut u4)?;
    let index = u32::from_le_bytes(u4);
    let mut u8b = [0u8; 8];
    r.read_exact(&mut u8b)?;
    let value = i64::from_le_bytes(u8b);
    r.read_exact(&mut u8b)?;
    let height = u64::from_le_bytes(u8b);
    let mut cb = [0u8; 1];
    r.read_exact(&mut cb)?;
    let is_coinbase = match cb[0] {
        0 => false,
        1 => true,
        b => bail!("fixed v1: invalid is_coinbase byte {b}"),
    };
    r.read_exact(&mut u4)?;
    let slen = u32::from_le_bytes(u4) as usize;
    let mut script = vec![0u8; slen];
    r.read_exact(&mut script)?;
    Ok(Some((
        OutPoint { hash, index },
        UTXO {
            value,
            script_pubkey: script.into(),
            height,
            is_coinbase,
        },
    )))
}

#[cfg(test)]
mod overlay_manifest_tests {
    use super::*;

    fn sample_manifest() -> OverlayManifest {
        OverlayManifest {
            snapshot_tip: 487954,
            tip: 489954,
            len: 55_000_010,
            coins_len: 4096,
            next_tomb: 3,
            next_coin: 1,
            resident: Some("resident-489954.bin".into()),
            heed_off: true,
            seal_complete: true,
            tombs: vec![("tombs-2.bin".into(), 100), ("tombs-4.bin".into(), 40)],
            coins: vec![("coins-0.idx".into(), 7)],
        }
    }

    #[test]
    fn tier_bloom_keeps_density_on_giant_runs() {
        let n = 300_000_000u64;
        let words = tier_bloom_words(n);
        // ~16 bits/key, uncapped (32 MiB would be only ~4M words).
        assert!(words * 8 >= 500 * 1024 * 1024);
        let mut bloom = tier_bloom(n);
        let key = [0xabu8; TOMB_REC];
        bloom_insert(&mut bloom, &key);
        assert!(bloom_maybe(&bloom, &key));
    }

    #[test]
    fn spill_probe_advice_is_accepted() {
        use std::os::fd::AsRawFd;
        let dir = std::env::temp_dir().join(format!("blvm-fadvise-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("probe.bin");
        {
            let mut f = File::create(&path).unwrap();
            f.write_all(&[7u8; 8192]).unwrap();
        }
        let file = File::open(&path).unwrap();
        let rc = unsafe { libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_RANDOM) };
        assert_eq!(rc, 0);
        let mut buf = [0u8; 36];
        read_at(&file, &mut buf, 100).unwrap();
        assert_eq!(buf[0], 7);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn disk_body_len_supports_over_u16() {
        let utxo = UTXO {
            value: 1,
            script_pubkey: vec![0x6a; 70_000].into(),
            height: 500_000,
            is_coinbase: false,
        };
        let body_len = 70_000u32 + 21; // value+height+flag+script_len prefix
        let packed = PackedCoin::from_disk_body(&utxo, 1_000_000, body_len).unwrap();
        assert_eq!(packed.spilled, 2);
        assert_eq!(packed.disk_off(), Some((1_000_000, body_len)));
    }

    #[test]
    fn ram_spill_script_len_supports_over_u16() {
        let script = vec![0x51; 70_000];
        let utxo = UTXO {
            value: 42,
            script_pubkey: script.clone().into(),
            height: 10,
            is_coinbase: false,
        };
        let mut spill = Vec::new();
        let mut free = Vec::new();
        let packed = PackedCoin::pack(&utxo, &mut spill, &mut free);
        assert_eq!(packed.spilled, 1);
        let got = packed.to_utxo(&spill);
        assert_eq!(got.script_pubkey.as_ref(), script.as_slice());
        free_packed_spill(&packed, &mut free);
        assert_eq!(free, vec![(0, 70_000)]);
    }

    #[test]
    fn failed_pack_leaves_spends_uncommitted() {
        unsafe {
            std::env::set_var("DISK_UTXO_WARM", "1");
            std::env::set_var("DISK_UTXO_HOT_MB", "64");
            std::env::set_var("DISK_UTXO_TOMB_BLOOM_MB", "8");
            std::env::set_var("DISK_UTXO_OVERLAY_CAP", "1000");
            std::env::set_var("DISK_UTXO_CACHE_CAP", "1000");
            std::env::set_var("DISK_UTXO_OVERLAY_CHECKPOINT_EVERY", "0");
        }
        let dir = std::env::temp_dir().join(format!(
            "blvm-atomic-apply-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let old = OutPoint {
            hash: [0x33; 32],
            index: 0,
        };
        let new_op = OutPoint {
            hash: [0x44; 32],
            index: 1,
        };
        let old_utxo = UTXO {
            value: 1_000,
            script_pubkey: vec![0x51].into(),
            height: 10,
            is_coinbase: true,
        };
        let new_utxo = UTXO {
            value: 900,
            script_pubkey: vec![0xAB; 40].into(),
            height: 12,
            is_coinbase: false,
        };
        {
            let seeded = DiskUtxoSet::open(&dir).unwrap();
            let mut body = Vec::new();
            utxo_to_bytes_into(&old_utxo, &mut body);
            let mut wtxn = seeded.env.write_txn().unwrap();
            seeded
                .db
                .put(&mut wtxn, outpoint_key(&old).as_slice(), &body)
                .unwrap();
            seeded
                .db
                .put(&mut wtxn, META_LEN_KEY, &1u64.to_le_bytes())
                .unwrap();
            seeded
                .db
                .put(&mut wtxn, META_TIP_KEY, &10u64.to_le_bytes())
                .unwrap();
            wtxn.commit().unwrap();
        }
        {
            let mut du = DiskUtxoSet::open(&dir).unwrap();
            assert!(du.resident.packed(&old).is_some() || du.len >= 1);
            // Ensure spend target is in resident (warm path).
            if du.resident.packed(&old).is_none() {
                du.consider_hot(old, &old_utxo, false).unwrap();
            }
            let tombs_before = du.pending_tombs.len();
            let had_old = du.resident.packed(&old).is_some();
            assert!(had_old);
            du.fail_next_pack_for_hot = true;
            let mut post = UtxoSet::default();
            post.insert(new_op, Arc::new(new_utxo));
            let prefetched: FxHashSet<OutPoint> = [old].into_iter().collect();
            let err = du
                .apply_block_delta(&[old], &post, &prefetched)
                .expect_err("pack failure must surface");
            assert!(err.to_string().contains("test-injected"));
            assert!(
                du.resident.packed(&old).is_some(),
                "spend must not commit when pack fails"
            );
            assert_eq!(du.pending_tombs.len(), tombs_before);
            assert!(!du.checkpoint_on_drop);
            // Drop must not seal a poisoned tip.
        }
        let _ = fs::remove_dir_all(&dir);
        unsafe {
            std::env::remove_var("DISK_UTXO_WARM");
            std::env::remove_var("DISK_UTXO_HOT_MB");
            std::env::remove_var("DISK_UTXO_TOMB_BLOOM_MB");
            std::env::remove_var("DISK_UTXO_OVERLAY_CAP");
            std::env::remove_var("DISK_UTXO_CACHE_CAP");
            std::env::remove_var("DISK_UTXO_OVERLAY_CHECKPOINT_EVERY");
        }
    }

    #[test]
    fn prefetch_finds_pending_disk_ix_before_flush() {
        unsafe {
            std::env::set_var("DISK_UTXO_WARM", "1");
            std::env::set_var("DISK_UTXO_HOT_MB", "64");
            std::env::set_var("DISK_UTXO_TOMB_BLOOM_MB", "8");
            std::env::set_var("DISK_UTXO_OVERLAY_CAP", "1000");
            std::env::set_var("DISK_UTXO_CACHE_CAP", "1000");
            std::env::set_var("DISK_UTXO_OVERLAY_CHECKPOINT_EVERY", "0");
        }
        let dir = std::env::temp_dir().join(format!(
            "blvm-pending-disk-ix-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let seed = OutPoint {
            hash: [0x55; 32],
            index: 0,
        };
        let seed_utxo = UTXO {
            value: 1,
            script_pubkey: vec![0x51].into(),
            height: 10,
            is_coinbase: true,
        };
        {
            let seeded = DiskUtxoSet::open(&dir).unwrap();
            let mut body = Vec::new();
            utxo_to_bytes_into(&seed_utxo, &mut body);
            let mut wtxn = seeded.env.write_txn().unwrap();
            seeded
                .db
                .put(&mut wtxn, outpoint_key(&seed).as_slice(), &body)
                .unwrap();
            seeded
                .db
                .put(&mut wtxn, META_LEN_KEY, &1u64.to_le_bytes())
                .unwrap();
            seeded
                .db
                .put(&mut wtxn, META_TIP_KEY, &10u64.to_le_bytes())
                .unwrap();
            wtxn.commit().unwrap();
        }
        {
            let mut du = DiskUtxoSet::open(&dir).unwrap();
            assert!(du.heed_off && du.seal_complete);
            let op = OutPoint {
                hash: [0x66; 32],
                index: 2,
            };
            let utxo = UTXO {
                value: 42_000,
                script_pubkey: vec![0xAB; 40].into(),
                height: 12,
                is_coinbase: false,
            };
            du.consider_hot(op, &utxo, true).unwrap();
            let packed = du.resident.packed(&op).expect("hot insert");
            assert_eq!(packed.spilled, 2, "long post-snapshot script must be spilled=2");
            du.evict_oldest().unwrap();
            assert!(
                du.resident.packed(&op).is_none(),
                "evicted from resident"
            );
            assert!(
                !du.pending_disk_ix.is_empty(),
                "eviction must park index in pending_disk_ix"
            );
            let got = du.prefetch(&[op]).unwrap();
            let found = got.get(&op).expect("prefetch must see pending_disk_ix");
            assert_eq!(found.value, 42_000);
            assert_eq!(found.script_pubkey.as_ref(), &[0xAB; 40]);
            assert!(
                !du.pending_disk_ix.is_empty(),
                "prefetch must not require flush_spill"
            );
        }
        let _ = fs::remove_dir_all(&dir);
        unsafe {
            std::env::remove_var("DISK_UTXO_WARM");
            std::env::remove_var("DISK_UTXO_HOT_MB");
            std::env::remove_var("DISK_UTXO_TOMB_BLOOM_MB");
            std::env::remove_var("DISK_UTXO_OVERLAY_CAP");
            std::env::remove_var("DISK_UTXO_CACHE_CAP");
            std::env::remove_var("DISK_UTXO_OVERLAY_CHECKPOINT_EVERY");
        }
    }

    #[test]
    fn overlay_manifest_round_trip() {
        let manifest = sample_manifest();
        let parsed = parse_manifest(&format_manifest(&manifest)).unwrap();
        assert_eq!(parsed.snapshot_tip, manifest.snapshot_tip);
        assert_eq!(parsed.tip, manifest.tip);
        assert_eq!(parsed.len, manifest.len);
        assert_eq!(parsed.coins_len, manifest.coins_len);
        assert_eq!(parsed.next_tomb, manifest.next_tomb);
        assert_eq!(parsed.next_coin, manifest.next_coin);
        assert_eq!(parsed.resident, manifest.resident);
        assert_eq!(parsed.heed_off, manifest.heed_off);
        assert_eq!(parsed.seal_complete, manifest.seal_complete);
        assert_eq!(parsed.tombs, manifest.tombs);
        assert_eq!(parsed.coins, manifest.coins);
    }

    #[test]
    fn overlay_manifest_rejects_parent_path() {
        let text = "OVERLAY 1\nsnapshot 1\ntip 2\nlen 3\ncoins_len 0\nnext_tomb 0\nnext_coin 0\ntomb ../tombs-0.bin 1\n";
        assert!(parse_manifest(text).is_err());
    }

    #[test]
    fn overlay_load_restores_runs_and_drops_strays() {
        let dir = std::env::temp_dir().join(format!(
            "blvm-overlay-manifest-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let tomb = dir.join("tombs-0.bin");
        let mut tomb_bytes = Vec::new();
        for i in 0u32..3 {
            let mut rec = [0u8; TOMB_REC];
            rec[31] = i as u8;
            tomb_bytes.extend_from_slice(&rec);
        }
        fs::write(&tomb, &tomb_bytes).unwrap();
        fs::write(dir.join("coins.bin"), [0u8; 8]).unwrap();
        fs::write(dir.join("stray.bin"), b"gone").unwrap();
        let manifest = OverlayManifest {
            snapshot_tip: 10,
            tip: 12,
            len: 4,
            coins_len: 8,
            next_tomb: 1,
            next_coin: 0,
            resident: None,
            heed_off: false,
            seal_complete: false,
            tombs: vec![("tombs-0.bin".into(), 3)],
            coins: vec![],
        };
        fs::write(dir.join("MANIFEST"), format_manifest(&manifest)).unwrap();
        let loaded = load_overlay(&dir, 10).unwrap().expect("manifest");
        assert_eq!(loaded.tip, 12);
        assert_eq!(loaded.len, 4);
        assert_eq!(loaded.coins_len, 8);
        assert_eq!(loaded.runs.tombs.len(), 1);
        assert_eq!(loaded.runs.tombs[0].count(), 3);
        assert!(loaded.runs.coins.is_empty());
        assert!(!dir.join("stray.bin").exists());
        assert!(dir.join("tombs-0.bin").exists());
        assert!(load_overlay(&dir, 9).unwrap().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn overlay_restart_keeps_spends_and_new_coins() {
        unsafe {
            std::env::set_var("DISK_UTXO_WARM", "1");
            std::env::set_var("DISK_UTXO_HOT_MB", "64");
            std::env::set_var("DISK_UTXO_TOMB_BLOOM_MB", "32");
            std::env::set_var("DISK_UTXO_OVERLAY_CAP", "1000");
            std::env::set_var("DISK_UTXO_CACHE_CAP", "1000");
            std::env::set_var("DISK_UTXO_OVERLAY_CHECKPOINT_EVERY", "2000");
        }
        assert!(overlay_resume_checkpoint_due(488_000));
        assert!(!overlay_resume_checkpoint_due(487_954));
        assert!(!overlay_resume_checkpoint_due(0));
        unsafe {
            std::env::set_var("DISK_UTXO_OVERLAY_CHECKPOINT_EVERY", "0");
        }
        assert!(!overlay_resume_checkpoint_due(488_000));

        let dir = std::env::temp_dir().join(format!(
            "blvm-overlay-restart-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let old = OutPoint {
            hash: [0x11; 32],
            index: 0,
        };
        let new_op = OutPoint {
            hash: [0x22; 32],
            index: 7,
        };
        let old_utxo = UTXO {
            value: 50_0000_0000,
            script_pubkey: vec![0x51].into(),
            height: 10,
            is_coinbase: true,
        };
        let new_utxo = UTXO {
            value: 42_000,
            script_pubkey: vec![0xAB; 40].into(),
            height: 12,
            is_coinbase: false,
        };

        {
            let seeded = DiskUtxoSet::open(&dir).unwrap();
            let mut body = Vec::new();
            utxo_to_bytes_into(&old_utxo, &mut body);
            let mut wtxn = seeded.env.write_txn().unwrap();
            seeded
                .db
                .put(&mut wtxn, outpoint_key(&old).as_slice(), &body)
                .unwrap();
            seeded
                .db
                .put(&mut wtxn, META_LEN_KEY, &1u64.to_le_bytes())
                .unwrap();
            seeded
                .db
                .put(&mut wtxn, META_TIP_KEY, &10u64.to_le_bytes())
                .unwrap();
            wtxn.commit().unwrap();
        }

        {
            let mut du = DiskUtxoSet::open(&dir).unwrap();
            assert_eq!(du.utxo_tip_height(), Some(10));
            let mut post = UtxoSet::default();
            post.insert(new_op, Arc::new(new_utxo.clone()));
            let prefetched: FxHashSet<OutPoint> = [old].into_iter().collect();
            du.set_inflight_tip(12);
            du.apply_block_delta(&[old], &post, &prefetched).unwrap();
            du.checkpoint_spill().unwrap();
            assert_eq!(
                read_meta_u64(&du.env, &du.db, META_TIP_KEY).unwrap(),
                Some(10),
                "checkpoint must not write the heed tip"
            );
        }

        {
            let mut du = DiskUtxoSet::open(&dir).unwrap();
            assert_eq!(du.utxo_tip_height(), Some(12));
            let got_new = du.prefetch(&[new_op]).unwrap();
            let got = got_new.get(&new_op).expect("post-snapshot coin");
            assert_eq!(got.value, 42_000);
            assert_eq!(got.height, 12);
            assert_eq!(got.script_pubkey.as_ref(), &[0xAB; 40]);
            assert!(
                du.prefetch(&[old]).unwrap().get(&old).is_none(),
                "spent snapshot coin must stay spent"
            );
            du.set_inflight_tip(14);
            let prefetched: FxHashSet<OutPoint> = [new_op].into_iter().collect();
            du.apply_block_delta(&[new_op], &UtxoSet::default(), &prefetched)
                .unwrap();
            du.checkpoint_spill().unwrap();
        }

        {
            let mut du = DiskUtxoSet::open(&dir).unwrap();
            assert_eq!(du.utxo_tip_height(), Some(14));
            assert!(du.prefetch(&[new_op]).unwrap().get(&new_op).is_none());
            assert!(du.prefetch(&[old]).unwrap().get(&old).is_none());
            let resident = fs::read(du.spill_path.join("resident-14.bin")).unwrap();
            assert!(!resident.windows(32).any(|w| w == new_op.hash));
            assert!(!resident.windows(32).any(|w| w == old.hash));
        }

        let spill = dir.join("heed3-spill");
        fs::write(spill.join("resident-99.bin"), b"stray-hot-set").unwrap();
        fs::write(spill.join("MANIFEST.tmp"), b"OVERLAY 1\ntip 99\n").unwrap();
        {
            let mut du = DiskUtxoSet::open(&dir).unwrap();
            assert_eq!(du.utxo_tip_height(), Some(14));
            assert!(!spill.join("resident-99.bin").exists());
            assert!(!spill.join("MANIFEST.tmp").exists());
            assert!(spill.join("resident-14.bin").is_file());
            assert!(du.prefetch(&[old]).unwrap().get(&old).is_none());
        }

        fs::remove_file(spill.join("resident-14.bin")).unwrap();
        {
            let mut du = DiskUtxoSet::open(&dir).unwrap();
            assert_eq!(
                du.utxo_tip_height(),
                Some(10),
                "a manifest whose hot-coin file is missing resumes at the heed tip"
            );
            let got = du.prefetch(&[old]).unwrap();
            assert!(got.get(&old).is_some());
            assert!(du.prefetch(&[new_op]).unwrap().get(&new_op).is_none());
            assert_eq!(
                read_meta_u64(&du.env, &du.db, META_TIP_KEY).unwrap(),
                Some(10)
            );
        }

        let _ = fs::remove_dir_all(&dir);
        unsafe {
            std::env::remove_var("DISK_UTXO_WARM");
            std::env::remove_var("DISK_UTXO_HOT_MB");
            std::env::remove_var("DISK_UTXO_TOMB_BLOOM_MB");
            std::env::remove_var("DISK_UTXO_OVERLAY_CAP");
            std::env::remove_var("DISK_UTXO_CACHE_CAP");
            std::env::remove_var("DISK_UTXO_OVERLAY_CHECKPOINT_EVERY");
        }
    }

    fn key_n(n: u16) -> [u8; TOMB_REC] {
        let mut key = [0u8; TOMB_REC];
        key[34..].copy_from_slice(&n.to_be_bytes());
        key
    }

    #[test]
    fn par_map_keeps_every_result_in_order() {
        let got = par_map(32, 4, &|i| Ok(i * 3)).unwrap();
        assert_eq!(got, (0..32).map(|i| i * 3).collect::<Vec<_>>());
        let one = par_map(5, 1, &|i| Ok(i)).unwrap();
        assert_eq!(one, vec![0, 1, 2, 3, 4]);
    }

    fn probes_for(check: impl FnOnce()) -> u64 {
        PAGE_PROBES.store(0, Ordering::Relaxed);
        check();
        PAGE_PROBES.load(Ordering::Relaxed)
    }

    #[test]
    fn page_index_reads_one_page_at_each_boundary() {
        let dir = std::env::temp_dir().join(format!("blvm-page-index-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tombs-0.bin");
        let mut bytes = Vec::new();
        for n in 1..=TOMB_STRIDE {
            bytes.extend_from_slice(&key_n(n as u16));
        }
        bytes.extend_from_slice(&key_n(200));
        bytes.extend_from_slice(&key_n(201));
        fs::write(&path, &bytes).unwrap();
        let count = (TOMB_STRIDE + 2) as u64;
        let file = File::open(&path).unwrap();
        let run = seal_tomb_run(path, file, count, None).unwrap();

        assert_eq!(probes_for(|| assert!(tomb_page_has(&run, &key_n(1)).unwrap())), 1);
        assert_eq!(
            probes_for(|| assert!(tomb_page_has(&run, &key_n(TOMB_STRIDE as u16)).unwrap())),
            1
        );
        assert_eq!(probes_for(|| assert!(tomb_page_has(&run, &key_n(200)).unwrap())), 1);
        assert_eq!(probes_for(|| assert!(!tomb_page_has(&run, &key_n(150)).unwrap())), 1);
        assert_eq!(probes_for(|| assert!(!tomb_page_has(&run, &key_n(0)).unwrap())), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    fn coin_rec(key: &[u8; TOMB_REC], off: u64, len: u32) -> [u8; COIN_IX] {
        let mut rec = [0u8; COIN_IX];
        rec[..TOMB_REC].copy_from_slice(key);
        rec[36..44].copy_from_slice(&off.to_le_bytes());
        rec[44..48].copy_from_slice(&len.to_le_bytes());
        rec
    }

    #[test]
    fn newer_coin_run_wins_with_one_index_read() {
        let dir = std::env::temp_dir().join(format!("blvm-coin-tier-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let key = key_n(7);
        let older = dir.join("coins-0.idx");
        let newer = dir.join("coins-1.idx");
        fs::write(&older, coin_rec(&key, 10, 4)).unwrap();
        fs::write(&newer, coin_rec(&key, 80, 4)).unwrap();
        let old_run = seal_coin_run(older, File::open(dir.join("coins-0.idx")).unwrap(), 1, None).unwrap();
        let new_run = seal_coin_run(newer, File::open(dir.join("coins-1.idx")).unwrap(), 1, None).unwrap();
        let runs = [old_run, new_run];
        PAGE_PROBES.store(0, Ordering::Relaxed);
        match spill_lookup(&[], &runs, &key).unwrap() {
            SpillLookup::Coin(off, len) => {
                assert_eq!(off, 80);
                assert_eq!(len, 4);
            }
            other => panic!("expected the newer coin, got {other:?}"),
        }
        assert_eq!(PAGE_PROBES.load(Ordering::Relaxed), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tomb_hides_a_spilled_coin_body() {
        let dir = std::env::temp_dir().join(format!("blvm-tomb-hides-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let key = key_n(9);
        let tomb_path = dir.join("tombs-0.bin");
        let coin_path = dir.join("coins-0.idx");
        fs::write(&tomb_path, key).unwrap();
        fs::write(&coin_path, coin_rec(&key, 5, 1)).unwrap();
        let tomb = seal_tomb_run(tomb_path, File::open(dir.join("tombs-0.bin")).unwrap(), 1, None).unwrap();
        let coin = seal_coin_run(coin_path, File::open(dir.join("coins-0.idx")).unwrap(), 1, None).unwrap();
        assert!(matches!(
            spill_lookup(&[tomb], &[coin], &key).unwrap(),
            SpillLookup::Spent
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn seal_cold_keeps_coin_after_heed_off() {
        let dir = std::env::temp_dir().join(format!("blvm-seal-cold-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let op = OutPoint {
            hash: [0x44; 32],
            index: 1,
        };
        let utxo = UTXO {
            value: 9_000,
            script_pubkey: vec![0x51].into(),
            height: 5,
            is_coinbase: false,
        };
        {
            let seeded = DiskUtxoSet::open(&dir).unwrap();
            let mut body = Vec::new();
            utxo_to_bytes_into(&utxo, &mut body);
            let mut wtxn = seeded.env.write_txn().unwrap();
            seeded
                .db
                .put(&mut wtxn, outpoint_key(&op).as_slice(), &body)
                .unwrap();
            seeded
                .db
                .put(&mut wtxn, META_LEN_KEY, &1u64.to_le_bytes())
                .unwrap();
            seeded
                .db
                .put(&mut wtxn, META_TIP_KEY, &5u64.to_le_bytes())
                .unwrap();
            wtxn.commit().unwrap();
        }
        unsafe {
            std::env::set_var("DISK_UTXO_WARM", "1");
            std::env::set_var("DISK_UTXO_HOT_MB", "64");
            std::env::set_var("DISK_UTXO_TOMB_BLOOM_MB", "32");
            std::env::set_var("DISK_UTXO_OVERLAY_CHECKPOINT_EVERY", "0");
        }
        {
            // open() warms then seals ALL heed coins (including the one that was hot).
            let mut du = DiskUtxoSet::open(&dir).unwrap();
            assert!(du.heed_off);
            assert!(du.seal_complete);
            assert!(
                !du.resident.contains(&op),
                "seal clears snapshot hot after writing sealed spill"
            );
            let got = du.prefetch(&[op]).unwrap();
            let coin = got.get(&op).expect("sealed spill must serve the cold coin");
            assert_eq!(coin.value, 9_000);
            assert_eq!(
                read_meta_u64(&du.env, &du.db, META_TIP_KEY).unwrap(),
                Some(5),
                "seal must not write the heed tip"
            );
        }
        {
            let mut du = DiskUtxoSet::open(&dir).unwrap();
            assert!(du.heed_off);
            assert!(du.seal_complete);
            let text = fs::read_to_string(du.spill_path.join("MANIFEST")).unwrap();
            assert!(text.contains("heed_off 1"));
            assert!(text.contains("seal_complete 1"));
            let got = du.prefetch(&[op]).unwrap();
            assert!(
                got.get(&op).is_some(),
                "reopen with seal_complete must find coin without relying on warm hot slab"
            );
        }
        let _ = fs::remove_dir_all(&dir);
        unsafe {
            std::env::remove_var("DISK_UTXO_WARM");
            std::env::remove_var("DISK_UTXO_HOT_MB");
            std::env::remove_var("DISK_UTXO_TOMB_BLOOM_MB");
            std::env::remove_var("DISK_UTXO_OVERLAY_CHECKPOINT_EVERY");
        }
    }

    #[test]
    fn collapse_merges_many_tomb_runs_once() {
        let dir = std::env::temp_dir().join(format!("blvm-collapse-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (name, keys) in [
            ("tombs-0.bin", vec![1u16, 3]),
            ("tombs-1.bin", vec![2, 3]),
            ("tombs-2.bin", vec![4]),
        ] {
            let mut bytes = Vec::new();
            for n in keys {
                bytes.extend_from_slice(&key_n(n));
            }
            fs::write(dir.join(name), bytes).unwrap();
        }
        fs::write(dir.join("coins.bin"), []).unwrap();
        let manifest = OverlayManifest {
            snapshot_tip: 10,
            tip: 12,
            len: 4,
            coins_len: 0,
            next_tomb: 3,
            next_coin: 0,
            resident: None,
            heed_off: false,
            seal_complete: false,
            tombs: vec![
                ("tombs-0.bin".into(), 2),
                ("tombs-1.bin".into(), 2),
                ("tombs-2.bin".into(), 1),
            ],
            coins: vec![],
        };
        fs::write(dir.join("MANIFEST"), format_manifest(&manifest)).unwrap();
        let mut loaded = load_overlay(&dir, 10).unwrap().expect("manifest");
        collapse_sealed_runs(&dir, &mut loaded, 10).unwrap();
        assert_eq!(loaded.runs.tombs.len(), 1);
        assert_eq!(loaded.runs.tombs[0].count(), 4);
        assert!(!dir.join("tombs-0.bin").exists());
        assert!(dir.join("tombs-3.bin").is_file());
        PAGE_PROBES.store(0, Ordering::Relaxed);
        assert!(tomb_page_has(&loaded.runs.tombs[0], &key_n(3)).unwrap());
        assert_eq!(PAGE_PROBES.load(Ordering::Relaxed), 1);
        let again = load_overlay(&dir, 10).unwrap().expect("reloaded");
        assert_eq!(again.runs.tombs.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
