//! FAST extraction of tx hex for failed transactions
//! Streams ALL blocks sequentially (no seeking), only extracts txs that failed
//! This is the OPTIMAL I/O pattern - same as step 6 but skips verification

use anyhow::{Context, Result};
use blvm_consensus::serialization::block::deserialize_block_with_witnesses;
use blvm_consensus::serialization::transaction::serialize_transaction;
use std::path::PathBuf;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::collections::{HashSet, HashMap};
use std::time::Instant;

use blvm_bench::chunked_cache::ChunkedBlockIterator;

fn main() -> Result<()> {
    let failures_file = PathBuf::from("/run/media/acolyte/Extra/blockchain/sort_merge_data/failures.log");
    let output_file = PathBuf::from("/run/media/acolyte/Extra/blockchain/sort_merge_data/failures_with_hex.log");
    let chunks_dir = PathBuf::from("/run/media/acolyte/Extra/blockchain");
    
    println!("🚀 FAST TX Extraction (sequential streaming, no seeking)");
    println!();
    
    // Step 1: Parse failures.log to get all (block_height, tx_idx) pairs
    println!("Step 1: Parsing failures.log...");
    let parse_start = Instant::now();
    
    let file = File::open(&failures_file)?;
    let reader = BufReader::new(file);
    
    // Map: block_height -> set of (tx_idx, input_idx, original_line)
    let mut failures_by_block: HashMap<u64, Vec<(usize, usize, String)>> = HashMap::new();
    let mut total_failures = 0usize;
    let mut min_height = u64::MAX;
    let mut max_height = 0u64;
    
    for line in reader.lines().filter_map(|l| l.ok()) {
        if !line.contains("Script returned false") { continue; }
        total_failures += 1;
        
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 3 { continue; }
        
        let block_height: u64 = match parts[0].trim().parse() {
            Ok(h) if h > 0 => h,
            _ => continue,
        };
        
        let msg = parts[2].trim();
        let tx_idx: usize = match msg.find("tx ").and_then(|s| {
            msg[s+3..].find(',').and_then(|e| msg[s+3..s+3+e].trim().parse().ok())
        }) {
            Some(t) => t,
            None => continue,
        };
        
        let input_idx: usize = msg.find("input ").and_then(|s| {
            msg[s+6..].trim().parse().ok()
        }).unwrap_or(0);
        
        min_height = min_height.min(block_height);
        max_height = max_height.max(block_height);
        
        failures_by_block.entry(block_height).or_default().push((tx_idx, input_idx, line.clone()));
    }
    
    println!("  Total script failures: {}", total_failures);
    println!("  Unique blocks with failures: {}", failures_by_block.len());
    println!("  Block range: {} to {}", min_height, max_height);
    println!("  Parse time: {:.1}s", parse_start.elapsed().as_secs_f64());
    
    // Step 2: Stream through blocks and extract tx hex
    println!("\nStep 2: Streaming blocks and extracting tx hex...");
    println!("  (This streams ALL blocks from {} to {}, extracting only failed txs)", min_height, max_height);
    
    let stream_start = Instant::now();
    
    // Open output file
    let out_file = File::create(&output_file)?;
    let mut writer = BufWriter::with_capacity(64 * 1024 * 1024, out_file); // 64MB buffer
    writeln!(writer, "# Block Height | Error Type | Details | TX Hex")?;
    
    // Create block iterator starting from min_height
    // Note: max_blocks is a COUNT not an end height, so we compute the count
    let block_count = (max_height - min_height + 1) as usize;
    let mut block_iter = ChunkedBlockIterator::new(&chunks_dir, Some(min_height), Some(block_count))?
        .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator"))?;
    
    let mut blocks_processed = 0u64;
    let mut txs_extracted = 0usize;
    let mut current_height = min_height;
    let last_report = Instant::now();
    
    println!();
    
    loop {
        let block_data = match block_iter.next_block() {
            Ok(Some(data)) => data,
            Ok(None) => break,
            Err(e) => {
                eprintln!("  Error reading block: {}", e);
                continue;
            }
        };
        
        blocks_processed += 1;
        
        // Progress report every 10 seconds or 10000 blocks
        if blocks_processed % 10000 == 0 || last_report.elapsed().as_secs() >= 10 {
            let elapsed = stream_start.elapsed().as_secs_f64();
            let rate = blocks_processed as f64 / elapsed;
            let remaining = (max_height - current_height) as f64 / rate;
            print!("\r  Block {}/{} ({:.1}%) - {} txs extracted - {:.0} blk/s - ETA: {:.0}m   ",
                   current_height, max_height,
                   (current_height - min_height) as f64 / (max_height - min_height) as f64 * 100.0,
                   txs_extracted, rate, remaining / 60.0);
            let _ = std::io::stdout().flush();
        }
        
        // Check if this block has failures
        if let Some(failures) = failures_by_block.get(&current_height) {
            // Deserialize block
            let (block, _) = match deserialize_block_with_witnesses(&block_data) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("\n  Error deserializing block {}: {}", current_height, e);
                    current_height += 1;
                    continue;
                }
            };
            
            // Extract tx hex for each failure
            for (tx_idx, input_idx, _original_line) in failures {
                if *tx_idx >= block.transactions.len() {
                    eprintln!("\n  Invalid tx_idx {} for block {} ({} txs)", 
                             tx_idx, current_height, block.transactions.len());
                    continue;
                }
                
                let tx = &block.transactions[*tx_idx];
                let tx_hex = hex::encode(serialize_transaction(tx));
                
                writeln!(writer, "{} | Script returned false | Script returned false: tx {}, input {} | {}",
                        current_height, tx_idx, input_idx, tx_hex)?;
                
                txs_extracted += 1;
            }
        }
        
        current_height += 1;
        if current_height > max_height { break; }
    }
    
    writer.flush()?;
    
    let stream_time = stream_start.elapsed();
    
    println!("\n");
    println!("================================================================================");
    println!("DONE!");
    println!("  Blocks streamed: {}", blocks_processed);
    println!("  Txs extracted: {}", txs_extracted);
    println!("  Stream time: {:.1}s ({:.1}m)", stream_time.as_secs_f64(), stream_time.as_secs_f64() / 60.0);
    println!("  Rate: {:.0} blocks/sec", blocks_processed as f64 / stream_time.as_secs_f64());
    println!("  Output: {}", output_file.display());
    println!();
    println!("Now run: check_divergences");
    
    Ok(())
}

