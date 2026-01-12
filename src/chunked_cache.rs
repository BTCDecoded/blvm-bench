//! Chunked and compressed cache support
//!
//! Handles reading from chunked, compressed cache files created by split_and_compress_cache.sh
//! Format: Multiple files like chunk_0.bin.zst, chunk_1.bin.zst, etc.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use crate::chunk_index::{load_block_index, build_block_index, save_block_index, BlockIndex};

/// Chunk metadata
#[derive(Debug, Clone)]
pub struct ChunkMetadata {
    pub total_blocks: u64,
    pub num_chunks: usize,
    pub blocks_per_chunk: u64,
    pub compression: String,
}

/// Load chunk metadata from chunks.meta file
pub fn load_chunk_metadata(chunks_dir: &Path) -> Result<Option<ChunkMetadata>> {
    let meta_file = chunks_dir.join("chunks.meta");
    if !meta_file.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&meta_file)?;
    let mut total_blocks = None;
    let mut num_chunks = None;
    let mut blocks_per_chunk = None;
    let mut compression = None;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "total_blocks" => total_blocks = value.trim().parse().ok(),
                "num_chunks" => num_chunks = value.trim().parse().ok(),
                "blocks_per_chunk" => blocks_per_chunk = value.trim().parse().ok(),
                "compression" => compression = Some(value.trim().to_string()),
                _ => {}
            }
        }
    }

    if let (Some(total), Some(num), Some(per_chunk), Some(comp)) =
        (total_blocks, num_chunks, blocks_per_chunk, compression)
    {
        Ok(Some(ChunkMetadata {
            total_blocks: total,
            num_chunks: num,
            blocks_per_chunk: per_chunk,
            compression: comp,
        }))
    } else {
        Ok(None)
    }
}

