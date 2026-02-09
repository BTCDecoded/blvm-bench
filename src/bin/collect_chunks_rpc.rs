//! Collect blockchain chunks via RPC (correct ordering guaranteed)
//! 
//! This fetches blocks by height directly from the Bitcoin Core node via RPC,
//! guaranteeing correct ordering. Slower than local file reading but always correct.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::io::{BufWriter, Write};
use std::process::{Command, Stdio};
use blvm_bench::start9_rpc_client::Start9RpcClient;
use tokio::time::{timeout, Duration};

const CHUNKS_DIR: &str = "/run/media/acolyte/Extra/blockchain";
const BLOCKS_PER_CHUNK: u64 = 125_000;
const SAVE_INTERVAL: u64 = 1000; // Save progress every 1000 blocks

#[tokio::main]
async fn main() -> Result<()> {
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("   ❌ PANIC: {:?}", panic_info);
    }));

    println!("🔨 Collecting blockchain chunks via RPC...");
    println!("   Target: {}", CHUNKS_DIR);
    println!("   Blocks per chunk: {}", BLOCKS_PER_CHUNK);
    
    let chunks_dir = PathBuf::from(CHUNKS_DIR);
    std::fs::create_dir_all(&chunks_dir)?;
    
    let rpc_client = Start9RpcClient::new();
    
    // Get chain height
    let chain_height = rpc_client.get_block_count().await
        .context("Failed to get chain height from Core")?;
    println!("   Chain height: {}", chain_height);
    
    // Find which chunks we already have
    let mut first_missing_chunk = 0u64;
    loop {
        let chunk_path = chunks_dir.join(format!("chunk_{}.bin.zst", first_missing_chunk));
        if chunk_path.exists() {
            // Verify chunk is complete (not truncated)
            let size = std::fs::metadata(&chunk_path)?.len();
            if size > 1024 * 1024 { // At least 1MB
                first_missing_chunk += 1;
            } else {
                println!("   ⚠️  Chunk {} exists but seems incomplete ({} bytes), will recreate", 
                         first_missing_chunk, size);
                break;
            }
        } else {
            break;
        }
    }
    
    let start_height = first_missing_chunk * BLOCKS_PER_CHUNK;
    println!("   Starting from height: {} (chunk {})", start_height, first_missing_chunk);
    
    // Process chunks
    let num_chunks = (chain_height + 1 + BLOCKS_PER_CHUNK - 1) / BLOCKS_PER_CHUNK;
    
    for chunk_num in first_missing_chunk..num_chunks {
        let chunk_start = chunk_num * BLOCKS_PER_CHUNK;
        let chunk_end = ((chunk_num + 1) * BLOCKS_PER_CHUNK - 1).min(chain_height);
        
        println!("\n📦 Creating chunk {} (blocks {}-{})...", chunk_num, chunk_start, chunk_end);
        
        let temp_path = chunks_dir.join(format!("chunk_{}.bin.tmp", chunk_num));
        let chunk_path = chunks_dir.join(format!("chunk_{}.bin.zst", chunk_num));
        let progress_path = chunks_dir.join(format!("chunk_{}.progress", chunk_num));
        
        // Check for resume from progress file
        let mut current_height = chunk_start;
        if progress_path.exists() {
            if let Ok(progress) = std::fs::read_to_string(&progress_path) {
                if let Ok(h) = progress.trim().parse::<u64>() {
                    if h > chunk_start && h <= chunk_end {
                        current_height = h;
                        println!("   📊 Resuming from height {} (progress file found)", current_height);
                    }
                }
            }
        }
        
        // Open temp file (append mode if resuming)
        let temp_file = std::fs::OpenOptions::new()
            .create(true)
            .append(current_height > chunk_start)
            .write(true)
            .truncate(current_height == chunk_start)
            .open(&temp_path)?;
        let mut writer = BufWriter::with_capacity(16 * 1024 * 1024, temp_file);
        
        let chunk_block_count = chunk_end - current_height + 1;
        let mut blocks_written = 0u64;
        let start_time = std::time::Instant::now();
        
        while current_height <= chunk_end {
            // Fetch block
            let block_result = timeout(
                Duration::from_secs(30),
                fetch_block(&rpc_client, current_height)
            ).await;
            
            let block_data = match block_result {
                Ok(Ok(data)) => data,
                Ok(Err(e)) => {
                    eprintln!("   ⚠️  Failed to fetch block {}: {}, retrying...", current_height, e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                Err(_) => {
                    eprintln!("   ⚠️  Timeout fetching block {}, retrying...", current_height);
                    continue;
                }
            };
            
            // Write to temp file: [size: u32][block_data]
            let size = block_data.len() as u32;
            writer.write_all(&size.to_le_bytes())?;
            writer.write_all(&block_data)?;
            
            blocks_written += 1;
            current_height += 1;
            
            // Progress update
            if blocks_written % 100 == 0 {
                let elapsed = start_time.elapsed().as_secs_f64();
                let blocks_per_sec = blocks_written as f64 / elapsed;
                let remaining = chunk_block_count - blocks_written;
                let eta_secs = remaining as f64 / blocks_per_sec;
                
                print!("\r   Progress: {}/{} blocks ({:.1}%), {:.1} blocks/sec, ETA: {:.0}s    ",
                       blocks_written, chunk_block_count, 
                       blocks_written as f64 / chunk_block_count as f64 * 100.0,
                       blocks_per_sec, eta_secs);
                std::io::stdout().flush()?;
            }
            
            // Save progress
            if blocks_written % SAVE_INTERVAL == 0 {
                writer.flush()?;
                std::fs::write(&progress_path, current_height.to_string())?;
            }
        }
        
        writer.flush()?;
        println!("\n   ✅ Chunk {} data collected ({} blocks)", chunk_num, blocks_written);
        
        // Compress chunk
        println!("   🗜️  Compressing chunk {} (this may take a while)...", chunk_num);
        let status = Command::new("zstd")
            .args(["-T0", "-19", "-f", "-o"])
            .arg(&chunk_path)
            .arg(&temp_path)
            .status()?;
        
        if !status.success() {
            anyhow::bail!("zstd compression failed for chunk {}", chunk_num);
        }
        
        // Cleanup
        std::fs::remove_file(&temp_path)?;
        std::fs::remove_file(&progress_path).ok();
        
        let compressed_size = std::fs::metadata(&chunk_path)?.len();
        println!("   ✅ Chunk {} complete: {} compressed", chunk_num, format_size(compressed_size));
    }
    
    // Update metadata
    let meta_path = chunks_dir.join("chunks.meta");
    let meta_content = format!(
        "# Chunk metadata\n# Collected via RPC\ntotal_blocks={}\nnum_chunks={}\nblocks_per_chunk={}\ncompression=zstd\n",
        chain_height + 1, num_chunks, BLOCKS_PER_CHUNK
    );
    std::fs::write(&meta_path, meta_content)?;
    
    // Delete old hashmap (will need to be rebuilt)
    let hashmap_path = chunks_dir.join("chunks.hashmap");
    if hashmap_path.exists() {
        std::fs::remove_file(&hashmap_path)?;
    }
    
    println!("\n✅ Chunk collection complete!");
    println!("   Next: Run build_hashmap_only to build hash map");
    
    Ok(())
}

async fn fetch_block(client: &Start9RpcClient, height: u64) -> Result<Vec<u8>> {
    // Get block hash
    let hash = client.get_block_hash(height).await
        .context("Failed to get block hash")?;
    
    // Get block hex
    let block_hex = client.get_block_hex(&hash).await
        .context("Failed to get block hex")?;
    
    // Decode hex
    let block_data = hex::decode(&block_hex)
        .context("Failed to decode block hex")?;
    
    Ok(block_data)
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}



















