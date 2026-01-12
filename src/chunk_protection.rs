//! Chunk protection module - PREVENTS DELETION OF CHUNKS
//! 
//! CRITICAL: This module exists to prevent accidental deletion of chunks
//! Chunks represent DAYS of work and MUST NEVER be deleted by code

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Chunk directories that MUST NEVER be deleted (final destinations only)
/// Cache chunks are temporary and can be deleted after successful move
const PROTECTED_CHUNK_DIRS: &[&str] = &[
    "/run/media/acolyte/Extra/blockchain",
];

/// Check if a path is a protected chunk directory
pub fn is_protected_chunk_dir(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    PROTECTED_CHUNK_DIRS.iter().any(|&protected| {
        path_str == protected || path_str.starts_with(&format!("{}/", protected))
    })
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
    
    // Also check if path contains chunk files in protected locations
    // Allow deletion from cache (temporary), but protect final destination
    if path.to_string_lossy().contains("chunk_") && 
       path.to_string_lossy().ends_with(".bin.zst") &&
       path.to_string_lossy().contains("/run/media/acolyte/Extra/blockchain") {
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
        
        if path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("chunk_") && n.ends_with(".bin.zst"))
            .unwrap_or(false) {
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
                    .and_then(|e| e.file_name().to_str().map(|s| s.starts_with("chunk_") && s.ends_with(".bin.zst")))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