/// Decompress a zstd-compressed chunk file
/// 
/// OPTIMIZATION: Returns a streaming reader instead of loading entire chunk into memory
/// This prevents OOM for large chunks (50-60GB compressed = 200GB+ uncompressed)
pub fn decompress_chunk_streaming(chunk_path: &Path) -> Result<std::process::Child> {
    use std::process::{Command, Stdio};

    // OPTIMIZATION: Use streaming decompression instead of loading entire chunk
    // This allows reading blocks one at a time without loading 200GB+ into memory
    let child = Command::new("zstd")
        .arg("-d")
        .arg("--stdout")
        .arg(chunk_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to start zstd decompression: {}", chunk_path.display()))?;

    Ok(child)
}

/// Decompress a zstd-compressed chunk file (legacy - loads entire chunk)
/// 
/// WARNING: This loads the entire chunk into memory. For large chunks (50-60GB compressed),
/// this can require 200GB+ RAM. Use decompress_chunk_streaming() instead.
#[allow(dead_code)]
pub fn decompress_chunk(chunk_path: &Path) -> Result<Vec<u8>> {
    use std::process::Command;

    // Check if zstd is available
    let output = Command::new("zstd")
        .arg("--version")
        .output()
        .context("zstd not found - install with: sudo pacman -S zstd")?;

    if !output.status.success() {
        anyhow::bail!("zstd command failed");
    }

    // Decompress chunk
    let output = Command::new("zstd")
        .arg("-d")
        .arg("--stdout")
        .arg(chunk_path)
        .output()
        .with_context(|| format!("Failed to decompress chunk: {}", chunk_path.display()))?;

    if !output.status.success() {
        anyhow::bail!(
            "zstd decompression failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(output.stdout)
}

/// Load blocks from a single chunk
pub fn load_chunk_blocks(chunk_data: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut blocks = Vec::new();
    let mut offset = 0usize;

    while offset + 4 <= chunk_data.len() {
        // Read block length (u32)
        let block_len = u32::from_le_bytes([
            chunk_data[offset],
            chunk_data[offset + 1],
            chunk_data[offset + 2],
            chunk_data[offset + 3],
        ]) as usize;
        offset += 4;

        if offset + block_len > chunk_data.len() {
            anyhow::bail!("Block extends beyond chunk data");
        }

        blocks.push(chunk_data[offset..offset + block_len].to_vec());
        offset += block_len;
    }

    Ok(blocks)
}

/// Create a streaming iterator over blocks from chunked cache
/// This yields blocks one at a time without loading all into memory
/// Uses block index to ensure correct ordering by height
pub struct ChunkedBlockIterator {
    chunks_dir: PathBuf,
    metadata: ChunkMetadata,
    index: BlockIndex, // Block index for correct ordering
    start_height: u64,
    end_height: u64,
    current_height: u64,
    current_chunk_reader: Option<std::io::BufReader<std::process::ChildStdout>>,
    current_zstd_proc: Option<std::process::Child>,
    current_chunk_number: Option<usize>,
    current_offset: u64,
}

impl ChunkedBlockIterator {
    pub fn new(
        chunks_dir: &Path,
        start_height: Option<u64>,
        max_blocks: Option<usize>,
    ) -> Result<Option<Self>> {
        // Load or build block index for correct ordering
            let index = match load_block_index(chunks_dir)? {
                Some(idx) => {
                    println!("   ✅ Loaded block index ({} entries)", idx.len());
                    idx
                }
                None => {
                    println!("   🔨 Block index not found, building...");
                    println!("   ⚠️  This may take a while (reading all blocks from chunks)...");
                    
                    // Try building index via chaining
                    let idx = match build_block_index(chunks_dir) {
                        Ok((idx, _)) if idx.len() > 1 => {
                            // Chaining succeeded
                            idx
                        }
                        Ok((idx, _)) => {
                            // Chaining returned partial index (likely missing block 1)
                            println!("   ⚠️  Chaining returned partial index ({} entries) - likely missing blocks", idx.len());
                            println!("   💡 Missing blocks will be fetched from RPC during async index build");
                            idx
                        }
                        Err(e) => {
                            // Chaining failed
                            eprintln!("   ⚠️  Chaining failed: {}", e);
                            eprintln!("   ⚠️  Returning empty index - will use RPC-based indexing");
                            BlockIndex::new()
                        }
                    };
                    
                    if idx.len() > 1 {
                        save_block_index(chunks_dir, &idx)?;
                        println!("   ✅ Built and saved block index ({} entries)", idx.len());
                    } else {
                        eprintln!("   ⚠️  Index build incomplete (only {} entries)", idx.len());
                        eprintln!("   💡 This is expected if block 1 is missing from chunks");
                        eprintln!("   💡 Index will be built via RPC in async context");
                        // Don't save incomplete index - will be rebuilt with RPC
                    }
                    idx
                }
            };
        eprintln!("   📍 DEBUG: ChunkedBlockIterator::new called with start_height={:?}, max_blocks={:?}", start_height, max_blocks);
        let metadata = match load_chunk_metadata(chunks_dir)? {
            Some(m) => {
                eprintln!("   📍 DEBUG: Loaded chunk metadata: {} chunks, {} total blocks", m.num_chunks, m.total_blocks);
                m
            },
            None => {
                eprintln!("   📍 DEBUG: No chunk metadata found");
                return Ok(None);
            },
        };

        let start_height_val = start_height.unwrap_or(0);
        let end_height_val = if let Some(max) = max_blocks {
            (start_height_val + max as u64).min(metadata.total_blocks)
        } else {
            metadata.total_blocks
        };

        // Verify index has all required blocks
        for h in start_height_val..end_height_val.min(100) {
            if !index.contains_key(&h) {
                anyhow::bail!("Block index missing entry for height {}", h);
            }
        }

        Ok(Some(Self {
            chunks_dir: chunks_dir.to_path_buf(),
            metadata,
            index,
            start_height: start_height_val,
            end_height: end_height_val,
            current_height: start_height_val,
            current_chunk_reader: None,
            current_zstd_proc: None,
            current_chunk_number: None,
            current_offset: 0,
        }))
    }

    fn load_block_from_index(&mut self, height: u64) -> Result<Option<Vec<u8>>> {
        if height < 100 {
            eprintln!("   🔄 load_block_from_index({}) called", height);
        }
        
        let entry = match self.index.get(&height) {
            Some(e) => {
                if height < 100 {
                    eprintln!("   📍 Found index entry: chunk {}, offset {}", e.chunk_number, e.offset_in_chunk);
                }
                e
            },
            None => {
                if height < 100 {
                    eprintln!("   ⚠️  No index entry for height {}", height);
                }
                return Ok(None); // Block not in index
            }
        };
        let _height = height; // For error context

        // Check if we need to switch chunks
        let need_new_chunk = self.current_chunk_number != Some(entry.chunk_number);
        
        if need_new_chunk {
            eprintln!("   🔄 Need to switch chunks: current={:?}, needed={}", self.current_chunk_number, entry.chunk_number);
            // Clean up previous chunk
            eprintln!("   🔄 Cleaning up previous chunk...");
            if let Some(mut proc) = self.current_zstd_proc.take() {
                eprintln!("   🔄 Killing previous zstd process (switching chunks)...");
                let _ = proc.kill(); // Kill immediately - we're switching chunks anyway
                let _ = proc.wait(); // Wait for it to die (should be instant)
                eprintln!("   ✅ Previous zstd process killed");
            }
            self.current_chunk_reader = None;
            eprintln!("   🔄 Checking if chunk_number is 999 (missing block)...");

            // Check if this is a missing block (chunk_number 999)
            if entry.chunk_number == 999 {
                    eprintln!("   🔄 Loading missing block {} from chunk_missing (chunk_number=999)...", height);
                    // Load from chunk_missing
                    use crate::missing_blocks::get_missing_block;
                    eprintln!("   🔄 About to call get_missing_block({})...", height);
                    let load_start = std::time::Instant::now();
                    let result = get_missing_block(&self.chunks_dir, height);
                    let load_duration = load_start.elapsed();
                    eprintln!("   ✅ get_missing_block({}) completed in {:.2}ms", height, load_duration.as_millis());
                
                match result? {
                    Some(block_data) => {
                        if height < 100 {
                            eprintln!("   ✅ Got missing block {} ({} bytes)", height, block_data.len());
                        }
                        return Ok(Some(block_data));
                    }
                    None => {
                        // Block not found - skip it and continue (don't bail)
                        eprintln!("   ⚠️  Missing block {} not found in chunk_missing - skipping", height);
                        return Ok(None); // Return None to skip this block
                    }
                }
            }
            
            // Start new chunk
            let chunk_file = self.chunks_dir.join(format!("chunk_{}.bin.zst", entry.chunk_number));
            if !chunk_file.exists() {
                anyhow::bail!("Chunk {} not found: {}", entry.chunk_number, chunk_file.display());
            }

            let mut zstd_proc = std::process::Command::new("zstd")
                .arg("-d")
                .arg("--stdout")
                .arg(&chunk_file)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .with_context(|| format!("Failed to start zstd for chunk {}", entry.chunk_number))?;

            let stdout = zstd_proc.stdout.take()
                .ok_or_else(|| anyhow::anyhow!("Failed to get zstd stdout"))?;
            let reader = std::io::BufReader::with_capacity(128 * 1024 * 1024, stdout);

            self.current_chunk_reader = Some(reader);
            self.current_zstd_proc = Some(zstd_proc);
            self.current_chunk_number = Some(entry.chunk_number);
            self.current_offset = 0;
        }

        // Seek to block offset (read and discard bytes until we reach offset)
        let reader = self.current_chunk_reader.as_mut()
            .ok_or_else(|| anyhow::anyhow!("No chunk reader available"))?;

        if self.current_offset < entry.offset_in_chunk {
            let skip_bytes = entry.offset_in_chunk - self.current_offset;
            let mut skip_buf = vec![0u8; (skip_bytes.min(1024 * 1024)) as usize]; // 1MB max skip buffer
            let mut remaining = skip_bytes;
            
            while remaining > 0 {
                let to_read = remaining.min(skip_buf.len() as u64) as usize;
                // CRITICAL FIX: Use read() instead of read_exact() to avoid blocking forever
                // If read() returns fewer bytes than requested, that's OK - we'll continue
                use std::io::Read;
                let bytes_read = reader.read(&mut skip_buf[..to_read])
                    .with_context(|| format!("Failed to read from chunk stream at offset {}", self.current_offset))?;
                if bytes_read == 0 {
                    anyhow::bail!("Unexpected EOF while seeking to block offset (current={}, needed={})", 
                                 self.current_offset, entry.offset_in_chunk);
                }
                remaining -= bytes_read as u64;
            }
            self.current_offset = entry.offset_in_chunk;
        } else if self.current_offset > entry.offset_in_chunk {
            // Can't seek backwards in a stream - need to restart chunk
            // This shouldn't happen if we're reading in order, but handle it
            anyhow::bail!("Cannot seek backwards in chunk stream (current={}, needed={})", 
                         self.current_offset, entry.offset_in_chunk);
        }

        // Read block length (4 bytes)
        if height < 100 {
            eprintln!("   🔄 Reading block length at offset {}...", self.current_offset);
        }
        
        let mut len_buf = [0u8; 4];
        use std::io::Read;
        // CRITICAL FIX: Use read_exact but with better error context
        let read_start = std::time::Instant::now();
        reader.read_exact(&mut len_buf)
            .with_context(|| format!("Failed to read block length at height {} (offset {})", 
                                    height, self.current_offset))?;
        let read_duration = read_start.elapsed();
        if height < 100 {
            eprintln!("   ✅ Read block length in {:.2}ms", read_duration.as_millis());
        } else if read_duration.as_secs() > 1 {
            eprintln!("   ⚠️  Reading block length took {:.2}s for height {} (slow!)", read_duration.as_secs_f64(), height);
        }
        
        self.current_offset += 4;
        
        let block_len = u32::from_le_bytes(len_buf) as usize;
        if block_len > 10 * 1024 * 1024 || block_len < 88 {
            anyhow::bail!("Invalid block size: {} bytes (height {}, offset {})", 
                         block_len, height, self.current_offset);
        }

        if height < 100 {
            eprintln!("   🔄 Reading block data ({} bytes) at offset {}...", block_len, self.current_offset);
        }

        // Read block data
        let mut block_data = vec![0u8; block_len];
        let data_read_start = std::time::Instant::now();
        reader.read_exact(&mut block_data)
            .with_context(|| format!("Failed to read block data at height {} (offset {}, len {})", 
                                    height, self.current_offset, block_len))?;
        let data_read_duration = data_read_start.elapsed();
        if height < 100 {
            eprintln!("   ✅ Read block data in {:.2}ms", data_read_duration.as_millis());
        } else if data_read_duration.as_secs() > 1 {
            eprintln!("   ⚠️  Reading block data took {:.2}s for height {} (slow!)", data_read_duration.as_secs_f64(), height);
        }
        
        self.current_offset += block_len as u64;

        if height < 100 {
            eprintln!("   ✅ load_block_from_index({}) completed successfully", height);
        }
        
        Ok(Some(block_data))
    }

    pub fn next_block(&mut self) -> Result<Option<Vec<u8>>> {
        // CRITICAL FIX: Skip missing blocks instead of stopping
        loop {
            // Check if we've reached the end
            if self.current_height >= self.end_height {
                eprintln!("   📍 ChunkedIterator: Reached end_height {}", self.end_height);
                return Ok(None);
            }

            // CRITICAL: Log every call for first 100 blocks to catch hangs
            if self.current_height < 100 {
                eprintln!("   🔄 ChunkedIterator::next_block() called for height {}", self.current_height);
            }
            
            let load_start = std::time::Instant::now();
            
            // Use index to load block at current height
            let result = match self.load_block_from_index(self.current_height) {
                Ok(Some(block)) => {
                    let load_duration = load_start.elapsed();
                    if self.current_height < 100 {
                        eprintln!("   ✅ ChunkedIterator: load_block_from_index({}) completed in {:.2}ms, got {} bytes", 
                                 self.current_height, load_duration.as_millis(), block.len());
                    } else if load_duration.as_secs() > 1 {
                        eprintln!("   ⚠️  ChunkedIterator: load_block_from_index({}) took {:.2}s (slow!)", 
                                 self.current_height, load_duration.as_secs_f64());
                    }
                    
                    // Verify block hash matches index
                use sha2::{Digest, Sha256};
                if block.len() >= 80 {
                    let header = &block[0..80];
                    let first_hash = Sha256::digest(header);
                    let second_hash = Sha256::digest(&first_hash);
                    let mut block_hash = [0u8; 32];
                    block_hash.copy_from_slice(&second_hash);
                    block_hash.reverse(); // Convert to big-endian
                    
                    if let Some(entry) = self.index.get(&self.current_height) {
                        if block_hash != entry.block_hash {
                            eprintln!("   ⚠️  WARNING: Block hash mismatch at height {}!", self.current_height);
                            eprintln!("      Expected: {}", hex::encode(&entry.block_hash));
                            eprintln!("      Got:      {}", hex::encode(&block_hash));
                        }
                    }
                }
                
                let height = self.current_height;
                self.current_height += 1;
                
                // DEBUG: Log first few blocks
                if height < 5 {
                    use sha2::{Digest, Sha256};
                    if block.len() >= 80 {
                        let header = &block[0..80];
                        let first_hash = Sha256::digest(header);
                        let second_hash = Sha256::digest(&first_hash);
                        let mut hash_bytes = second_hash.as_slice().to_vec();
                        hash_bytes.reverse();
                        eprintln!("   📍 DEBUG ChunkedIterator: Yielding block {} (height {}), block_hash (first 8) = {}", 
                                 height, height, hex::encode(&hash_bytes[..8]));
                    }
                }
                
                // More frequent logging for early blocks to catch issues
                if height < 100 && height % 10 == 0 {
                    println!("     Loaded block {} (height {}) from chunks...", 
                            height, height);
                } else if height < 1000 && height % 100 == 0 {
                    println!("     Loaded block {} (height {}) from chunks...", 
                            height, height);
                } else if height % 25000 == 0 {
                    println!("     Loaded block {} (height {}) from chunks...", 
                            height, height);
                }
                
                    if height < 100 {
                        eprintln!("   ✅ ChunkedIterator: Returning block {} ({} bytes)", height, block.len());
                    }
                    return Ok(Some(block));
                }
                Ok(None) => {
                    // Block not in index - skip it and continue
                    let missing_height = self.current_height;
                    if missing_height < 100 || missing_height % 1000 == 0 {
                        eprintln!("   ⚠️  Block {} missing from index - skipping", missing_height);
                    }
                    self.current_height += 1;
                    // Continue loop to try next block
                    continue;
                }
                Err(e) => {
                    // Error loading block - log and skip
                    let error_height = self.current_height;
                    let load_duration = load_start.elapsed();
                    eprintln!("   ❌ Error loading block {} after {:.2}ms: {} - skipping", 
                            error_height, load_duration.as_millis(), e);
                    self.current_height += 1;
                    // Continue loop to try next block
                    continue;
                }
            };
            
            // If we get here, we've processed the result
            if self.current_height < 100 {
                eprintln!("   📍 ChunkedIterator: Processed result for height {}, continuing loop", self.current_height - 1);
            }
        }
    }

}

/// Load blocks from chunked cache (legacy - loads all into memory)
/// 
/// WARNING: This loads all blocks into memory. For large ranges, use ChunkedBlockIterator instead.
/// This function is kept for backward compatibility but should not be used for >10k blocks.
pub fn load_chunked_cache(
    chunks_dir: &Path,
    start_height: Option<u64>,
    max_blocks: Option<usize>,
) -> Result<Option<Vec<Vec<u8>>>> {
    // Load metadata
    let metadata = match load_chunk_metadata(chunks_dir)? {
        Some(m) => m,
        None => {
            // No chunked cache found
            return Ok(None);
        }
    };

    println!("📂 Loading from chunked cache: {} chunks, {} total blocks", 
             metadata.num_chunks, metadata.total_blocks);

    // Determine which chunks we need
    let start_idx = start_height.unwrap_or(0) as usize;
    let end_idx = if let Some(max) = max_blocks {
        (start_idx + max).min(metadata.total_blocks as usize)
    } else {
        metadata.total_blocks as usize
    };

    let start_chunk = start_idx / metadata.blocks_per_chunk as usize;
    let end_chunk = (end_idx - 1) / metadata.blocks_per_chunk as usize;

    println!("   Loading chunks {}-{} (blocks {}-{})", 
             start_chunk, end_chunk, start_idx, end_idx);

    // CRITICAL FIX: For large ranges, warn and suggest using DirectFile instead
    // Loading 125,000 blocks = ~187GB memory (125k × 1.5MB avg)
    let total_blocks_to_load = end_idx - start_idx;
    if total_blocks_to_load > 10_000 {
        eprintln!("⚠️  WARNING: Attempting to load {} blocks into memory (requires ~{}GB RAM)", 
                 total_blocks_to_load, 
                 (total_blocks_to_load * 1_500_000) / 1_000_000_000);
        eprintln!("   💡 For large ranges, use DirectFile source instead of chunked cache");
        eprintln!("   💡 Chunked cache is optimized for small ranges (<10k blocks)");
        eprintln!("   💡 Consider processing in smaller batches or using DirectFile");
        
        // For very large ranges, return None to force DirectFile usage
        if total_blocks_to_load > 50_000 {
            eprintln!("   ❌ Refusing to load {} blocks - would require ~{}GB RAM", 
                     total_blocks_to_load,
                     (total_blocks_to_load * 1_500_000) / 1_000_000_000);
            return Ok(None); // Force fallback to DirectFile
        }
    }

    // OPTIMIZATION: Stream blocks from chunks instead of loading entire chunks into memory
    // For 50-60GB compressed chunks, this prevents loading 200GB+ into RAM
    let mut all_blocks = Vec::new();
    for chunk_num in start_chunk..=end_chunk.min(metadata.num_chunks - 1) {
        let chunk_file = chunks_dir.join(format!("chunk_{}.bin.zst", chunk_num));
        
        if !chunk_file.exists() {
            eprintln!("   ⚠️  Chunk {} not found: {}", chunk_num, chunk_file.display());
            continue;
        }

        println!("   📦 Streaming blocks from chunk {}...", chunk_num);
        
        // OPTIMIZATION: Stream decompression instead of loading entire chunk
        use std::io::{BufReader, Read};
        use std::process::{Command, Stdio};
        
        let mut zstd_proc = Command::new("zstd")
            .arg("-d")
            .arg("--stdout")
            .arg(&chunk_file)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to start zstd for chunk {}", chunk_num))?;
        
        let mut reader = BufReader::with_capacity(128 * 1024 * 1024, // 128MB buffer
            zstd_proc.stdout.take()
                .ok_or_else(|| anyhow::anyhow!("Failed to get zstd stdout"))?);
        
        // Read blocks one at a time (streaming)
        let mut blocks_in_chunk = 0;
        loop {
            let mut len_buf = [0u8; 4];
            match reader.read_exact(&mut len_buf) {
                Ok(_) => {},
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => {
                    let _ = zstd_proc.wait(); // Clean up
                    return Err(e.into());
                }
            }
            
            let block_len = u32::from_le_bytes(len_buf) as usize;
            
            // Validate block size
            if block_len > 10 * 1024 * 1024 || block_len < 88 {
                let _ = zstd_proc.wait();
                anyhow::bail!("Invalid block size in chunk {}: {} bytes", chunk_num, block_len);
            }
            
            // Read block data
            let mut block_data = vec![0u8; block_len];
            reader.read_exact(&mut block_data)?;
            
            all_blocks.push(block_data);
            blocks_in_chunk += 1;
            
            // OPTIMIZATION: Reduce progress reporting frequency (less I/O overhead)
            if blocks_in_chunk % 25000 == 0 {
                println!("     Loaded {}/{} blocks from chunk {}...", 
                        blocks_in_chunk, metadata.blocks_per_chunk, chunk_num);
            }
        }
        
        // Wait for zstd to finish
        let status = zstd_proc.wait()?;
        if !status.success() {
            anyhow::bail!("zstd decompression failed for chunk {}", chunk_num);
        }
        
        println!("   ✅ Loaded {} blocks from chunk {}", blocks_in_chunk, chunk_num);
    }

    // Filter to requested range
    if start_idx > 0 || end_idx < all_blocks.len() {
        let filtered: Vec<_> = all_blocks.into_iter()
            .skip(start_idx)
            .take(end_idx - start_idx)
            .collect();
        Ok(Some(filtered))
    } else {
        Ok(Some(all_blocks))
    }
}

/// Get chunk directory path
/// 
/// Checks multiple locations in priority order:
/// 1. BLOCK_CACHE_DIR environment variable
/// 2. Secondary drive location (/run/media/acolyte/Extra/blockchain)
/// 3. Default cache directory (~/.cache/blvm-bench/chunks)
pub fn get_chunks_dir() -> Option<PathBuf> {
    // Check environment variable first
    if let Ok(env_dir) = std::env::var("BLOCK_CACHE_DIR") {
        let path = PathBuf::from(env_dir);
        if path.exists() {
            return Some(path);
        }
    }
    
    // Check secondary drive location (where chunks are actually stored)
    let secondary_dir = PathBuf::from("/run/media/acolyte/Extra/blockchain");
    if secondary_dir.exists() && secondary_dir.join("chunk_0.bin.zst").exists() {
        return Some(secondary_dir);
    }
    
    // Fallback to default cache directory
    dirs::cache_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".cache")))
        .map(|cache| cache.join("blvm-bench").join("chunks"))
}

/// Check if chunked cache exists
pub fn chunked_cache_exists() -> bool {
    if let Some(chunks_dir) = get_chunks_dir() {
        chunks_dir.exists() && chunks_dir.join("chunks.meta").exists()
    } else {
        false
    }
}
