//! Rebuild the hash map to include ALL chunks (0-11)
//! 
//! This fixes the issue where the hash map only has chunk 0,
//! causing blocks from other chunks to be fetched via RPC unnecessarily.

use anyhow::{Context, Result};
use std::path::PathBuf;
use blvm_bench::chunk_index::{build_block_index, save_hash_map, BlockHashMap};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<()> {
    let chunks_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let chunks_dir = PathBuf::from(chunks_dir);
    
    println!("🔨 Rebuilding hash map from ALL chunks...");
    println!("   Chunks directory: {}", chunks_dir.display());
    
    // Check if hash map exists
    let hash_map_file = chunks_dir.join("chunks.hashmap");
    if hash_map_file.exists() {
        println!("   💾 Backing up existing hash map...");
        let backup_path = chunks_dir.join("chunks.hashmap.backup");
        std::fs::copy(&hash_map_file, &backup_path)?;
        println!("   ✅ Backed up to {}", backup_path.display());
        
        // Delete existing hash map to force rebuild
        std::fs::remove_file(&hash_map_file)?;
        println!("   🗑️  Deleted incomplete hash map - will rebuild from all chunks");
    }
    
    println!("\n🚀 Building hash map from all chunks (this may take ~2 hours on HDD)...");
    
    // Build hash map from all chunks
    let (_, hash_map) = build_block_index(&chunks_dir)
        .with_context(|| "Failed to build hash map from chunks")?;
    
    println!("   ✅ Built hash map with {} entries", hash_map.len());
    
    // Count blocks by chunk to verify all chunks are included
    let mut chunk_counts: HashMap<usize, usize> = HashMap::new();
    for (_, (chunk_num, _, _)) in &hash_map {
        *chunk_counts.entry(*chunk_num).or_insert(0) += 1;
    }
    
    println!("   📊 Blocks by chunk:");
    for chunk_num in 0..=11 {
        let count = chunk_counts.get(&chunk_num).copied().unwrap_or(0);
        if count > 0 {
            println!("      Chunk {}: {} blocks", chunk_num, count);
        }
    }
    
    // Convert to BlockHashMap format (block_hash -> (chunk_num, offset))
    let mut blocks_by_hash: BlockHashMap = HashMap::new();
    for (block_hash, (chunk_num, offset, _)) in hash_map {
        blocks_by_hash.insert(block_hash, (chunk_num, offset));
    }
    
    // Save hash map
    save_hash_map(&chunks_dir, &blocks_by_hash)
        .with_context(|| "Failed to save hash map")?;
    
    println!("   💾 Saved hash map ({} entries) - all chunks included!", blocks_by_hash.len());
    println!("\n✅ Hash map rebuilt successfully!");
    println!("   💡 Future runs will use chunks instead of RPC for blocks in chunks");
    
    Ok(())
}

