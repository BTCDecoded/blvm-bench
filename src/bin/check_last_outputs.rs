//! Check the last N outputs in outputs_sorted.bin to see what block heights they're from
//! This helps determine if outputs from late blocks are actually in the file

use anyhow::Result;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::PathBuf;

use blvm_bench::sort_merge::output_refs::OutputRef;

fn main() -> Result<()> {
    let outputs_file = PathBuf::from("/run/media/acolyte/Extra/blockchain/sort_merge_data/outputs_sorted.bin");
    let n = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    
    println!("Reading last {} outputs from: {}", n, outputs_file.display());
    
    let file = File::open(&outputs_file)?;
    let file_size = file.metadata()?.len();
    let mut reader = BufReader::new(file);
    
    // Read last 10MB (should contain many records)
    let chunk_size = (10 * 1024 * 1024).min(file_size);
    let start_offset = file_size - chunk_size;
    
    reader.seek(SeekFrom::Start(start_offset))?;
    let mut buf = vec![0u8; chunk_size as usize];
    reader.read_exact(&mut buf)?;
    
    // Parse all records from this chunk
    let mut records = Vec::new();
    let mut pos = 0;
    
    while pos < buf.len() {
        if let Some((record, consumed)) = OutputRef::from_bytes(&buf[pos..]) {
            records.push(record);
            pos += consumed;
        } else {
            // Can't parse more - might be incomplete record at end
            break;
        }
    }
    
    // Take last N records
    let start_idx = records.len().saturating_sub(n);
    let last_n = &records[start_idx..];
    
    println!("\nLast {} outputs (from {} total in chunk):", last_n.len(), records.len());
    println!("{}", "-".repeat(80));
    for (i, record) in last_n.iter().enumerate() {
        println!("  {}: Block {}, TX {}, Output {} (coinbase: {})", 
            i + 1,
            record.block_height,
            hex::encode(&record.txid[..8]),
            record.output_idx,
            record.is_coinbase
        );
    }
    
    if let Some(last) = last_n.last() {
        println!("\n✅ Last output is from block: {}", last.block_height);
        println!("   Expected: blocks up to 912723");
    }
    
    Ok(())
}

