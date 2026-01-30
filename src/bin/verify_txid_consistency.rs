//! Verify that input.prevout.hash matches calculated txid for the creating transaction
//! This helps identify if there are txid calculation mismatches

use anyhow::{Context, Result};
use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_consensus::block::calculate_tx_id;
use blvm_consensus::serialization::block::deserialize_block_with_witnesses;
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: {} <block_height> <tx_idx> <input_idx>", args[0]);
        eprintln!("Example: {} 900848 3712 8", args[0]);
        std::process::exit(1);
    }

    let block_height: u64 = args[1].parse()?;
    let tx_idx: usize = args[2].parse()?;
    let input_idx: usize = args[3].parse()?;

    let chunks_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let chunks_dir = PathBuf::from(chunks_dir);

    println!("🔍 Verifying txid consistency:");
    println!("  Block: {}", block_height);
    println!("  Transaction index: {}", tx_idx);
    println!("  Input index: {}", input_idx);
    println!("");

    // Load the spending block
    let mut block_iter = ChunkedBlockIterator::new(&chunks_dir, Some(block_height), Some(1))?
        .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator"))?;

    let block_data = block_iter.next_block()?
        .ok_or_else(|| anyhow::anyhow!("Block {} not found", block_height))?;

    let (block, _witnesses) = deserialize_block_with_witnesses(&block_data)
        .context("Failed to deserialize block")?;

    if tx_idx >= block.transactions.len() {
        return Err(anyhow::anyhow!("Transaction index {} out of range", tx_idx));
    }

    let tx = &block.transactions[tx_idx];
    if input_idx >= tx.inputs.len() {
        return Err(anyhow::anyhow!("Input index {} out of range", input_idx));
    }

    let input = &tx.inputs[input_idx];
    let prevout_txid_from_input = input.prevout.hash;
    let prevout_idx = input.prevout.index;

    println!("📋 Input prevout:");
    println!("  Txid (from input.prevout.hash): {}", hex::encode(prevout_txid_from_input));
    println!("  Output index: {}", prevout_idx);
    println!("");

    // Now search for the transaction that created this output
    println!("🔎 Searching for creating transaction...");
    let start_height: u64 = 0;
    let end_height: u64 = block_height; // Can't be created after it's spent
    
    let mut creating_block = None;
    let mut creating_tx_idx = None;
    let mut found = false;
    let mut checked = 0u64;

    let mut search_iter = ChunkedBlockIterator::new(&chunks_dir, Some(start_height), None)?
        .ok_or_else(|| anyhow::anyhow!("Failed to create search iterator"))?;

    let mut height = start_height;
    while height < end_height {
        let search_block_data = match search_iter.next_block()? {
            Some(data) => data,
            None => break,
        };

        let (search_block, _) = deserialize_block_with_witnesses(&search_block_data)
            .context("Failed to deserialize search block")?;

        for (idx, search_tx) in search_block.transactions.iter().enumerate() {
            let calculated_txid = calculate_tx_id(search_tx);
            if calculated_txid == prevout_txid_from_input {
                creating_block = Some(height);
                creating_tx_idx = Some(idx);
                found = true;
                
                println!("  ✅ FOUND creating transaction!");
                println!("  Block: {}", height);
                println!("  Transaction index: {}", idx);
                println!("  Calculated txid: {}", hex::encode(calculated_txid));
                println!("");
                
                // Check if output index is valid
                if prevout_idx < search_tx.outputs.len() as u64 {
                    println!("  ✅ Output index {} is valid (transaction has {} outputs)", 
                             prevout_idx, search_tx.outputs.len());
                } else {
                    println!("  ❌ Output index {} is INVALID (transaction has only {} outputs)", 
                             prevout_idx, search_tx.outputs.len());
                }
                
                break;
            }
        }

        if found {
            break;
        }

        height += 1;
        checked += 1;
        if checked % 50000 == 0 {
            println!("  Checked {} blocks... (current: {})", checked, height);
        }
    }

    if !found {
        println!("  ❌ Creating transaction NOT FOUND in blocks 0-{}", block_height);
        println!("  This means:");
        println!("    - Transaction was created AFTER block {} (impossible)", block_height);
        println!("    - OR there's a txid mismatch");
        println!("    - OR transaction is from outside processed range");
    } else {
        println!("✅ Txid consistency check:");
        println!("  Input.prevout.hash: {}", hex::encode(prevout_txid_from_input));
        if let Some(calc_txid) = creating_block.and_then(|_| {
            // Re-calculate to show it matches
            if let Some(tx_idx) = creating_tx_idx {
                // We'd need to reload, but for now just confirm
                Some(prevout_txid_from_input)
            } else {
                None
            }
        }) {
            println!("  Calculated txid: {}", hex::encode(calc_txid));
            println!("  ✅ MATCH!");
        }
    }

    Ok(())
}





