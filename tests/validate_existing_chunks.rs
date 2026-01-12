//! Validate existing chunks for corruption

use anyhow::Result;
use blvm_bench::chunked_cache::{load_chunk_metadata, decompress_chunk_streaming};
use sha2::{Sha256, Digest};
use std::io::Read;
use std::path::Path;

#[test]
fn validate_existing_chunks() -> Result<()> {
    let chunks_dir = Path::new("/run/media/acolyte/Extra/blockchain");
    
    let metadata = load_chunk_metadata(chunks_dir)?
        .ok_or_else(|| anyhow::anyhow!("No chunk metadata found"))?;
    
    println!("🔍 Validating {} chunks for corruption...", metadata.num_chunks);
    println!("");
    
    let mut total_blocks = 0;
    let mut corrupted_blocks = 0;
    let mut invalid_prev_hash = 0;
    
    for chunk_num in 0..metadata.num_chunks {
        let chunk_file = chunks_dir.join(format!("chunk_{}.bin.zst", chunk_num));
        if !chunk_file.exists() {
            continue;
        }
        
        println!("📦 Validating chunk {}...", chunk_num);
        
        let mut zstd_proc = decompress_chunk_streaming(&chunk_file)?;
        let stdout = zstd_proc.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get zstd stdout"))?;
        let mut reader = std::io::BufReader::new(stdout);
        
        let mut offset: u64 = 0;
        let mut block_num = 0;
        let mut chunk_corrupted = 0;
        let mut chunk_invalid_prev = 0;
        
        loop {
            let mut len_buf = [0u8; 4];
            match reader.read_exact(&mut len_buf) {
                Ok(_) => {},
                Err(_) => break,
            }
            
            let block_len = u32::from_le_bytes(len_buf) as usize;
            offset += 4;
            
            // Bitcoin blocks can be up to ~4MB (MAX_BLOCK_SIZE = 4,000,000 bytes)
            // But we allow up to 10MB to handle edge cases
            if block_len < 80 || block_len > 10 * 1024 * 1024 {
                // Invalid size - might be end of chunk or corruption
                if block_num < 10 {
                    println!("   ⚠️  Block {} has invalid size: {} bytes - stopping chunk read", block_num, block_len);
                }
                break;
            }
            
            // Read header
            let mut header_buf = vec![0u8; 80];
            reader.read_exact(&mut header_buf)?;
            
            // Calculate block hash
            let first_hash = Sha256::digest(&header_buf);
            let second_hash = Sha256::digest(&first_hash);
            
            // Check if hash is all zeros
            if second_hash.iter().all(|&b| b == 0) {
                corrupted_blocks += 1;
                chunk_corrupted += 1;
                if chunk_corrupted <= 5 {
                    println!("   ❌ Block {}: all-zero hash (corrupted)", block_num);
                }
            } else {
                // Check prev_hash
                let prev_hash = &header_buf[4..36];
                let is_genesis = prev_hash.iter().all(|&b| b == 0);
                
                if is_genesis {
                    // Verify it's actually genesis
                    let mut block_hash = [0u8; 32];
                    block_hash.copy_from_slice(&second_hash);
                    block_hash.reverse();
                    let genesis_prefix = hex::encode(&block_hash[..8]);
                    
                    if genesis_prefix != "000000000019d668" {
                        invalid_prev_hash += 1;
                        chunk_invalid_prev += 1;
                        if chunk_invalid_prev <= 5 {
                            println!("   ❌ Block {}: prev_hash all zeros but not genesis (corrupted)", block_num);
                        }
                    }
                }
            }
            
            // Skip rest
            if block_len > 80 {
                let mut skip_buf = vec![0u8; block_len - 80];
                let _ = reader.read(&mut skip_buf);
            }
            
            offset += block_len as u64;
            block_num += 1;
            total_blocks += 1;
            
            if block_num % 50000 == 0 {
                println!("   Checked {} blocks...", block_num);
            }
        }
        
        if chunk_corrupted > 0 || chunk_invalid_prev > 0 {
            println!("   ⚠️  Chunk {}: {} corrupted blocks, {} invalid prev_hash", 
                     chunk_num, chunk_corrupted, chunk_invalid_prev);
        } else {
            println!("   ✅ Chunk {}: All blocks valid", chunk_num);
        }
    }
    
    println!("");
    println!("📊 Validation Summary:");
    println!("   Total blocks checked: {}", total_blocks);
    println!("   Corrupted blocks (all-zero hash): {}", corrupted_blocks);
    println!("   Invalid prev_hash blocks: {}", invalid_prev_hash);
    
    if corrupted_blocks > 0 || invalid_prev_hash > 0 {
        println!("");
        println!("❌ CHUNKS ARE CORRUPTED - Rerun collection required!");
        return Err(anyhow::anyhow!("Found {} corrupted blocks and {} invalid prev_hash blocks", 
                                   corrupted_blocks, invalid_prev_hash));
    } else {
        println!("");
        println!("✅ All chunks are valid!");
    }
    
    Ok(())
}
