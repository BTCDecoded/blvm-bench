//! Utility to rebuild missing blocks metadata by scanning the cache file
//! 
//! This fixes the offset mismatch issue by scanning the decompressed cache
//! and building a correct height -> offset mapping.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

fn main() -> Result<()> {
    let chunks_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let chunks_dir = PathBuf::from(chunks_dir);
    
    println!("🔧 Rebuilding missing blocks metadata from cache file...");
    println!("   Chunks directory: {}", chunks_dir.display());
    
    // Load block index to get expected hashes
    let index_path = chunks_dir.join("chunks.index");
    if !index_path.exists() {
        anyhow::bail!("Block index not found at {}", index_path.display());
    }
    
    let index_data = std::fs::read(&index_path)
        .with_context(|| format!("Failed to read index: {}", index_path.display()))?;
    let index: HashMap<u64, blvm_bench::chunk_index::BlockIndexEntry> = bincode::deserialize(&index_data)
        .context("Failed to deserialize block index")?;
    
    println!("   ✅ Loaded block index with {} entries", index.len());
    
    // Find all blocks that should be in missing blocks (chunk_number = 999)
    let mut missing_heights: Vec<u64> = index.iter()
        .filter(|(_, entry)| entry.chunk_number == 999)
        .map(|(height, _)| *height)
        .collect();
    missing_heights.sort();
    
    println!("   📋 Found {} blocks marked as missing", missing_heights.len());
    
    if missing_heights.is_empty() {
        println!("   ✅ No missing blocks to rebuild metadata for");
        return Ok(());
    }
    
    // Build hash -> height mapping for quick lookup
    let mut hash_to_height: HashMap<[u8; 32], u64> = HashMap::new();
    for height in &missing_heights {
        if let Some(entry) = index.get(height) {
            hash_to_height.insert(entry.block_hash, *height);
        }
    }
    
    println!("   🔍 Scanning cache file to find blocks...");
    
    // Scan cache file
    let cache_path = chunks_dir.join("chunk_missing.bin");
    if !cache_path.exists() {
        anyhow::bail!("Cache file not found at {}", cache_path.display());
    }
    
    let mut cache_file = std::fs::File::open(&cache_path)
        .with_context(|| format!("Failed to open cache file: {}", cache_path.display()))?;
    
    let mut new_metadata = HashMap::new();
    let mut current_offset: u64 = 0;
    let mut blocks_scanned = 0;
    let mut blocks_found = 0;
    
    loop {
        // Save current position (this is the offset we want)
        let block_start_offset = current_offset;
        
        // Read block length
        let mut len_buf = [0u8; 4];
        match cache_file.read_exact(&mut len_buf) {
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                println!("   ✅ Reached end of file (incomplete block at end)");
                break;
            },
            Err(e) => return Err(e.into()),
        }
        
        let block_len = u32::from_le_bytes(len_buf) as usize;
        
        // Validate block length
        if block_len < 80 || block_len > 10 * 1024 * 1024 {
            println!("   ⚠️  Invalid block length {} at offset {} - reached end of valid blocks", block_len, current_offset);
            break;
        }
        
        // Check if we have enough data remaining for this block
        let file_size = cache_file.seek(SeekFrom::End(0))?;
        cache_file.seek(SeekFrom::Start(current_offset + 4))?;
        if current_offset + 4 + block_len as u64 > file_size {
            println!("   ⚠️  Block at offset {} would extend past EOF (len={}, file_size={}) - stopping", 
                     current_offset, block_len, file_size);
            break;
        }
        
        // Read block data
        let mut block_data = vec![0u8; block_len];
        match cache_file.read_exact(&mut block_data) {
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                println!("   ⚠️  Incomplete block at offset {} - reached end of file", current_offset);
                break;
            },
            Err(e) => return Err(e.into()),
        }
        
        // Calculate block hash
        if block_data.len() >= 80 {
            let header = &block_data[0..80];
            let first_hash = Sha256::digest(header);
            let second_hash = Sha256::digest(&first_hash);
            let mut block_hash = [0u8; 32];
            block_hash.copy_from_slice(&second_hash);
            block_hash.reverse(); // Convert to big-endian
            
            // Check if this is one of the missing blocks we're looking for
            if let Some(&height) = hash_to_height.get(&block_hash) {
                new_metadata.insert(height, block_start_offset);
                blocks_found += 1;
                
                if blocks_found <= 10 || blocks_found % 100 == 0 {
                    println!("   ✅ Found block {} at offset {} (hash: {})", 
                             height, block_start_offset, hex::encode(&block_hash[..8]));
                }
            }
        }
        
        // Move to next block
        current_offset += 4 + block_len as u64;
        blocks_scanned += 1;
        
        if blocks_scanned % 1000 == 0 {
            println!("   📍 Scanned {} blocks, found {} missing blocks so far...", blocks_scanned, blocks_found);
        }
    }
    
    println!("\n📊 Scan Results:");
    println!("   Blocks scanned: {}", blocks_scanned);
    println!("   Missing blocks found: {}", blocks_found);
    println!("   Missing blocks expected: {}", missing_heights.len());
    
    if blocks_found < missing_heights.len() {
        println!("\n⚠️  Warning: Not all missing blocks were found in cache!");
        let found_heights: std::collections::HashSet<u64> = new_metadata.keys().copied().collect();
        let expected_heights: std::collections::HashSet<u64> = missing_heights.iter().copied().collect();
        let not_found: Vec<u64> = expected_heights.difference(&found_heights).copied().collect();
        println!("   Blocks not found: {:?}", not_found);
    }
    
    // Save new metadata
    let meta = blvm_bench::missing_blocks::MissingBlocksMeta {
        blocks: new_metadata,
        count: blocks_found,
    };
    
    let meta_path = chunks_dir.join("missing_blocks.meta");
    let backup_path = chunks_dir.join("missing_blocks.meta.backup");
    
    // Backup old metadata
    if meta_path.exists() {
        println!("\n💾 Backing up old metadata to {}", backup_path.display());
        std::fs::copy(&meta_path, &backup_path)?;
    }
    
    // Save new metadata
    println!("💾 Saving new metadata to {}", meta_path.display());
    blvm_bench::missing_blocks::save_missing_blocks_meta(&chunks_dir, &meta)?;
    
    println!("\n✅ Metadata rebuild complete!");
    println!("   Total missing blocks: {}", meta.count);
    println!("   Metadata file: {}", meta_path.display());
    
    Ok(())
}

