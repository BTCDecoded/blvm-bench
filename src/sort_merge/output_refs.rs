//! Step 3: Extract outputs from all blocks
//!
//! For each transaction output, record:
//! - txid (32 bytes): Transaction ID
//! - output_idx (4 bytes): Output index within transaction
//! - block_height (4 bytes): Block height (needed for coinbase maturity check)
//! - is_coinbase (1 byte): Whether this is a coinbase output
//! - value (8 bytes): Output value in satoshis
//! - script_len (2 bytes): Length of scriptPubKey
//! - script_pubkey (variable): The scriptPubKey
//!
//! Variable size due to scriptPubKey, average ~50 bytes per record.
//! ~2.5B outputs, but we can filter to only spent ones during merge.

use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufWriter, Write, BufReader, Read};
use std::path::Path;
use std::time::Instant;
use rayon::prelude::*;

use blvm_consensus::serialization::block::deserialize_block_with_witnesses;
use blvm_consensus::serialization::encode_varint;
use blvm_consensus::transaction::is_coinbase;
use blvm_consensus::types::Hash;

use crate::chunked_cache::ChunkedBlockIterator;

/// Output record (variable size)
/// Header: 32 + 4 + 4 + 1 + 8 + 2 = 51 bytes fixed
/// Plus variable scriptPubKey
#[derive(Debug, Clone)]
pub struct OutputRef {
    pub txid: Hash,
    pub output_idx: u32,
    pub block_height: u32,
    pub is_coinbase: bool,
    pub value: i64,
    pub script_pubkey: Vec<u8>,
}

impl OutputRef {
    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(51 + self.script_pubkey.len());
        buf.extend_from_slice(&self.txid);
        buf.extend_from_slice(&self.output_idx.to_le_bytes());
        buf.extend_from_slice(&self.block_height.to_le_bytes());
        buf.push(if self.is_coinbase { 1 } else { 0 });
        buf.extend_from_slice(&self.value.to_le_bytes());
        buf.extend_from_slice(&(self.script_pubkey.len() as u16).to_le_bytes());
        buf.extend_from_slice(&self.script_pubkey);
        buf
    }
    
    /// Read from bytes, returns (record, bytes_consumed)
    pub fn from_bytes(buf: &[u8]) -> Option<(Self, usize)> {
        if buf.len() < 51 {
            return None;
        }
        
        let mut txid = [0u8; 32];
        txid.copy_from_slice(&buf[0..32]);
        let output_idx = u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]);
        let block_height = u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]);
        let is_coinbase = buf[40] != 0;
        let value = i64::from_le_bytes([
            buf[41], buf[42], buf[43], buf[44], buf[45], buf[46], buf[47], buf[48]
        ]);
        let script_len = u16::from_le_bytes([buf[49], buf[50]]) as usize;
        
        if buf.len() < 51 + script_len {
            return None;
        }
        
        let script_pubkey = buf[51..51 + script_len].to_vec();
        
        Some((Self {
            txid,
            output_idx,
            block_height,
            is_coinbase,
            value,
            script_pubkey,
        }, 51 + script_len))
    }
}

/// Calculate txid from transaction
/// CRITICAL: Must use the SAME txid calculation as blvm-consensus to ensure merge-join works
/// Step 1 reads prevout.hash (which is the txid), and step 3 calculates txid - they MUST match
fn calculate_txid(tx: &blvm_consensus::types::Transaction) -> Hash {
    // Use the same calculate_tx_id function from blvm-consensus to ensure consistency
    use blvm_consensus::block::calculate_tx_id;
    calculate_tx_id(tx)
}

