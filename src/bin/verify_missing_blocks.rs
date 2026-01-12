//! Utility to verify if blocks marked as "missing" are actually in regular chunks
//! 
//! This helps identify if the index is incorrectly marking blocks as missing
//! when they're actually in the chunks.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

fn main() -> Result<()> {
    let chunks_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let chunks_dir = PathBuf::from(chunks_dir);
    
    println!("🔍 Verifying missing blocks in chunks...");
    println!("   Chunks directory: {}", chunks_dir.display());
    
    // Load block index
    let index_path = chunks_dir.join("chunks.index");
    if !index_path.exists() {
        anyhow::bail!("Block index not found at {}", index_path.display());
    }
    
    let index_data = std::fs::read(&index_path)
        .with_context(|| format!("Failed to read index: {}", index_path.display()))?;
    let index: HashMap<u64, blvm_bench::chunk_index::BlockIndexEntry> = bincode::deserialize(&index_data)
        .context("Failed to deserialize block index")?;
    
    println!("   ✅ Loaded block index with {} entries", index.len());
    
    // Find all blocks marked as missing
    let missing_heights: Vec<u64> = index.iter()
        .filter(|(_, entry)| entry.chunk_number == 999)
        .map(|(height, _)| *height)
        .collect();
    
    println!("   📋 Found {} blocks marked as missing (chunk_number=999)", missing_heights.len());
    
    if missing_heights.is_empty() {
        println!("   ✅ No blocks marked as missing");
        return Ok(());
    }
    
    // Build hash map from chunks (same logic as chunk_index_rpc)
    println!("\n🔍 Building hash map from chunks...");
    let (_, blocks_by_block_hash) = blvm_bench::chunk_index::build_block_index(&chunks_dir)?;
    
    // Convert to simple hash map for lookup
    use std::collections::HashMap;
    let mut blocks_by_hash: HashMap<[u8; 32], (usize, u64)> = HashMap::new();
    for (block_hash, (chunk_num, offset, _)) in blocks_by_block_hash {
        blocks_by_hash.insert(block_hash, (chunk_num, offset));
    }
    println!("   ✅ Built hash map with {} blocks from chunks", blocks_by_hash.len());
    
    // Check how many "missing" blocks are actually in chunks
    let mut found_in_chunks = 0;
    let mut not_in_chunks = 0;
    let mut sample_missing: Vec<u64> = Vec::new();
    
    for height in &missing_heights {
        let entry = index.get(height).unwrap();
        let hash = entry.block_hash;
        
        if blocks_by_hash.contains_key(&hash) {
            found_in_chunks += 1;
            if sample_missing.len() < 10 {
                sample_missing.push(*height);
            }
        } else {
            not_in_chunks += 1;
        }
    }
    
    println!("\n📊 Analysis:");
    println!("   Blocks marked as missing: {}", missing_heights.len());
    println!("   Actually found in chunks: {}", found_in_chunks);
    println!("   Not found in chunks: {}", not_in_chunks);
    
    if found_in_chunks > 0 {
        println!("\n⚠️  WARNING: {} blocks are incorrectly marked as missing!", found_in_chunks);
        println!("   Sample heights incorrectly marked: {:?}", &sample_missing[..sample_missing.len().min(10)]);
        println!("\n💡 These blocks should be in regular chunks, not chunk_missing!");
        println!("   The index needs to be fixed to point to the correct chunks.");
    }
    
    if not_in_chunks > 0 {
        println!("\n📋 {} blocks are truly missing and need to be fetched", not_in_chunks);
    }
    
    Ok(())
}

