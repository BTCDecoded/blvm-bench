//! Investigate an unmatched transaction to determine the root cause
//! Loads the actual transaction from blockchain and checks if prevout exists

use anyhow::{Context, Result};
use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_consensus::serialization::block::deserialize_block_with_witnesses;
use blvm_consensus::block::calculate_tx_id;
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: {} <block_height> <tx_idx> <input_idx>", args[0]);
        std::process::exit(1);
    }

    let block_height: u64 = args[1].parse()?;
    let tx_idx: usize = args[2].parse()?;
    let input_idx: usize = args[3].parse()?;

    let chunks_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let chunks_dir = PathBuf::from(chunks_dir);

    println!("🔍 Investigating unmatched transaction:");
    println!("  Block: {}", block_height);
    println!("  Transaction index: {}", tx_idx);
    println!("  Input index: {}", input_idx);
    println!("");

    // Load the block
    let mut block_iter = ChunkedBlockIterator::new(&chunks_dir, Some(block_height), Some(1))?
        .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator"))?;

    let block_data = block_iter.next_block()?
        .ok_or_else(|| anyhow::anyhow!("Block {} not found", block_height))?;

    let (block, _witnesses) = deserialize_block_with_witnesses(&block_data)
        .context("Failed to deserialize block")?;

    if tx_idx >= block.transactions.len() {
        return Err(anyhow::anyhow!("Transaction index {} out of range (block has {} transactions)", tx_idx, block.transactions.len()));
    }

    let tx = &block.transactions[tx_idx];
    if input_idx >= tx.inputs.len() {
        return Err(anyhow::anyhow!("Input index {} out of range (transaction has {} inputs)", input_idx, tx.inputs.len()));
    }

    let input = &tx.inputs[input_idx];
    let prevout_txid = input.prevout.hash;
    let prevout_idx = input.prevout.index;

    println!("📋 Transaction details:");
    let tx_id = calculate_tx_id(tx);
    println!("  Transaction ID: {}", hex::encode(tx_id));
    println!("  Input {} prevout:", input_idx);
    println!("    Txid: {}", hex::encode(prevout_txid));
    println!("    Index: {}", prevout_idx);
    println!("");

    // Check if this is a null prevout (shouldn't happen for non-coinbase)
    if prevout_txid == [0u8; 32] && prevout_idx == 0xffffffff {
        println!("  ❌ NULL prevout detected!");
        println!("     This is a coinbase input - should have been skipped!");
        return Ok(());
    }

    // Check the raw transaction bytes to see the actual prevout hash
    println!("🔎 Checking raw transaction bytes...");
    use blvm_consensus::serialization::transaction::serialize_transaction;
    let tx_bytes = serialize_transaction(tx);
    
    // Find the input in the serialized bytes
    // Transaction format: version (4) + input_count (varint) + inputs...
    // Each input: prevout_hash (32) + prevout_index (4) + script_len (varint) + script + sequence (4)
    
    println!("  Transaction serialized size: {} bytes", tx_bytes.len());
    println!("  Number of inputs: {}", tx.inputs.len());
    
    // Parse the serialized transaction to find the raw prevout hash
    // Skip version (4 bytes)
    let mut offset = 4;
    
    // Read input count (varint)
    use blvm_consensus::serialization::decode_varint;
    let (input_count, varint_len) = decode_varint(&tx_bytes[offset..])?;
    offset += varint_len;
    
    println!("  Input count from serialization: {}", input_count);
    
    // Skip to the specific input
    for i in 0..input_idx {
        // Skip prevout hash (32)
        offset += 32;
        // Skip prevout index (4)
        offset += 4;
        // Read script length (varint)
        let (script_len, varint_len) = decode_varint(&tx_bytes[offset..])?;
        offset += varint_len;
        // Skip script
        offset += script_len as usize;
        // Skip sequence (4)
        offset += 4;
    }
    
    // Now we're at the target input
    if offset + 32 <= tx_bytes.len() {
        let raw_prevout_hash = &tx_bytes[offset..offset+32];
        println!("  Raw prevout hash from serialized bytes: {}", hex::encode(raw_prevout_hash));
        println!("  Extracted prevout hash: {}", hex::encode(prevout_txid));
        
        if raw_prevout_hash == prevout_txid.as_slice() {
            println!("  ✅ Hash matches - extraction is correct");
        } else {
            println!("  ❌ Hash mismatch - extraction bug!");
        }
    }
    
    // Check if this prevout_txid is actually a valid transaction ID format
    // Valid Bitcoin txids are SHA256 hashes - they should be random-looking
    // If it starts with 0xff, it's suspicious
    if prevout_txid[0] == 0xff {
        println!("");
        println!("  ⚠️  CRITICAL: Prevout txid starts with 0xff!");
        println!("     This is HIGHLY suspicious - valid SHA256 hashes are random.");
        println!("     This suggests:");
        println!("     1. Invalid/orphaned transaction (most likely)");
        println!("     2. Data corruption in blockchain data");
        println!("     3. Testnet/regtest data mixed with mainnet");
        println!("");
        println!("  💡 If Bitcoin Core accepted this block, the transaction must be valid.");
        println!("     But if the prevout doesn't exist, Bitcoin Core would reject it.");
        println!("     This is a contradiction - investigating further...");
    }

    Ok(())
}

