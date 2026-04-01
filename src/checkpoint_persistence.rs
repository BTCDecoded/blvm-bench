//! Load UTXO checkpoints written under `{BLOCK_CACHE_DIR}/differential_checkpoints/`.
//!
//! On-disk format matches IBD dumps: `bincode` of `HashMap<OutPoint, UTXO>` (values without `Arc`).

use anyhow::{Context, Result};
use blvm_consensus::types::{OutPoint, UTXO, UtxoSet};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Access UTXO checkpoint files next to a chunk cache root.
pub struct CheckpointManager {
    cache_root: PathBuf,
}

impl CheckpointManager {
    pub fn new(cache_root: impl AsRef<Path>) -> Result<Self> {
        let cache_root = cache_root.as_ref().to_path_buf();
        Ok(Self { cache_root })
    }

    fn checkpoint_path(&self, height: u64) -> PathBuf {
        self.cache_root
            .join("differential_checkpoints")
            .join(format!("utxo_{}.bin", height))
    }

    /// Load `utxo_{height}.bin` if present. Format: `HashMap<OutPoint, UTXO>` bincode.
    pub fn load_utxo_checkpoint(&self, height: u64) -> Result<Option<UtxoSet>> {
        let path = self.checkpoint_path(height);
        if !path.is_file() {
            return Ok(None);
        }
        let file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
        let raw: HashMap<OutPoint, UTXO> = bincode::deserialize_from(BufReader::new(file))
            .with_context(|| format!("deserialize UTXO checkpoint {}", path.display()))?;
        let set: UtxoSet = raw.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
        Ok(Some(set))
    }
}
