//! Step 4: Merge-Join inputs with outputs to get prevout data
//!
//! Both files are sorted by (txid, index):
//! - Inputs sorted by (prevout_txid, prevout_idx)
//! - Outputs sorted by (txid, output_idx)
//!
//! Output: For each input, the prevout data needed for verification:
//! - block_height (spending block)
//! - tx_idx (spending transaction)
//! - input_idx (spending input)
//! - prevout_height (source block, for coinbase maturity)
//! - is_coinbase (source output)
//! - value (for SegWit sighash)
//! - script_pubkey (for verification)

use anyhow::{Context, Result};
use hex;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Instant;

use super::input_refs::InputRef;
use super::output_refs::OutputRef;

/// Joined prevout record (variable size)
/// Fixed header: 4 + 4 + 4 + 4 + 1 + 8 + 2 = 27 bytes
/// Plus variable scriptPubKey
#[derive(Debug, Clone)]
pub struct JoinedPrevout {
    /// Block height where this input is being spent
    pub spending_block: u32,
    /// Transaction index in the spending block
    pub spending_tx_idx: u32,
    /// Input index in the spending transaction
    pub spending_input_idx: u32,
    /// Block height where the prevout was created
    pub prevout_height: u32,
    /// Whether the prevout is from a coinbase transaction
    pub is_coinbase: bool,
    /// Value of the prevout (for SegWit sighash calculation)
    pub value: i64,
    /// The scriptPubKey to verify against
    pub script_pubkey: Vec<u8>,
}

impl JoinedPrevout {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(27 + self.script_pubkey.len());
        buf.extend_from_slice(&self.spending_block.to_le_bytes());
        buf.extend_from_slice(&self.spending_tx_idx.to_le_bytes());
        buf.extend_from_slice(&self.spending_input_idx.to_le_bytes());
        buf.extend_from_slice(&self.prevout_height.to_le_bytes());
        buf.push(if self.is_coinbase { 1 } else { 0 });
        buf.extend_from_slice(&self.value.to_le_bytes());
        buf.extend_from_slice(&(self.script_pubkey.len() as u16).to_le_bytes());
        buf.extend_from_slice(&self.script_pubkey);
        buf
    }
    
    pub fn from_bytes(buf: &[u8]) -> Option<(Self, usize)> {
        if buf.len() < 27 {
            return None;
        }
        
        let spending_block = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let spending_tx_idx = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let spending_input_idx = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let prevout_height = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
        let is_coinbase = buf[16] != 0;
        let value = i64::from_le_bytes([
            buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23], buf[24]
        ]);
        let script_len = u16::from_le_bytes([buf[25], buf[26]]) as usize;
        
        if buf.len() < 27 + script_len {
            return None;
        }
        
        let script_pubkey = buf[27..27 + script_len].to_vec();
        
        Some((Self {
            spending_block,
            spending_tx_idx,
            spending_input_idx,
            prevout_height,
            is_coinbase,
            value,
            script_pubkey,
        }, 27 + script_len))
    }
}

