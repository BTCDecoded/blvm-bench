//! Chunk protection module - PREVENTS DELETION OF CHUNKS
//!
//! CRITICAL: This module exists to prevent accidental deletion of chunks
//! Chunks represent DAYS of work and MUST NEVER be deleted by code

use crate::block_cache_env::block_cache_dir_from_env;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Roots under which chunk data must never be deleted (from `BLOCK_CACHE_DIR` only).
fn protected_chunk_roots() -> Vec<PathBuf> {
    block_cache_dir_from_env().into_iter().collect()
}

/// Check if a path is a protected chunk directory
pub fn is_protected_chunk_dir(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    for root in protected_chunk_roots() {
        let r = root.to_string_lossy();
        if path_str == r.as_ref() || path_str.starts_with(&format!("{}/", r.as_ref())) {
            return true;
        }
    }
    false
}

/// Validate that we're not trying to delete chunks
///
/// This function should be called before ANY operation that could delete files
pub fn validate_no_chunk_deletion(operation: &str, path: &Path) -> Result<()> {
    if is_protected_chunk_dir(path) {
        anyhow::bail!(
            "🚨🚨🚨 CRITICAL ERROR: Attempted to {} protected chunk directory: {}\n\
             🚨 CHUNKS MUST NEVER BE DELETED - They represent DAYS of work!\n\
             🚨 This operation has been BLOCKED to prevent data loss.\n\
             🚨 If you need to delete chunks, you must do it manually after careful consideration.",
            operation,
            path.display()
        );
    }

    // Also protect chunk files under BLOCK_CACHE_DIR
    if path.to_string_lossy().contains("chunk_")
        && path.to_string_lossy().ends_with(".bin.zst")
        && protected_chunk_roots()
            .iter()
            .any(|root| path.starts_with(root.as_path()))
    {
        anyhow::bail!(
            "🚨🚨🚨 CRITICAL ERROR: Attempted to {} chunk file: {}\n\
                 🚨 CHUNK FILES MUST NEVER BE DELETED!\n\
                 🚨 This operation has been BLOCKED.",
            operation,
            path.display()
        );
    }

    Ok(())
}

/// Make chunks read-only to prevent accidental deletion
pub fn protect_chunks(chunks_dir: &Path) -> Result<()> {
    use std::fs;

    if !chunks_dir.exists() {
        return Ok(());
    }

    // Make all chunk files read-only
    for entry in fs::read_dir(chunks_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("chunk_") && n.ends_with(".bin.zst"))
            .unwrap_or(false)
        {
            let mut perms = fs::metadata(&path)?.permissions();
            perms.set_readonly(true);
            fs::set_permissions(&path, perms)
                .with_context(|| format!("Failed to make chunk read-only: {}", path.display()))?;
        }
    }

    Ok(())
}

/// Check if chunks directory exists and has chunks
pub fn chunks_exist(chunks_dir: &Path) -> bool {
    if !chunks_dir.exists() {
        return false;
    }

    std::fs::read_dir(chunks_dir)
        .map(|entries| {
            entries.into_iter().any(|entry| {
                entry
                    .ok()
                    .and_then(|e| {
                        e.file_name()
                            .to_str()
                            .map(|s| s.starts_with("chunk_") && s.ends_with(".bin.zst"))
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}
