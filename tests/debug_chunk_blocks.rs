//! Debug: Check what blocks are actually in chunks and their prev_hashes

use anyhow::Result;
use blvm_bench::chunked_cache::{load_chunk_metadata, decompress_chunk_streaming};
use sha2::{Sha256, Digest};
use std::io::Read;
use std::path::Path;

#[test]
fn debug_chunk_blocks() -> Result<()> {
    let chunks_dir = Path::new("/run/media/acolyte/Extra/blockchain");
    
    let metadata = load_chunk_metadata(chunks_dir)?
        .ok_or_else(|| anyhow::anyhow!("No chunk metadata found"))?;
    
    println!("🔍 Checking first 10 blocks in each chunk for prev_hash values...");
    
    // Genesis hash (BE)
    let genesis_hash_be_hex = "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f";
    let genesis_hash_be = hex::decode(genesis_hash_be_hex)?;
    let mut genesis_hash_be_array = [0u8; 32];
    genesis_hash_be_array.copy_from_slice(&genesis_hash_be);
    
    for chunk_num in 0..metadata.num_chunks.min(3) {
        let chunk_file = chunks_dir.join(format!("chunk_{}.bin.zst", chunk_num));
        if !chunk_file.exists() {
            continue;
        }
        
        println!("\n📦 Chunk {}:", chunk_num);
        
        let mut zstd_proc = decompress_chunk_streaming(&chunk_file)?;
        let stdout = zstd_proc.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get zstd stdout"))?;
        let mut reader = std::io::BufReader::new(stdout);
        
        let mut offset: u64 = 0;
        let mut block_num = 0;
        
        // Check first 10 blocks
        for _ in 0..10 {
            let mut len_buf = [0u8; 4];
            match reader.read_exact(&mut len_buf) {
                Ok(_) => {},
                Err(_) => break,
            }
            
            let block_len = u32::from_le_bytes(len_buf) as usize;
            offset += 4;
            
            if block_len < 80 {
                break;
            }
            
            // Read header
            let mut header_buf = vec![0u8; 80];
            reader.read_exact(&mut header_buf)?;
            
            // Extract prev_hash (LE from header)
            let mut prev_hash_le = [0u8; 32];
            prev_hash_le.copy_from_slice(&header_buf[4..36]);
            
            // Convert to BE
            let mut prev_hash_be = prev_hash_le;
            prev_hash_be.reverse();
            
            // Calculate block hash
            let first_hash = Sha256::digest(&header_buf);
            let second_hash = Sha256::digest(&first_hash);
            let mut block_hash = [0u8; 32];
            block_hash.copy_from_slice(&second_hash);
            block_hash.reverse(); // BE
            
            println!("  Block {}: prev_hash_be={}, block_hash_be={}", 
                     block_num,
                     hex::encode(&prev_hash_be[..8]),
                     hex::encode(&block_hash[..8]));
            
            // Check if prev_hash matches genesis
            if prev_hash_be == genesis_hash_be_array {
                println!("    ✅ THIS BLOCK HAS GENESIS AS PREV_HASH! (Should be block 1)");
                println!("    Block hash: {}", hex::encode(&block_hash));
            }
            
            // Skip rest
            if block_len > 80 {
                let mut skip_buf = vec![0u8; block_len - 80];
                let _ = reader.read(&mut skip_buf);
            }
            
            offset += block_len as u64;
            block_num += 1;
        }
    }
    
    Ok(())
}
