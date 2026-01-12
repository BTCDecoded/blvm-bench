//! Fix the index by checking if "missing" blocks are actually in chunks
//! 
//! If blocks marked as missing (chunk_number=999) are actually in regular chunks,
//! update the index to point to the correct chunks instead.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use sha2::{Sha256, Digest};

fn main() -> Result<()> {
    let chunks_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let chunks_dir = PathBuf::from(chunks_dir);
    
    println!("🔧 Fixing missing blocks index...");
    println!("   Chunks directory: {}", chunks_dir.display());
    
    // Load block index
    let index_path = chunks_dir.join("chunks.index");
    if !index_path.exists() {
        anyhow::bail!("Block index not found at {}", index_path.display());
    }
    
    let mut index: HashMap<u64, blvm_bench::chunk_index::BlockIndexEntry> = {
        let index_data = std::fs::read(&index_path)
            .with_context(|| format!("Failed to read index: {}", index_path.display()))?;
        bincode::deserialize(&index_data)
            .context("Failed to deserialize block index")?
    };
    
    println!("   ✅ Loaded block index with {} entries", index.len());
    
    // Find all blocks marked as missing
    let missing_heights: Vec<u64> = index.iter()
        .filter(|(_, entry)| entry.chunk_number == 999)
        .map(|(height, _)| *height)
        .collect();
    
    println!("   📋 Found {} blocks marked as missing (chunk_number=999)", missing_heights.len());
    
    if missing_heights.is_empty() {
        println!("   ✅ No blocks marked as missing - index is correct!");
        return Ok(());
    }
    
    // Sample check: verify first 10 "missing" blocks to see if they're actually in chunks
    println!("\n🔍 Sampling first 10 'missing' blocks to check if they're in chunks...");
    let sample: Vec<u64> = missing_heights.iter().take(10).copied().collect();
    
    let mut found_in_chunks = 0;
    for height in &sample {
        let entry = index.get(height).unwrap();
        let hash = entry.block_hash;
        
        // Check each chunk for this hash
        let mut found = false;
        for chunk_num in 0..10 {
            let chunk_file = chunks_dir.join(format!("chunk_{}.bin.zst", chunk_num));
            if !chunk_file.exists() {
                continue;
            }
            
            // Quick check: decompress and search for hash
            if let Ok(Some((chunk_num_found, offset))) = find_block_in_chunk(&chunk_file, &hash) {
                println!("   ✅ Block {} found in chunk {} at offset {}", height, chunk_num_found, offset);
                // Update index entry
                index.insert(*height, blvm_bench::chunk_index::BlockIndexEntry {
                    chunk_number: chunk_num_found,
                    offset_in_chunk: offset,
                    block_hash: hash,
                });
                found = true;
                found_in_chunks += 1;
                break;
            }
        }
        
        if !found {
            println!("   ❌ Block {} not found in chunks (truly missing)", height);
        }
    }
    
    println!("\n📊 Sample Results:");
    println!("   Checked: {} blocks", sample.len());
    println!("   Found in chunks: {}", found_in_chunks);
    println!("   Truly missing: {}", sample.len() - found_in_chunks);
    
    if found_in_chunks > 0 {
        println!("\n⚠️  WARNING: Some blocks are incorrectly marked as missing!");
        println!("   The index needs to be rebuilt properly.");
        println!("\n💡 Recommendation: Rebuild the index using build_block_index_via_rpc");
        println!("   This will correctly identify which blocks are in chunks vs truly missing.");
    } else {
        println!("\n✅ Sample blocks are truly missing - index appears correct");
    }
    
    // Save updated index if we found any fixes
    if found_in_chunks > 0 {
        println!("\n💾 Saving updated index...");
        blvm_bench::chunk_index::save_block_index(&chunks_dir, &index)?;
        println!("   ✅ Saved index with {} fixes", found_in_chunks);
    }
    
    Ok(())
}

fn find_block_in_chunk(chunk_file: &std::path::Path, target_hash: &[u8; 32]) -> Result<Option<(usize, u64)>> {
    use std::process::{Command, Stdio};
    use std::io::Read;
    
    let chunk_num = chunk_file.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("chunk_"))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(999);
    
    // Decompress chunk
    let mut zstd_proc = Command::new("zstd")
        .arg("-d")
        .arg("--stdout")
        .arg(chunk_file)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to start zstd")?;
    
    let mut stdout = zstd_proc.stdout.take().unwrap();
    let mut buffer = Vec::new();
    stdout.read_to_end(&mut buffer)?;
    zstd_proc.wait()?;
    
    // Search for block
    let mut offset = 0u64;
    let mut pos = 0usize;
    
    while pos + 4 < buffer.len() {
        let block_start = pos;
        
        // Read block length
        let len_bytes = &buffer[pos..pos+4];
        let block_len = u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
        
        if block_len < 80 || block_len > 10 * 1024 * 1024 {
            break;
        }
        
        pos += 4;
        if pos + block_len > buffer.len() {
            break;
        }
        
        // Read block data
        let block_data = &buffer[pos..pos+block_len];
        
        // Calculate hash
        if block_data.len() >= 80 {
            let header = &block_data[0..80];
            let first_hash = Sha256::digest(header);
            let second_hash = Sha256::digest(&first_hash);
            let mut block_hash = [0u8; 32];
            block_hash.copy_from_slice(&second_hash);
            block_hash.reverse();
            
            if block_hash == *target_hash {
                return Ok(Some((chunk_num, offset)));
            }
        }
        
        offset += 4 + block_len as u64;
        pos += block_len;
    }
    
    Ok(None)
}




