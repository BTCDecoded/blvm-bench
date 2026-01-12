//! Test to find block 1 in chunks by searching for its hash

use anyhow::Result;
use blvm_bench::chunk_index::build_block_index;
use std::collections::HashMap;
use std::path::Path;
use sha2::{Sha256, Digest};

#[test]
fn test_find_block1_in_chunks() -> Result<()> {
    let chunks_dir = Path::new("/run/media/acolyte/Extra/blockchain");
    
    // Block 1 hash from Core (big-endian)
    let block1_hash_be = hex::decode("00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048")?;
    let mut block1_hash_be_array = [0u8; 32];
    block1_hash_be_array.copy_from_slice(&block1_hash_be);
    
    println!("🔍 Searching for block 1 in chunks...");
    println!("   Block 1 hash (BE): {}", hex::encode(&block1_hash_be_array[..8]));
    
    // Search through all chunks manually
    use blvm_bench::chunked_cache::{load_chunk_metadata, decompress_chunk_streaming};
    use std::io::Read;
    
    let metadata = load_chunk_metadata(chunks_dir)?
        .ok_or_else(|| anyhow::anyhow!("No chunk metadata found"))?;
    
    println!("   Processing {} chunks...", metadata.num_chunks);
    
    for chunk_num in 0..metadata.num_chunks {
        let chunk_file = chunks_dir.join(format!("chunk_{}.bin.zst", chunk_num));
        if !chunk_file.exists() {
            continue;
        }
        
        println!("   📦 Checking chunk {}...", chunk_num);
        
        let mut zstd_proc = decompress_chunk_streaming(&chunk_file)?;
        let stdout = zstd_proc.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get zstd stdout"))?;
        let mut reader = std::io::BufReader::new(stdout);
        
        let mut offset: u64 = 0;
        let mut block_num = 0;
        let mut found_count = 0;
        
        loop {
            let mut len_buf = [0u8; 4];
            match reader.read_exact(&mut len_buf) {
                Ok(_) => {},
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
            
            let block_len = u32::from_le_bytes(len_buf) as usize;
            offset += 4;
            
            if block_len > 10 * 1024 * 1024 || block_len < 88 {
                break; // Invalid, probably end of chunk
            }
            
            // Read header
            let mut header_buf = vec![0u8; 80.min(block_len)];
            reader.read_exact(&mut header_buf)?;
            
            // Calculate block hash
            let first_hash = Sha256::digest(&header_buf);
            let second_hash = Sha256::digest(&first_hash);
            let mut block_hash = [0u8; 32];
            block_hash.copy_from_slice(&second_hash);
            block_hash.reverse(); // Big-endian
            
            // Check if this is block 1
            if block_hash == block1_hash_be_array {
                println!("   ✅ FOUND BLOCK 1!");
                println!("      Chunk: {}", chunk_num);
                println!("      Offset: {}", offset - 4);
                println!("      Block number in chunk: {}", block_num);
                println!("      Hash: {}", hex::encode(&block_hash[..8]));
                
                // Get prev_hash
                let mut prev_hash_le = [0u8; 32];
                prev_hash_le.copy_from_slice(&header_buf[4..36]);
                let mut prev_hash_be = prev_hash_le;
                prev_hash_be.reverse();
                println!("      Prev hash (BE): {}", hex::encode(&prev_hash_be[..8]));
                
                found_count += 1;
            }
            
            // Skip rest of block
            if block_len > 80 {
                let mut skip_buf = vec![0u8; block_len - 80];
                reader.read_exact(&mut skip_buf)?;
            }
            
            offset += block_len as u64;
            block_num += 1;
            
            if block_num % 50000 == 0 {
                println!("      Checked {} blocks in chunk {}...", block_num, chunk_num);
            }
        }
        
        if found_count > 0 {
            println!("   ✅ Found {} instance(s) of block 1 in chunk {}", found_count, chunk_num);
        }
    }
    
    Ok(())
}
