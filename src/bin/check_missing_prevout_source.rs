//! Check if a missing prevout txid corresponds to a transaction from a processed block
//! This helps determine if missing prevouts are expected (from blocks > 912723) or bugs

use anyhow::{Context, Result};
use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_consensus::block::calculate_tx_id;
use blvm_consensus::serialization::block::deserialize_block_with_witnesses;
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <txid_hex>", args[0]);
        eprintln!("Example: {} 1e8a8280e253fc635aea2731e606af721e548f7d1a7ff3a399512e7bf8bc5bd8", args[0]);
        std::process::exit(1);
    }

    let txid_hex = &args[1];
    let mut txid = [0u8; 32];
    hex::decode_to_slice(txid_hex, &mut txid)
        .context("Invalid txid hex")?;

    let chunks_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let chunks_dir = PathBuf::from(chunks_dir);

    let start_height: u64 = std::env::var("START_HEIGHT")
        .unwrap_or_else(|_| "0".to_string())
        .parse()
        .unwrap_or(0);
    let end_height: u64 = std::env::var("END_HEIGHT")
        .unwrap_or_else(|_| "912723".to_string())
        .parse()
        .unwrap_or(912723);

    println!("🔍 Searching for txid: {}", txid_hex);
    println!("  Block range: {} to {}", start_height, end_height);
    println!("");

    // Search through blocks to find this txid
    let mut block_iter = ChunkedBlockIterator::new(&chunks_dir, Some(start_height), None)?
        .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator"))?;

    let mut height = start_height;
    let mut found = false;
    let mut checked = 0u64;

    while height < end_height {
        let block_data = match block_iter.next_block()? {
            Some(data) => data,
            None => break,
        };

        let (block, _witnesses) = deserialize_block_with_witnesses(&block_data)
            .context("Failed to deserialize block")?;

        // Check each transaction in this block
        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            let calculated_txid = calculate_tx_id(tx);
            if calculated_txid == txid {
                println!("✅ FOUND!");
                println!("  Block height: {}", height);
                println!("  Transaction index: {}", tx_idx);
                println!("  Is coinbase: {}", blvm_consensus::transaction::is_coinbase(tx));
                println!("  Outputs: {}", tx.outputs.len());
                println!("  This transaction SHOULD be in outputs_sorted.bin!");
                found = true;
                break;
            }
        }

        if found {
            break;
        }

        height += 1;
        checked += 1;
        if checked % 10000 == 0 {
            println!("  Checked {} blocks... (current: {})", checked, height);
        }
    }

    if !found {
        println!("❌ NOT FOUND in blocks {} to {}", start_height, end_height);
        println!("  This means the prevout is from a transaction created:");
        println!("    - AFTER block {} (expected to be missing)", end_height);
        println!("    - OR there's a bug in txid calculation/extraction");
    }

    Ok(())
}


