/// Extract all outputs from blocks and write to file
pub fn extract_outputs(
    chunks_dir: &Path,
    output_file: &Path,
    start_height: u64,
    end_height: u64,
    progress_interval: u64,
) -> Result<u64> {
    println!("\n{}", "═".repeat(60));
    println!("STEP 3: Extract Transaction Outputs");
    println!("{}", "═".repeat(60));
    println!("  Chunks dir: {}", chunks_dir.display());
    println!("  Blocks: {} to {}", start_height, end_height);
    println!("  Output: {}", output_file.display());
    
    let start_time = Instant::now();
    
    // Check if output file exists and find last processed block height
    let mut actual_start_height = start_height;
    let mut file_mode = std::fs::OpenOptions::new();
    let file_exists = output_file.exists();
    
    if file_exists {
        println!("  📍 Output file exists, checking last processed block...");
        // OPTIMIZATION: Use the same approach as check_last_outputs - read last chunk and parse
        use std::io::Seek;
        let existing_file = File::open(output_file)
            .with_context(|| format!("Failed to open existing output file: {}", output_file.display()))?;
        let file_size = existing_file.metadata()?.len();
        
        // Read last 100MB (should contain many records with highest block heights)
        // Larger size ensures we get valid records even if we start mid-record
        let chunk_size = (100 * 1024 * 1024).min(file_size);
        let start_offset = file_size - chunk_size;
        
        let mut reader = BufReader::new(existing_file);
        reader.seek(std::io::SeekFrom::Start(start_offset))?;
        let mut buf = vec![0u8; chunk_size as usize];
        reader.read_exact(&mut buf)?;
        
        // Parse all records from this chunk (same logic as check_last_outputs)
        let mut records: Vec<OutputRef> = Vec::new();
        let mut pos = 0;
        
        while pos < buf.len() {
            if let Some((record, consumed)) = OutputRef::from_bytes(&buf[pos..]) {
                // Validate: reasonable block height (< 1M blocks) and reasonable value
                if record.block_height < 1_000_000 && record.value >= 0 && record.value < 21_000_000_000_000_000 {
                    records.push(record);
                }
                pos += consumed;
            } else {
                // Can't parse more - might be incomplete record at end
                break;
            }
        }
        
        // Find max block_height from parsed records
        let mut max_block_height = records.iter()
            .map(|r| r.block_height as u64)
            .max()
            .unwrap_or(0);
        
        // If parsing failed or gave garbage, use known value from check_last_outputs tool
        // The tool confirmed last output is from block 762154
        if max_block_height == 0 || max_block_height >= 1_000_000 {
            println!("  ⚠️  Parsing gave invalid result ({}), using known fallback: 762154", max_block_height);
            max_block_height = 762154; // Known from check_last_outputs tool
        }
        
        println!("  ✅ Scanned last {}MB, found {} records, max block height: {}", 
                 chunk_size / (1024 * 1024), records.len(), max_block_height);
        
        if max_block_height > 0 && max_block_height < 1_000_000 {
            actual_start_height = max_block_height + 1;
            println!("  ✅ Found {} records in last {}MB, last processed block: {}", 
                     records.len(), chunk_size / (1024 * 1024), max_block_height);
            println!("  📍 Resuming from block {} (will append to existing file)", actual_start_height);
            
            if actual_start_height >= end_height {
                println!("  ✅ All blocks already processed (up to {})", max_block_height);
                return Ok(records.len() as u64);
            }
        } else {
            println!("  ⚠️  Couldn't determine last block, using fallback: 762154");
            actual_start_height = 762155; // Resume from known last block + 1
        }
    }
    
    // Create or append to output file
    let file = file_mode
        .create(true)
        .append(file_exists && actual_start_height > start_height)
        .write(true)
        .open(output_file)
        .with_context(|| format!("Failed to open output file: {}", output_file.display()))?;
    let mut writer = BufWriter::with_capacity(64 * 1024 * 1024, file); // 64MB buffer
    
    // Create block iterator starting from actual_start_height
    // CRITICAL FIX: Calculate max_blocks from end_height to ensure we process all requested blocks
    // Note: Iterator will still stop at metadata.total_blocks if chunks don't have all blocks,
    // but this ensures we try to process up to end_height
    let max_blocks = (end_height - actual_start_height) as usize;
    let mut block_iter = ChunkedBlockIterator::new(chunks_dir, Some(actual_start_height), Some(max_blocks))?
        .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator - chunks.meta not found?"))?;
    
    // Log the actual end_height the iterator will use (may be limited by metadata.total_blocks)
    eprintln!("  📍 Block iterator configured: start={}, requested_end={}, max_blocks={}", 
              actual_start_height, end_height, max_blocks);
    
    let mut total_outputs = 0u64;
    let mut height = actual_start_height;
    let mut last_report = Instant::now();
    
    while height < end_height {
        // Get next block
        let block_data = match block_iter.next_block()? {
            Some(data) => data,
            None => {
                if height < end_height {
                    eprintln!("  ⚠️  WARNING: Block iterator ended at height {} but end_height is {}", height, end_height);
                    eprintln!("  Missing blocks: {} to {} ({} blocks)", height, end_height - 1, end_height - height);
                    eprintln!("  This will cause missing prevouts for transactions in blocks {} to {}", height, end_height - 1);
                    eprintln!("  Possible causes:");
                    eprintln!("    1. Chunks don't contain all blocks up to {}", end_height);
                    eprintln!("    2. Chunk metadata (chunks.meta) reports fewer blocks than available");
                    eprintln!("    3. Block index is incomplete");
                    eprintln!("  Solution: Ensure chunks contain all blocks up to {} or update chunks.meta", end_height);
                }
                // Stop extraction - we've processed all available blocks
                break;
            }
        };
        
        // Deserialize block
        let (block, _witnesses) = deserialize_block_with_witnesses(&block_data)
            .with_context(|| format!("Failed to deserialize block {}", height))?;
        
        // Process each transaction
        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            let txid = calculate_txid(tx);
            let tx_is_coinbase = is_coinbase(tx);
            
            // Record each output
            for (output_idx, output) in tx.outputs.iter().enumerate() {
                let record = OutputRef {
                    txid,
                    output_idx: output_idx as u32,
                    block_height: height as u32,
                    is_coinbase: tx_is_coinbase,
                    value: output.value,
                    script_pubkey: output.script_pubkey.clone(),
                };
                
                writer.write_all(&record.to_bytes())?;
                total_outputs += 1;
            }
            
            // Silence unused variable warning
            let _ = tx_idx;
        }
        
        // Progress report
        let processed = height - start_height + 1;
        if processed % progress_interval == 0 || last_report.elapsed().as_secs() >= 10 {
            let elapsed = start_time.elapsed().as_secs_f64();
            let rate = processed as f64 / elapsed;
            let remaining = (end_height - height) as f64 / rate;
            println!(
                "  Block {}/{} ({:.1}%) - {} outputs - {:.0} blk/s - ETA: {:.0}m",
                height, end_height,
                (height - start_height) as f64 / (end_height - start_height) as f64 * 100.0,
                total_outputs,
                rate,
                remaining / 60.0
            );
            last_report = Instant::now();
        }
        
        height += 1;
    }
    
    writer.flush()?;
    
    let elapsed = start_time.elapsed();
    let file_size = std::fs::metadata(output_file)?.len();
    
    println!("{}", "─".repeat(60));
    println!("  ✅ Step 3 Complete!");
    println!("  Total outputs: {}", total_outputs);
    println!("  Blocks processed: {}", height - start_height);
    println!("  File size: {:.2} GB", file_size as f64 / 1_073_741_824.0);
    println!("  Time: {:.1}m", elapsed.as_secs_f64() / 60.0);
    println!("  Rate: {:.0} blocks/sec", (height - start_height) as f64 / elapsed.as_secs_f64());
    
    Ok(total_outputs)
}

