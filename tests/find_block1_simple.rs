//! Simple direct search for block 1 in first few block files

#[cfg(feature = "differential")]
use anyhow::Result;
#[cfg(feature = "differential")]
use sha2::{Sha256, Digest};
#[cfg(feature = "differential")]
use std::fs::File;
#[cfg(feature = "differential")]
use std::io::{BufReader, Read, Seek, SeekFrom};

#[test]
#[cfg(feature = "differential")]
fn find_block1_simple() -> Result<()> {
    const XOR_KEY1: [u8; 4] = [0x84, 0x22, 0xe9, 0xad];
    const XOR_KEY2: [u8; 4] = [0xb7, 0x8f, 0xff, 0x14];
    const ENCRYPTED_MAGIC: [u8; 4] = [0x7d, 0x9c, 0x5d, 0x74];
    const MAINNET_MAGIC: [u8; 4] = [0xf9, 0xbe, 0xb4, 0xd9];
    
    let genesis_hash_be = hex::decode("000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f")?;
    let block1_hash_be = hex::decode("00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048")?;
    
    println!("🔍 Searching for block 1 in first 5 block files...");
    println!("Genesis hash: {}", hex::encode(&genesis_hash_be));
    println!("Block 1 hash: {}", hex::encode(&block1_hash_be));
    
    let blocks_dir = "/home/acolyte/mnt/bitcoin-start9/blocks";
    
    for file_num in 0..20 {
        let filename = format!("{}/blk{:05}.dat", blocks_dir, file_num);
        let file = match File::open(&filename) {
            Ok(f) => f,
            Err(_) => continue,
        };
        
        println!("\n📂 Checking {}", filename);
        let mut reader = BufReader::new(file);
        let mut offset = 0u64;
        let mut block_count = 0;
        
        loop {
            let pos = reader.stream_position()?;
            let mut magic_buf = [0u8; 4];
            
            match reader.read_exact(&mut magic_buf) {
                Ok(_) => {},
                Err(_) => break, // EOF
            }
            
            let is_encrypted = magic_buf == ENCRYPTED_MAGIC;
            let mut magic = magic_buf;
            
            if is_encrypted {
                // Decrypt magic
                let use_key1 = (pos / 4) % 2 == 0;
                let key1_u32 = u32::from_le_bytes(XOR_KEY1);
                let key2_u32 = u32::from_le_bytes(XOR_KEY2);
                let key_u32 = if use_key1 { key1_u32 } else { key2_u32 };
                let magic_u32 = u32::from_le_bytes(magic);
                let decrypted_magic_u32 = magic_u32 ^ key_u32;
                magic = decrypted_magic_u32.to_le_bytes();
            }
            
            if magic != MAINNET_MAGIC {
                // Not a valid block, try next position
                reader.seek(SeekFrom::Start(pos + 1))?;
                continue;
            }
            
            // Read block size
            let mut size_buf = [0u8; 4];
            reader.read_exact(&mut size_buf)?;
            let block_size = if is_encrypted {
                let size_pos = pos + 4;
                let use_key1 = (size_pos / 4) % 2 == 0;
                let key1_u32 = u32::from_le_bytes(XOR_KEY1);
                let key2_u32 = u32::from_le_bytes(XOR_KEY2);
                let key_u32 = if use_key1 { key1_u32 } else { key2_u32 };
                let size_u32 = u32::from_le_bytes(size_buf) ^ key_u32;
                size_u32 as usize
            } else {
                u32::from_le_bytes(size_buf) as usize
            };
            
            if block_size < 80 || block_size > 32 * 1024 * 1024 {
                reader.seek(SeekFrom::Start(pos + 1))?;
                continue;
            }
            
            // Read block data
            let mut block_data = vec![0u8; block_size];
            reader.read_exact(&mut block_data)?;
            
            // Decrypt block if needed
            if is_encrypted {
                let key1_u32 = u32::from_le_bytes(XOR_KEY1);
                let key2_u32 = u32::from_le_bytes(XOR_KEY2);
                let mut i = 0;
                while i + 4 <= block_data.len() {
                    let byte_offset = pos + 4 + 4 + i as u64;
                    let use_key1 = (byte_offset / 4) % 2 == 0;
                    let key_u32 = if use_key1 { key1_u32 } else { key2_u32 };
                    let chunk = u32::from_le_bytes([
                        block_data[i],
                        block_data[i + 1],
                        block_data[i + 2],
                        block_data[i + 3],
                    ]);
                    let decrypted = chunk ^ key_u32;
                    let bytes = decrypted.to_le_bytes();
                    block_data[i..i + 4].copy_from_slice(&bytes);
                    i += 4;
                }
                while i < block_data.len() {
                    let byte_offset = pos + 4 + 4 + i as u64;
                    let use_key1 = (byte_offset / 4) % 2 == 0;
                    let key = if use_key1 { &XOR_KEY1 } else { &XOR_KEY2 };
                    block_data[i] ^= key[(byte_offset % 4) as usize];
                    i += 1;
                }
            }
            
            // Calculate block hash
            let header = &block_data[0..80];
            let first = Sha256::digest(header);
            let second = Sha256::digest(&first);
            let mut block_hash_be = [0u8; 32];
            block_hash_be.copy_from_slice(&second);
            block_hash_be.reverse();
            
            // Extract prev_hash
            let prev_hash_le = &block_data[4..36];
            let mut prev_hash_be = [0u8; 32];
            prev_hash_be.copy_from_slice(prev_hash_le);
            prev_hash_be.reverse();
            
            // Check if this is genesis
            if block_hash_be == genesis_hash_be.as_slice() {
                println!("   ✅ Found genesis block at offset {}, block #{}", pos, block_count);
            }
            
            // Check if this is block 1
            if block_hash_be == block1_hash_be.as_slice() {
                println!("\n✅✅✅ FOUND BLOCK 1! ✅✅✅");
                println!("   File: {}", filename);
                println!("   Offset: {}", pos);
                println!("   Block number in file: {}", block_count);
                println!("   prev_hash (BE): {}", hex::encode(&prev_hash_be));
                println!("   Genesis (BE): {}", hex::encode(&genesis_hash_be));
                if prev_hash_be == genesis_hash_be.as_slice() {
                    println!("   ✅✅✅ prev_hash MATCHES genesis!");
                } else {
                    println!("   ❌ prev_hash does NOT match genesis");
                    println!("   This is the bug - block 1's prev_hash should match genesis");
                }
                return Ok(());
            }
            
            // Also check by prev_hash
            if prev_hash_be == genesis_hash_be.as_slice() {
                println!("\n✅ Found block with genesis as prev_hash!");
                println!("   File: {}", filename);
                println!("   Offset: {}", pos);
                println!("   block_hash (BE): {}", hex::encode(&block_hash_be));
                println!("   Expected block 1: {}", hex::encode(&block1_hash_be));
                if block_hash_be == block1_hash_be.as_slice() {
                    println!("   ✅✅✅ THIS IS BLOCK 1!");
                    return Ok(());
                }
            }
            
            block_count += 1;
            offset = reader.stream_position()?;
            
            if block_count > 1000 {
                break; // Limit blocks per file
            }
        }
    }
    
    println!("\n❌ Block 1 not found in first 20 files");
    Ok(())
}




