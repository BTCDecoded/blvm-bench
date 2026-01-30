//! Quick check: Load block 481929, tx 168, input 0 and inspect the dummy element

use anyhow::{Context, Result};
use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_consensus::serialization::block::deserialize_block_with_witnesses;
use blvm_consensus::block::calculate_tx_id;
use std::path::PathBuf;

fn main() -> Result<()> {
    let block_height = 481929u64;
    let tx_idx = 168;
    let input_idx = 0;

    let chunks_dir = PathBuf::from("/run/media/acolyte/Extra/blockchain");

    println!("🔍 Quick check: Block {}, tx {}, input {}", block_height, tx_idx, input_idx);
    
    let mut block_iter = ChunkedBlockIterator::new(&chunks_dir, Some(block_height), Some(1))?
        .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator"))?;

    let block_data = block_iter.next_block()?
        .ok_or_else(|| anyhow::anyhow!("Block {} not found", block_height))?;

    let (block, witnesses) = deserialize_block_with_witnesses(&block_data)
        .context("Failed to deserialize block")?;

    let tx = &block.transactions[tx_idx];
    let input = &tx.inputs[input_idx];
    
    println!("\n📋 Transaction details:");
    println!("  Tx ID: {}", hex::encode(calculate_tx_id(tx)));
    println!("  Script sig: {}", hex::encode(&input.script_sig));
    
    // Parse script_sig to find the dummy element
    // For OP_CHECKMULTISIG, the stack is: [dummy] [sig1] ... [sigm] [m] [pubkey1] ... [pubkeyn] [n]
    // The dummy is the FIRST element consumed (last on stack before OP_CHECKMULTISIG)
    
    // Check if script_sig contains OP_CHECKMULTISIG (0xae)
    if input.script_sig.contains(&0xae) {
        println!("  ✅ Contains OP_CHECKMULTISIG (0xae)");
        
        // Find the position of OP_CHECKMULTISIG
        if let Some(pos) = input.script_sig.iter().position(|&b| b == 0xae) {
            println!("  OP_CHECKMULTISIG at position: {}", pos);
            
            // The dummy element is pushed just before OP_CHECKMULTISIG
            // We need to parse backwards from position to find the last push
            // For now, let's just check what's right before 0xae
            if pos > 0 {
                println!("  Byte before OP_CHECKMULTISIG: 0x{:02x}", input.script_sig[pos - 1]);
            }
            
            // Check last few bytes
            let start = pos.saturating_sub(10);
            println!("  Last 10 bytes before OP_CHECKMULTISIG: {}", 
                hex::encode(&input.script_sig[start..pos]));
        }
    }
    
    // Check script_pubkey
    // We need to get it from the prevout, but for now let's just report what we have
    println!("\n💡 To fully analyze, we need the prevout script_pubkey");
    println!("   This requires loading from joined_sorted.bin");

    Ok(())
}

