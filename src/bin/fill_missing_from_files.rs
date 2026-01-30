//! Fill missing blocks from local blk*.dat files instead of RPC
//! 
//! This reads blocks directly from the mounted Bitcoin Core data directory,
//! which is MUCH faster than RPC calls.

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::io::{BufReader, Read, Seek, SeekFrom};
use sha2::{Sha256, Digest};

const BITCOIN_DATA_DIR: &str = "/home/acolyte/mnt/bitcoin-start9";
const BLOCK_MAGIC: [u8; 4] = [0xf9, 0xbe, 0xb4, 0xd9]; // Mainnet

#[tokio::main]
async fn main() -> Result<()> {
    println!("🔨 Filling missing blocks from local files...");
    
    let chunks_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let chunks_dir = PathBuf::from(chunks_dir);
    
    let bitcoin_data = PathBuf::from(BITCOIN_DATA_DIR);
    let blocks_dir = bitcoin_data.join("blocks");
    
    // Check if mounted
    if !blocks_dir.exists() {
        anyhow::bail!("Bitcoin data not mounted at {}. Run:\n  sshfs start9@192.168.2.100:/embassy-data/package-data/volumes/bitcoind/data/main {}", 
                      BITCOIN_DATA_DIR, BITCOIN_DATA_DIR);
    }
    
    // Load existing index
    println!("📂 Loading existing index...");
    let index = blvm_bench::chunk_index::load_block_index(&chunks_dir)?
        .ok_or_else(|| anyhow::anyhow!("No existing index found"))?;
    println!("   ✅ Loaded {} entries", index.len());
    
    // Find missing heights (0-249999)
    let max_height = 249999u64;
    let mut missing_heights: Vec<u64> = Vec::new();
    for h in 0..=max_height {
        if !index.contains_key(&h) {
            missing_heights.push(h);
        }
    }
    println!("   📊 {} blocks missing from index", missing_heights.len());
    
    if missing_heights.is_empty() {
        println!("✅ No missing blocks!");
        return Ok(());
    }
    
    // Build hash -> height map from missing_blocks.meta if it exists
    // For now, we'll scan the blk files directly
    
    println!("\n🔍 Scanning blk*.dat files for missing blocks...");
    println!("   This reads directly from SSHFS - much faster than RPC!");
    
    // Find all blk files
    let mut blk_files: Vec<PathBuf> = std::fs::read_dir(&blocks_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("blk") && n.ends_with(".dat"))
                .unwrap_or(false)
        })
        .collect();
    blk_files.sort();
    
    println!("   Found {} blk files", blk_files.len());
    
    // For efficiency, we'll use the LevelDB index to find blocks
    // But first, let's try a simpler approach: scan files and match by block hash
    
    // We need block hashes for the missing heights
    // The index stores (height -> BlockIndexEntry) but we need to find blocks in blk files
    // The blk files don't have height info - they're indexed by hash
    
    // Approach: 
    // 1. Get block hashes for missing heights via RPC (just getblockhash, not getblock)
    // 2. Scan blk files to find those blocks
    // 3. Add to missing_blocks cache
    
    // Actually, let's use the LevelDB block index which maps hash -> (file, offset)
    // This is the most efficient approach
    
    // For now, let's do a hybrid: get hashes via RPC, then read blocks from files
    
    println!("\n🔍 Getting block hashes for missing heights via RPC...");
    let rpc = blvm_bench::start9_rpc_client::Start9RpcClient::new();
    
    // Get hashes in batches
    let mut height_to_hash: HashMap<u64, String> = HashMap::new();
    let batch_size = 1000;
    
    for chunk_start in (0..missing_heights.len()).step_by(batch_size) {
        let chunk_end = (chunk_start + batch_size).min(missing_heights.len());
        let heights_batch: Vec<u64> = missing_heights[chunk_start..chunk_end].to_vec();
        
        println!("   Getting hashes for heights {}-{} ({}/{})", 
                 heights_batch.first().unwrap(), 
                 heights_batch.last().unwrap(),
                 chunk_end, missing_heights.len());
        
        let results = rpc.get_block_hashes_batch(&heights_batch).await?;
        for (height, result) in results {
            if let Ok(hash) = result {
                height_to_hash.insert(height, hash);
            }
        }
    }
    
    println!("   ✅ Got {} block hashes", height_to_hash.len());
    
    // Now scan blk files to find these blocks
    println!("\n📂 Scanning blk files for blocks (reading from SSHFS)...");
    
    // Build reverse lookup: hash -> height
    let mut hash_to_height: HashMap<String, u64> = HashMap::new();
    for (h, hash) in &height_to_hash {
        hash_to_height.insert(hash.clone(), *h);
    }
    
    let mut found_blocks: HashMap<u64, Vec<u8>> = HashMap::new();
    let mut files_scanned = 0;
    
    for blk_file in &blk_files {
        if found_blocks.len() >= height_to_hash.len() {
            break; // Found all blocks
        }
        
        files_scanned += 1;
        if files_scanned % 100 == 0 {
            println!("   Scanned {}/{} files, found {}/{} blocks...", 
                     files_scanned, blk_files.len(), 
                     found_blocks.len(), height_to_hash.len());
        }
        
        // Read and scan the file
        let file = std::fs::File::open(blk_file)?;
        let file_len = file.metadata()?.len();
        let mut reader = BufReader::with_capacity(1024 * 1024, file);
        
        let mut pos: u64 = 0;
        while pos < file_len - 8 {
            // Read magic and size
            let mut header = [0u8; 8];
            if reader.read_exact(&mut header).is_err() {
                break;
            }
            
            if header[0..4] != BLOCK_MAGIC {
                // Seek forward 1 byte and try again
                reader.seek(SeekFrom::Current(-7))?;
                pos += 1;
                continue;
            }
            
            let block_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
            if block_size < 80 || block_size > 4_000_000 {
                pos += 8;
                continue;
            }
            
            // Read block header to get hash
            let mut block_header = [0u8; 80];
            if reader.read_exact(&mut block_header).is_err() {
                break;
            }
            
            // Calculate block hash
            let first_hash = Sha256::digest(&block_header);
            let block_hash = Sha256::digest(&first_hash);
            let hash_hex = hex::encode(block_hash.iter().rev().cloned().collect::<Vec<u8>>());
            
            // Check if this is one of our missing blocks
            if let Some(&height) = hash_to_height.get(&hash_hex) {
                // Read the rest of the block
                let mut block_data = vec![0u8; block_size];
                block_data[0..80].copy_from_slice(&block_header);
                if reader.read_exact(&mut block_data[80..]).is_err() {
                    break;
                }
                
                found_blocks.insert(height, block_data);
                
                if found_blocks.len() % 1000 == 0 {
                    println!("   Found {} blocks...", found_blocks.len());
                }
            } else {
                // Skip the rest of the block
                reader.seek(SeekFrom::Current((block_size - 80) as i64))?;
            }
            
            pos += 8 + block_size as u64;
        }
    }
    
    println!("\n✅ Found {} blocks in blk files", found_blocks.len());
    
    if found_blocks.is_empty() {
        println!("⚠️  No blocks found in files. May need to use RPC instead.");
        return Ok(());
    }
    
    // Save found blocks to missing_blocks cache
    println!("\n💾 Adding blocks to index...");
    let mut new_index = index.clone();
    
    for (height, block_data) in &found_blocks {
        let hash_hex = height_to_hash.get(height).unwrap();
        let offset = blvm_bench::missing_blocks::add_missing_block(&chunks_dir, block_data)?;
        
        // Convert hash hex to bytes
        let hash_bytes: [u8; 32] = hex::decode(hash_hex)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid hash length"))?;
        
        let entry = blvm_bench::chunk_index::BlockIndexEntry {
            chunk_number: 999, // Missing blocks special chunk
            offset_in_chunk: offset,
            block_hash: hash_bytes,
        };
        new_index.insert(*height, entry);
    }
    
    // Save updated index
    blvm_bench::chunk_index::save_block_index(&chunks_dir, &new_index)?;
    println!("✅ Index updated with {} new blocks (total: {})", found_blocks.len(), new_index.len());
    
    Ok(())
}