/// Merge-join sorted inputs with sorted outputs
/// 
/// Both files must be sorted by (txid, index).
/// Outputs all inputs that have matching outputs (spent outputs).
pub fn merge_join(
    inputs_file: &Path,
    outputs_file: &Path,
    joined_file: &Path,
) -> Result<(u64, u64)> {
    println!("\n{}", "═".repeat(60));
    println!("STEP 4: Merge-Join Inputs with Outputs");
    println!("{}", "═".repeat(60));
    println!("  Inputs: {}", inputs_file.display());
    println!("  Outputs: {}", outputs_file.display());
    println!("  Joined: {}", joined_file.display());
    
    let start_time = Instant::now();
    
    // Check if we can resume from existing joined file
    // Inputs are sorted by (prevout_txid, prevout_idx), so we need to find
    // the last matched input's prevout key to resume correctly
    let mut resume_from_prevout: Option<([u8; 32], u32)> = None; // (prevout_txid, prevout_idx)
    let mut existing_joined_count = 0u64;
    let mut file_mode = std::fs::OpenOptions::new();
    let file_exists = joined_file.exists();
    
    if file_exists {
        println!("  📍 Joined file exists, checking if we can resume...");
        let joined_meta = std::fs::metadata(joined_file)?;
        let joined_size = joined_meta.len();
        
        if joined_size > 0 {
            // Read last 10MB to find last matched record
            let read_size = std::cmp::min(10 * 1024 * 1024, joined_size);
            let mut joined_reader = BufReader::new(File::open(joined_file)?);
            joined_reader.seek(SeekFrom::End(-(read_size as i64)))?;
            let mut buf = vec![0u8; 256 * 1024];
            let mut leftover = Vec::new();
            
            let mut read_next = |reader: &mut BufReader<File>, leftover: &mut Vec<u8>, buf: &mut [u8]| -> Result<Option<JoinedPrevout>> {
                loop {
                    if leftover.len() >= 27 {
                        if let Some((record, consumed)) = JoinedPrevout::from_bytes(leftover) {
                            leftover.drain(..consumed);
                            return Ok(Some(record));
                        }
                    }
                    let n = reader.read(buf)?;
                    if n == 0 {
                        return Ok(None);
                    }
                    leftover.extend_from_slice(&buf[..n]);
                }
            };
            
            // Find last record
            let mut last_record: Option<JoinedPrevout> = None;
            while let Some(record) = read_next(&mut joined_reader, &mut leftover, &mut buf)? {
                last_record = Some(record);
            }
            
            if let Some(record) = last_record {
                println!("  ✅ Found last matched input: block {}, tx {}, input {}", 
                        record.spending_block, record.spending_tx_idx, record.spending_input_idx);
                
                // Now find this input in the inputs file to get its prevout_txid/prevout_idx
                // (inputs are sorted by prevout, not by spending location, so we need to scan)
                println!("  🔍 Finding input's prevout key in inputs file...");
                let mut inputs_scan = BufReader::new(File::open(inputs_file)?);
                let mut input_scan_buf = [0u8; InputRef::SIZE];
                let mut found_prevout: Option<([u8; 32], u32)> = None;
                let mut scanned = 0u64;
                
                while inputs_scan.read_exact(&mut input_scan_buf).is_ok() {
                    let input = InputRef::from_bytes(&input_scan_buf);
                    scanned += 1;
                    
                    if input.block_height == record.spending_block &&
                       input.tx_idx == record.spending_tx_idx &&
                       input.input_idx == record.spending_input_idx {
                        found_prevout = Some((input.prevout_txid, input.prevout_idx));
                        println!("  ✅ Found prevout key: txid={}, idx={}", 
                                hex::encode(&input.prevout_txid), input.prevout_idx);
                        break;
                    }
                    
                    if scanned % 10_000_000 == 0 {
                        println!("  ⏳ Scanned {}M inputs...", scanned / 1_000_000);
                    }
                }
                
                if let Some(prevout_key) = found_prevout {
                    resume_from_prevout = Some(prevout_key);
                    println!("  📍 Will resume from next input after prevout ({}, {})", 
                            hex::encode(&prevout_key.0), prevout_key.1);
                } else {
                    println!("  ⚠️  Could not find input in inputs file - will re-run from start");
                }
                
                // Count existing records
                let mut count_reader = BufReader::new(File::open(joined_file)?);
                let mut count_buf = vec![0u8; 256 * 1024];
                let mut count_leftover = Vec::new();
                while let Some(_) = read_next(&mut count_reader, &mut count_leftover, &mut count_buf)? {
                    existing_joined_count += 1;
                }
                println!("  📊 Existing joined records: {}", existing_joined_count);
            }
        }
    }
    
    let mut inputs_reader = BufReader::with_capacity(32 * 1024 * 1024, File::open(inputs_file)?);
    let mut outputs_reader = BufReader::with_capacity(32 * 1024 * 1024, File::open(outputs_file)?);
    
    // Open file for append if resuming, create if new, truncate if file exists but we're not resuming
    let mut writer = if resume_from_prevout.is_some() {
        // Resuming - append to existing file
        BufWriter::with_capacity(32 * 1024 * 1024, 
            std::fs::OpenOptions::new().create(false).append(true).write(true).open(joined_file)?)
    } else if file_exists {
        // File exists but we're not resuming - truncate and start fresh
        println!("  ⚠️  File exists but resume not possible - will overwrite");
        BufWriter::with_capacity(32 * 1024 * 1024, 
            std::fs::OpenOptions::new().create(true).truncate(true).write(true).open(joined_file)?)
    } else {
        // New file - create
        BufWriter::with_capacity(32 * 1024 * 1024, 
            std::fs::OpenOptions::new().create(true).write(true).open(joined_file)?)
    };
    
    let mut joined_count = existing_joined_count;
    let mut unmatched_inputs = 0u64;
    let mut last_output_txid: Option<[u8; 32]> = None;
    
    // Skip inputs until we reach the resume point (sorted by prevout_txid, prevout_idx)
    let mut input_buf = [0u8; InputRef::SIZE];
    let mut current_input: Option<InputRef> = None;
    let mut current_output: Option<OutputRef> = None;
    
    // Initialize output reading buffers (needed for resume logic)
    let mut output_buf = vec![0u8; 256 * 1024]; // 256KB read buffer
    let mut output_leftover = Vec::new();
    let mut outputs_exhausted = false;
    
    // Helper to read next output (needed for resume logic)
    let mut read_next_output = |reader: &mut BufReader<File>, leftover: &mut Vec<u8>, buf: &mut [u8]| -> Result<Option<OutputRef>> {
        loop {
            // Try to parse from leftover
            if leftover.len() >= 51 {
                if let Some((output, consumed)) = OutputRef::from_bytes(leftover) {
                    leftover.drain(..consumed);
                    return Ok(Some(output));
                }
            }
            
            // Read more data
            let n = reader.read(buf)?;
            if n == 0 {
                return Ok(None); // EOF
            }
            leftover.extend_from_slice(&buf[..n]);
        }
    };
    
    if let Some((resume_txid, resume_idx)) = resume_from_prevout {
        println!("  ⏩ Skipping inputs until resume point...");
        let mut skipped = 0u64;
        loop {
            if inputs_reader.read_exact(&mut input_buf).is_err() {
                break;
            }
            let input = InputRef::from_bytes(&input_buf);
            
            // Compare: (prevout_txid, prevout_idx) - inputs are sorted by this
            let cmp = input.prevout_txid.cmp(&resume_txid)
                .then_with(|| input.prevout_idx.cmp(&resume_idx));
            
            match cmp {
                std::cmp::Ordering::Less => {
                    skipped += 1;
                    continue; // Skip this input
                }
                std::cmp::Ordering::Equal => {
                    // Found the resume point, skip this one and start from next
                    skipped += 1;
                    if inputs_reader.read_exact(&mut input_buf).is_ok() {
                        current_input = Some(InputRef::from_bytes(&input_buf));
                    }
                    println!("  ✅ Resumed from input after {} skipped inputs", skipped);
                    break;
                }
                std::cmp::Ordering::Greater => {
                    // We've passed the resume point, use this input
                    current_input = Some(input);
                    println!("  ✅ Resumed from input ({} skipped)", skipped);
                    break;
                }
            }
        }
        
        // Also need to position outputs reader at the matching output
        // Outputs are sorted by (txid, output_idx), so find the output matching resume_txid/resume_idx
        println!("  ⏩ Positioning outputs reader at resume point...");
        let mut output_pos_found = false;
        
        while let Some(output) = read_next_output(&mut outputs_reader, &mut output_leftover, &mut output_buf)? {
            let cmp = output.txid.cmp(&resume_txid)
                .then_with(|| output.output_idx.cmp(&resume_idx));
            
            match cmp {
                std::cmp::Ordering::Less => continue, // Keep reading
                std::cmp::Ordering::Equal | std::cmp::Ordering::Greater => {
                    // Found it or passed it - use this output
                    let txid_str = hex::encode(&output.txid);
                    let idx = output.output_idx;
                    current_output = Some(output);
                    output_pos_found = true;
                    println!("  ✅ Positioned outputs reader at txid={}, idx={}", txid_str, idx);
                    break;
                }
            }
        }
        
        if !output_pos_found {
            println!("  ⚠️  Could not find matching output - starting from beginning of outputs");
            outputs_reader = BufReader::with_capacity(32 * 1024 * 1024, File::open(outputs_file)?);
            output_leftover.clear(); // Clear leftover from positioning attempt
            // CRITICAL: Initialize current_output from the beginning
            if let Some(output) = read_next_output(&mut outputs_reader, &mut output_leftover, &mut output_buf)? {
                current_output = Some(output);
                println!("  ✅ Initialized outputs reader from beginning");
            } else {
                outputs_exhausted = true;
                println!("  ⚠️  No outputs available - outputs file may be empty");
            }
        }
    } else {
        // No resume - start from beginning
        if inputs_reader.read_exact(&mut input_buf).is_ok() {
            current_input = Some(InputRef::from_bytes(&input_buf));
        }
        // Initialize current_output from beginning
        if let Some(output) = read_next_output(&mut outputs_reader, &mut output_leftover, &mut output_buf)? {
            current_output = Some(output);
        } else {
            outputs_exhausted = true;
        }
    }
    
    let mut last_report = Instant::now();
    
    // Merge-join loop
    while let Some(ref input) = current_input {
        if outputs_exhausted {
            // No more outputs - remaining inputs are unmatched
            unmatched_inputs += 1;
            
            // Read next input
            if inputs_reader.read_exact(&mut input_buf).is_ok() {
                current_input = Some(InputRef::from_bytes(&input_buf));
            } else {
                break;
            }
            continue;
        }
        
        let output = current_output.as_ref().unwrap();
        
        // Compare keys: (txid, index)
        let cmp = input.prevout_txid.cmp(&output.txid)
            .then_with(|| input.prevout_idx.cmp(&output.output_idx));
        
        match cmp {
            std::cmp::Ordering::Equal => {
                // Match! Write joined record
                let joined = JoinedPrevout {
                    spending_block: input.block_height,
                    spending_tx_idx: input.tx_idx,
                    spending_input_idx: input.input_idx,
                    prevout_height: output.block_height,
                    is_coinbase: output.is_coinbase,
                    value: output.value,
                    script_pubkey: output.script_pubkey.clone(),
                };
                writer.write_all(&joined.to_bytes())?;
                joined_count += 1;
                
                // Advance input (output might be spent multiple times, though rare)
                if inputs_reader.read_exact(&mut input_buf).is_ok() {
                    current_input = Some(InputRef::from_bytes(&input_buf));
                } else {
                    current_input = None;
                }
            }
            std::cmp::Ordering::Less => {
                // Input < Output: input has no matching output (shouldn't happen for valid chain)
                unmatched_inputs += 1;
                
                // Advance input
                if inputs_reader.read_exact(&mut input_buf).is_ok() {
                    current_input = Some(InputRef::from_bytes(&input_buf));
                } else {
                    current_input = None;
                }
            }
            std::cmp::Ordering::Greater => {
                // Input > Output: output is not spent, advance output
                if let Some(output) = read_next_output(&mut outputs_reader, &mut output_leftover, &mut output_buf)? {
                    last_output_txid = Some(output.txid);
                    current_output = Some(output);
                } else {
                    // Outputs exhausted - log diagnostic info
                    if let Some(ref last_txid) = last_output_txid {
                        eprintln!("  ⚠️  Outputs exhausted at input prevout_txid: {}", hex::encode(input.prevout_txid));
                        eprintln!("  Last output txid: {}", hex::encode(*last_txid));
                        eprintln!("  Input prevout_txid > Last output txid: {}", input.prevout_txid > *last_txid);
                    }
                    outputs_exhausted = true;
                }
            }
        }
        
        // Progress report every 10 seconds
        if last_report.elapsed().as_secs() >= 10 {
            println!("  Joined: {}, Unmatched: {}", joined_count, unmatched_inputs);
            last_report = Instant::now();
        }
    }
    
    writer.flush()?;
    
    let elapsed = start_time.elapsed();
    let file_size = std::fs::metadata(joined_file)?.len();
    
    println!("{}", "─".repeat(60));
    println!("  ✅ Step 4 Complete!");
    println!("  Joined records: {}", joined_count);
    println!("  Unmatched inputs: {} (should be 0 for valid chain)", unmatched_inputs);
    println!("  File size: {:.2} GB", file_size as f64 / 1_073_741_824.0);
    println!("  Time: {:.1}m", elapsed.as_secs_f64() / 60.0);
    
    Ok((joined_count, unmatched_inputs))
}

