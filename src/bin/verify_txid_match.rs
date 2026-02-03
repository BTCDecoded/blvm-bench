//! Verify if prevout.hash (from step 1) matches calculated txid (from step 3)
//! Load a block, check an input's prevout.hash, then find that transaction and calculate its txid

use anyhow::{Context, Result};
use std::path::PathBuf;
use blvm_consensus::serialization::block::deserialize_block_with_witnesses;
use blvm_consensus::block::calculate_tx_id;
use blvm_consensus::types::Network;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 4 {
        eprintln!("Usage: verify_txid_match <block_height> <tx_idx> <input_idx>");
        eprintln!("  Loads block at height, checks input's prevout.hash,");
        eprintln!("  then verifies if calculated txid matches");
        return Ok(());
    }
    
    let block_height: u64 = args[1].parse()?;
    let tx_idx: usize = args[2].parse()?;
    let input_idx: usize = args[3].parse()?;
    
    let chunks_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let chunks_dir = PathBuf::from(chunks_dir);
    
    println!("Loading block {}...", block_height);
    
    // Load block
    use blvm_bench::chunked_cache::ChunkedBlockIterator;
    let mut iter = ChunkedBlockIterator::new(&chunks_dir, Some(block_height), Some(1))?
        .ok_or_else(|| anyhow::anyhow!("Failed to create iterator"))?;
    
    let block_data = iter.next_block()?
        .ok_or_else(|| anyhow::anyhow!("Block {} not found", block_height))?;
    
    let (block, _witnesses) = deserialize_block_with_witnesses(&block_data)?;
    
    println!("Block has {} transactions", block.transactions.len());
    
    if tx_idx >= block.transactions.len() {
        anyhow::bail!("Transaction index {} out of range (block has {} transactions)", tx_idx, block.transactions.len());
    }
    
    let tx = &block.transactions[tx_idx];
    
    if input_idx >= tx.inputs.len() {
        anyhow::bail!("Input index {} out of range (tx has {} inputs)", input_idx, tx.inputs.len());
    }
    
    let input = &tx.inputs[input_idx];
    let prevout_hash = input.prevout.hash;
    let prevout_idx = input.prevout.index;
    
    println!("\nInput prevout (from step 1 - read directly):");
    println!("  Txid: {}", hex::encode(&prevout_hash));
    println!("  Index: {}", prevout_idx);
    
    // Now we need to find the transaction that created this output
    // We'll search blocks before this one to find the transaction
    println!("\nSearching for transaction that created this output...");
    println!("(This may take a while - searching blocks {} to 0)", block_height);
    
    let mut found_tx: Option<(u64, usize, blvm_consensus::types::Transaction)> = None;
    
    // Search backwards from current block
    for search_height in (0..=block_height).rev() {
        if search_height == block_height {
            // Check current block (could be intra-block spending)
            for (tx_i, check_tx) in block.transactions.iter().enumerate() {
                let calculated_txid = calculate_tx_id(check_tx);
                if calculated_txid == prevout_hash {
                    // Found it! Check if it has the right output index
                    if prevout_idx < check_tx.outputs.len() as u64 {
                        found_tx = Some((search_height, tx_i, check_tx.clone()));
                        break;
                    }
                }
            }
            if found_tx.is_some() {
                break;
            }
            continue;
        }
        
        // Load block
        let mut search_iter = ChunkedBlockIterator::new(&chunks_dir, Some(search_height), Some(1))?
            .ok_or_else(|| anyhow::anyhow!("Failed to create iterator"))?;
        
        if let Ok(Some(search_data)) = search_iter.next_block() {
            if let Ok((search_block, _)) = deserialize_block_with_witnesses(&search_data) {
                for (tx_i, check_tx) in search_block.transactions.iter().enumerate() {
                    let calculated_txid = calculate_tx_id(check_tx);
                    if calculated_txid == prevout_hash {
                        // Found it! Check if it has the right output index
                        if prevout_idx < check_tx.outputs.len() as u64 {
                            found_tx = Some((search_height, tx_i, check_tx.clone()));
                            break;
                        }
                    }
                }
                if found_tx.is_some() {
                    break;
                }
            }
        }
        
        if search_height % 10000 == 0 {
            println!("  Searched down to block {}...", search_height);
        }
        
        // Limit search to reasonable range
        if block_height - search_height > 100000 {
            println!("  Stopping search at block {} (searched 100k blocks)", search_height);
            break;
        }
    }
    
    if let Some((found_height, found_tx_idx, found_transaction)) = found_tx {
        println!("\n✅ Found transaction!");
        println!("  Block: {}", found_height);
        println!("  Transaction index: {}", found_tx_idx);
        
        let calculated_txid = calculate_tx_id(&found_transaction);
        println!("  Calculated txid (step 3): {}", hex::encode(&calculated_txid));
        println!("  Prevout hash (step 1):     {}", hex::encode(&prevout_hash));
        
        if calculated_txid == prevout_hash {
            println!("\n✅ TXID MATCHES! Both methods produce the same result.");
            println!("   The missing prevout is NOT due to txid calculation mismatch.");
            println!("   Possible causes:");
            println!("   - Transaction is outside block range 0-912723");
            println!("   - Step 3 didn't extract this output for some reason");
        } else {
            println!("\n❌ TXID MISMATCH!");
            println!("   This is the root cause of missing prevouts!");
            println!("   Step 1 reads:        {}", hex::encode(&prevout_hash));
            println!("   Step 3 calculates:   {}", hex::encode(&calculated_txid));
            println!("   The merge-join fails because these don't match.");
        }
        
        if prevout_idx < found_transaction.outputs.len() as u64 {
            let output = &found_transaction.outputs[prevout_idx as usize];
            println!("\nOutput details:");
            println!("  Value: {} sat", output.value);
            println!("  ScriptPubkey length: {} bytes", output.script_pubkey.len());
        }
    } else {
        println!("\n❌ Transaction NOT FOUND in blocks 0-{}", block_height);
        println!("   This means the transaction creating this output:");
        println!("   - Was created AFTER block {} (impossible if we're spending it)", block_height);
        println!("   - Is from a different chain/fork");
        println!("   - Has a different txid than what's in the prevout.hash");
    }
    
    Ok(())
}