/// Sort outputs file by (txid, output_idx) using binary external merge sort
/// 
/// Uses binary merge sort (no hex expansion) like Step 2.
/// Variable-length records are handled by parsing them during read.
pub fn sort_outputs(input_file: &Path, output_file: &Path) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    use std::collections::BinaryHeap;
    use std::cmp::Reverse;
    
    println!("\n{}", "═".repeat(60));
    println!("STEP 3b: Sort Outputs by TxID");
    println!("{}", "═".repeat(60));
    println!("  Input: {}", input_file.display());
    println!("  Output: {}", output_file.display());
    
    let start_time = Instant::now();
    
    let input_meta = std::fs::metadata(input_file)?;
    let input_size = input_meta.len();
    let input_mtime = input_meta.modified()?;
    println!("  Input size: {:.2} GB", input_size as f64 / 1_073_741_824.0);
    
    // SAFETY CHECK: If output exists, verify it was created from the same input file
    if output_file.exists() {
        let output_meta = std::fs::metadata(output_file)?;
        let output_mtime = output_meta.modified()?;
        
        // If input file is NEWER than output file, the input has been updated
        if input_mtime > output_mtime {
            eprintln!("\n  ⚠️  WARNING: Output file exists but input file is NEWER!");
            eprintln!("     Input modified:  {:?}", input_mtime);
            eprintln!("     Output modified: {:?}", output_mtime);
            eprintln!("     This means the input file was updated AFTER the output was created.");
            eprintln!("     The output file is likely INCOMPLETE and should be regenerated.");
            eprintln!("\n  Options:");
            eprintln!("     1. Delete {} and re-run step3b", output_file.display());
            eprintln!("     2. Continue anyway (NOT RECOMMENDED - will cause merge-join failures)");
            eprintln!("\n  Aborting to prevent incomplete data. Delete the output file to proceed.");
            anyhow::bail!("Output file is outdated - input file was modified after output was created. Delete {} to regenerate.", output_file.display());
        }
        
        // Also check file sizes are reasonable (output should be similar to input)
        let output_size = output_meta.len();
        let size_diff_pct = ((input_size as f64 - output_size as f64) / input_size as f64 * 100.0).abs();
        if size_diff_pct > 5.0 {
            eprintln!("\n  ⚠️  WARNING: Input and output file sizes differ significantly!");
            eprintln!("     Input size:  {:.2} GB", input_size as f64 / 1_073_741_824.0);
            eprintln!("     Output size: {:.2} GB", output_size as f64 / 1_073_741_824.0);
            eprintln!("     Difference: {:.1}%", size_diff_pct);
            eprintln!("     This may indicate incomplete data.");
        }
    }
    
    // Chunk size: ~2GB of records
    // Average record is ~50 bytes, so ~40M records per chunk
    let chunk_records = 40_000_000usize;
    
    // Create temp directory
    let temp_dir = input_file.parent()
        .unwrap_or(Path::new("."))
        .join("sort_tmp");
    std::fs::create_dir_all(&temp_dir)?;
    
    // Phase 1: Create sorted chunks (PARALLELIZED)
    // Process chunks in batches to avoid excessive memory usage
    println!("  Phase 1: Creating sorted chunks (parallelized)...");
    use std::sync::Mutex;
    
    let mut reader = BufReader::with_capacity(64 * 1024 * 1024, File::open(input_file)?);
    let chunk_files = Mutex::new(Vec::new());
    let total_records = Mutex::new(0u64);
    let chunk_idx = Mutex::new(0usize);
    
    let mut buf = vec![0u8; 64 * 1024]; // Read buffer
    let mut leftover = Vec::new();
    
    // Process chunks in batches (read sequentially, sort/write in parallel)
    // Reduced to 2 to avoid OOM - each chunk is ~2GB in memory
    const PARALLEL_BATCH_SIZE: usize = 2; // Process 2 chunks at a time
    let mut batch = Vec::new();
    
    loop {
        // Read chunk into memory (parse variable-length records)
        let mut records: Vec<OutputRef> = Vec::with_capacity(chunk_records);
        
        // Read and parse records until we have enough or EOF
        while records.len() < chunk_records {
            // Get more data if needed
            if leftover.len() < 51 {
                let n = reader.read(&mut buf)?;
                if n == 0 && leftover.is_empty() {
                    break; // EOF
                }
                
                leftover.extend_from_slice(&buf[..n]);
            }
            
            // Try to parse a record
            match OutputRef::from_bytes(&leftover) {
                Some((record, consumed)) => {
                    records.push(record);
                    leftover.drain(..consumed);
                }
                None => {
                    // Not enough data - read more if we haven't hit EOF
                    if leftover.len() >= 1024 * 1024 {
                        // Too much leftover, probably error
                        anyhow::bail!("Failed to parse record: leftover too large");
                    }
                    
                    let n = reader.read(&mut buf)?;
                    if n == 0 {
                        break; // EOF with incomplete record
                    }
                    leftover.extend_from_slice(&buf[..n]);
                }
            }
        }
        
        if records.is_empty() {
            // Process remaining batch
            if !batch.is_empty() {
                batch.into_par_iter().for_each(|mut records: Vec<OutputRef>| {
                    // Sort in memory by (txid, output_idx)
                    records.sort_unstable_by(|a, b| {
                        a.txid.cmp(&b.txid)
                            .then_with(|| a.output_idx.cmp(&b.output_idx))
                    });
                    
                    // Get chunk index
                    let current_idx = {
                        let mut idx = chunk_idx.lock().unwrap();
                        let idx_val = *idx;
                        *idx += 1;
                        idx_val
                    };
                    
                    // Write sorted chunk
                    let chunk_path = temp_dir.join(format!("chunk_{}.bin", current_idx));
                    let mut chunk_writer = BufWriter::with_capacity(64 * 1024 * 1024, 
                        File::create(&chunk_path).expect("Failed to create chunk file"));
                    for record in &records {
                        chunk_writer.write_all(&record.to_bytes())
                            .expect("Failed to write chunk");
                    }
                    chunk_writer.flush().expect("Failed to flush chunk");
                    
                    // Update totals
                    {
                        let mut files = chunk_files.lock().unwrap();
                        files.push(chunk_path);
                        let mut total = total_records.lock().unwrap();
                        *total += records.len() as u64;
                    }
                    
                    if current_idx % 10 == 0 {
                        println!("    Chunk {}: {} records", current_idx, records.len());
                    }
                });
            }
            break;
        }
        
        batch.push(records);
        
        // When batch is full, process in parallel
        if batch.len() >= PARALLEL_BATCH_SIZE {
            let batch_to_process = std::mem::take(&mut batch);
            batch_to_process.into_par_iter().for_each(|mut records: Vec<OutputRef>| {
                // Sort in memory by (txid, output_idx)
                records.sort_unstable_by(|a, b| {
                    a.txid.cmp(&b.txid)
                        .then_with(|| a.output_idx.cmp(&b.output_idx))
                });
                
                // Get chunk index
                let current_idx = {
                    let mut idx = chunk_idx.lock().unwrap();
                    let idx_val = *idx;
                    *idx += 1;
                    idx_val
                };
                
                // Write sorted chunk
                let chunk_path = temp_dir.join(format!("chunk_{}.bin", current_idx));
                let mut chunk_writer = BufWriter::with_capacity(64 * 1024 * 1024, 
                    File::create(&chunk_path).expect("Failed to create chunk file"));
                for record in &records {
                    chunk_writer.write_all(&record.to_bytes())
                        .expect("Failed to write chunk");
                }
                chunk_writer.flush().expect("Failed to flush chunk");
                
                // Update totals
                {
                    let mut files = chunk_files.lock().unwrap();
                    files.push(chunk_path);
                    let mut total = total_records.lock().unwrap();
                    *total += records.len() as u64;
                }
                
                if current_idx % 10 == 0 {
                    println!("    Chunk {}: {} records", current_idx, records.len());
                }
            });
        }
    }
    
    let chunk_files = chunk_files.into_inner().unwrap();
    let total_records = total_records.into_inner().unwrap();
    
    println!("  Phase 2: Merging {} chunks...", chunk_files.len());
    
    // Phase 2: K-way merge
    struct ChunkReader {
        reader: BufReader<File>,
        current: Option<OutputRef>,
        chunk_idx: usize,
        leftover: Vec<u8>,
    }
    
    impl ChunkReader {
        fn read_next(&mut self) -> Result<()> {
            let mut buf = vec![0u8; 64 * 1024];
            
            // Try to parse from leftover first
            loop {
                if self.leftover.len() >= 51 {
                    match OutputRef::from_bytes(&self.leftover) {
                        Some((record, consumed)) => {
                            self.current = Some(record);
                            self.leftover.drain(..consumed);
                            return Ok(());
                        }
                        None => {
                            // Need more data
                        }
                    }
                }
                
                // Read more data
                match self.reader.read(&mut buf) {
                    Ok(0) => {
                        // EOF
                        self.current = None;
                        return Ok(());
                    }
                    Ok(n) => {
                        self.leftover.extend_from_slice(&buf[..n]);
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
    }
    
    // Wrapper for heap ordering (min-heap by key)
    #[derive(Eq, PartialEq)]
    struct HeapItem {
        key: ([u8; 32], u32),
        chunk_idx: usize,
    }
    
    impl Ord for HeapItem {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            // Reverse for min-heap
            other.key.cmp(&self.key)
        }
    }
    
    impl PartialOrd for HeapItem {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    
    let mut chunk_readers: Vec<ChunkReader> = Vec::new();
    let mut heap: BinaryHeap<HeapItem> = BinaryHeap::new();
    
    for (idx, chunk_path) in chunk_files.iter().enumerate() {
        let file = File::open(chunk_path)?;
        let mut reader = ChunkReader {
            reader: BufReader::with_capacity(1024 * 1024, file),
            current: None,
            chunk_idx: idx,
            leftover: Vec::new(),
        };
        reader.read_next()?;
        
        if let Some(ref record) = reader.current {
            heap.push(HeapItem {
                key: (record.txid, record.output_idx),
                chunk_idx: idx,
            });
        }
        chunk_readers.push(reader);
    }
    
    // Merge chunks
    let mut output_writer = BufWriter::with_capacity(64 * 1024 * 1024, File::create(output_file)?);
    let mut merged = 0u64;
    let progress_interval = (total_records / 100) as u64;
    
    while let Some(item) = heap.pop() {
        let reader = &mut chunk_readers[item.chunk_idx];
        if let Some(record) = reader.current.take() {
            output_writer.write_all(&record.to_bytes())?;
            merged += 1;
            
            if merged % progress_interval == 0 {
                println!("    Merged: {} / {} ({:.1}%)", merged, total_records, 
                    merged as f64 / total_records as f64 * 100.0);
            }
        }
        
        // Read next from this chunk
        reader.read_next()?;
        if let Some(ref record) = reader.current {
            heap.push(HeapItem {
                key: (record.txid, record.output_idx),
                chunk_idx: item.chunk_idx,
            });
        } else {
            // Chunk exhausted - delete the temp file
            let chunk_path = &chunk_files[item.chunk_idx];
            let _ = std::fs::remove_file(chunk_path);
        }
    }
    
    output_writer.flush()?;
    
    // Cleanup temp directory
    let _ = std::fs::remove_dir_all(&temp_dir);
    
    let elapsed = start_time.elapsed();
    let output_size = std::fs::metadata(output_file)?.len();
    
    println!("{}", "─".repeat(60));
    println!("  ✅ Step 3b Complete!");
    println!("  Output: {} records ({:.2} GB)", merged, output_size as f64 / 1_073_741_824.0);
    println!("  Time: {:.1}m", elapsed.as_secs_f64() / 60.0);
    
    Ok(())
}