/// Sort joined file by (spending_block, spending_tx_idx, spending_input_idx) using binary merge sort
/// This puts prevouts in the exact order we'll need them during verification.
pub fn sort_joined(input_file: &Path, output_file: &Path) -> Result<()> {
    use std::io::Read;
    use std::collections::BinaryHeap;
    use std::cmp::Reverse;
    
    println!("\n{}", "═".repeat(60));
    println!("STEP 5: Sort Joined Data by Spending Location");
    println!("{}", "═".repeat(60));
    println!("  Input: {}", input_file.display());
    println!("  Output: {}", output_file.display());
    
    let start_time = Instant::now();
    
    let input_size = std::fs::metadata(input_file)?.len();
    println!("  Input size: {:.2} GB", input_size as f64 / 1_073_741_824.0);
    
    // Chunk size: ~2GB of records
    // Average record is ~50 bytes, so ~40M records per chunk
    let chunk_records = 40_000_000usize;
    
    // Create temp directory
    let temp_dir = input_file.parent()
        .unwrap_or(Path::new("."))
        .join("sort_tmp");
    std::fs::create_dir_all(&temp_dir)?;
    
    // Phase 1: Create sorted chunks
    println!("  Phase 1: Creating sorted chunks...");
    let mut reader = BufReader::with_capacity(64 * 1024 * 1024, File::open(input_file)?);
    let mut chunk_files: Vec<std::path::PathBuf> = Vec::new();
    let mut chunk_idx = 0;
    let mut total_records = 0;
    
    let mut buf = vec![0u8; 256 * 1024];
    let mut leftover = Vec::new();
    
    loop {
        // Read chunk into memory (parse variable-length records)
        let mut records: Vec<JoinedPrevout> = Vec::with_capacity(chunk_records);
        
        // Read and parse records until we have enough or EOF
        while records.len() < chunk_records {
            // Get more data if needed
            if leftover.len() < 27 {
                let n = reader.read(&mut buf)?;
                if n == 0 && leftover.is_empty() {
                    break; // EOF
                }
                
                leftover.extend_from_slice(&buf[..n]);
            }
            
            // Try to parse a record
            match JoinedPrevout::from_bytes(&leftover) {
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
            break;
        }
        
        // Sort in memory by (spending_block, spending_tx_idx, spending_input_idx)
        records.sort_unstable_by(|a, b| {
            a.spending_block.cmp(&b.spending_block)
                .then_with(|| a.spending_tx_idx.cmp(&b.spending_tx_idx))
                .then_with(|| a.spending_input_idx.cmp(&b.spending_input_idx))
        });
        
        // Write sorted chunk
        let chunk_path = temp_dir.join(format!("chunk_{}.bin", chunk_idx));
        let mut chunk_writer = BufWriter::with_capacity(64 * 1024 * 1024, File::create(&chunk_path)?);
        for record in &records {
            chunk_writer.write_all(&record.to_bytes())?;
        }
        chunk_writer.flush()?;
        
        println!("    Chunk {}: {} records", chunk_idx, records.len());
        chunk_files.push(chunk_path);
        total_records += records.len();
        chunk_idx += 1;
    }
    
    println!("  Phase 2: Merging {} chunks...", chunk_files.len());
    
    // Phase 2: K-way merge
    struct ChunkReader {
        reader: BufReader<File>,
        current: Option<JoinedPrevout>,
        chunk_idx: usize,
        leftover: Vec<u8>,
    }
    
    impl ChunkReader {
        fn read_next(&mut self) -> Result<()> {
            let mut buf = vec![0u8; 256 * 1024];
            
            // Try to parse from leftover first
            loop {
                if self.leftover.len() >= 27 {
                    match JoinedPrevout::from_bytes(&self.leftover) {
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
        key: (u32, u32, u32),
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
                key: (record.spending_block, record.spending_tx_idx, record.spending_input_idx),
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
                key: (record.spending_block, record.spending_tx_idx, record.spending_input_idx),
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
    println!("  ✅ Step 5 Complete!");
    println!("  Output: {} records ({:.2} GB)", merged, output_size as f64 / 1_073_741_824.0);
    println!("  Time: {:.1}m", elapsed.as_secs_f64() / 60.0);
    
    Ok(())
}

