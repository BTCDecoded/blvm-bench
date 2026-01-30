//! Verify unmatched transactions with Bitcoin Core RPC
//! Checks if Bitcoin Core accepts/rejects these transactions and why

use anyhow::{Context, Result};
use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_bench::start9_rpc_client::Start9RpcClient;
use blvm_consensus::serialization::block::deserialize_block_with_witnesses;
use blvm_consensus::block::calculate_tx_id;
use blvm_consensus::serialization::transaction::serialize_transaction;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: {} <block_height> <tx_idx> <input_idx>", args[0]);
        eprintln!("Example: {} 478118 26 0", args[0]);
        std::process::exit(1);
    }

    let block_height: u64 = args[1].parse()?;
    let tx_idx: usize = args[2].parse()?;
    let input_idx: usize = args[3].parse()?;

    let chunks_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let chunks_dir = PathBuf::from(chunks_dir);

    println!("🔍 Verifying transaction with Bitcoin Core:");
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
    let tx_id = calculate_tx_id(tx);

    println!("📋 Transaction details:");
    println!("  Transaction ID: {}", hex::encode(tx_id));
    println!("  Input {} prevout:", input_idx);
    println!("    Prevout txid: {}", hex::encode(prevout_txid));
    println!("    Prevout index: {}", prevout_idx);
    println!("");

    // Connect to Bitcoin Core via Start9 RPC
    println!("🔌 Connecting to Bitcoin Core via Start9 RPC...");
    let rpc_client = Start9RpcClient::new();
    
    // Test connection
    match rpc_client.get_block_count().await {
        Ok(height) => {
            println!("  ✅ Connected to Bitcoin Core");
            println!("  Current block height: {}", height);
        }
        Err(e) => {
            eprintln!("  ❌ Failed to connect to Bitcoin Core: {}", e);
            return Err(e);
        }
    }
    println!("");

    // Check if the block exists in Bitcoin Core
    println!("🔎 Checking block {} in Bitcoin Core...", block_height);
    let block_hash_result = rpc_client.get_block_hash(block_height).await;
    
    match block_hash_result {
        Ok(block_hash) => {
            println!("  ✅ Block {} exists in Bitcoin Core", block_height);
            println!("  Block hash: {}", block_hash);
            
            // Get the block from Bitcoin Core
            let core_block_hex = rpc_client.get_block_hex(&block_hash).await?;
            println!("  Block hex length from Core: {} bytes", core_block_hex.len());
            
            // Check if the transaction exists in this block (compare hex)
            let tx_hex = hex::encode(serialize_transaction(tx));
            if core_block_hex.contains(&tx_hex) {
                println!("  ✅ Transaction found in Bitcoin Core's block");
            } else {
                println!("  ⚠️  Transaction NOT found in Bitcoin Core's block (might be different serialization)");
            }
        }
        Err(e) => {
            println!("  ❌ Block {} NOT found in Bitcoin Core: {}", block_height, e);
            println!("  💡 This suggests the block is from a side chain or reorg!");
            return Ok(());
        }
    }
    println!("");

    // Check if the prevout transaction exists
    println!("🔎 Checking if prevout transaction exists in Bitcoin Core...");
    println!("  Looking for txid: {}", hex::encode(prevout_txid));
    
    // Try to get the transaction
    let prevout_tx_result = rpc_client.get_raw_transaction(&hex::encode(prevout_txid), false).await;
    
    match prevout_tx_result {
        Ok(prevout_tx_hex) => {
            println!("  ✅ Prevout transaction EXISTS in Bitcoin Core!");
            println!("  Transaction hex length: {} bytes", prevout_tx_hex.len());
            
            // Check if the output index exists
            // Parse the transaction to count outputs
            // For now, just report that it exists
            println!("  💡 The prevout exists, so this should be matched!");
            println!("  ⚠️  This suggests an EXTRACTION BUG - output wasn't extracted!");
        }
        Err(e) => {
            let error_msg = e.to_string();
            if error_msg.contains("not found") || error_msg.contains("No such mempool transaction") {
                println!("  ❌ Prevout transaction NOT FOUND in Bitcoin Core");
                println!("  💡 This confirms the prevout doesn't exist!");
                println!("  💡 Bitcoin Core would REJECT this transaction!");
                println!("  💡 This transaction is INVALID/ORPHANED!");
            } else {
                println!("  ⚠️  Error checking prevout: {}", e);
            }
        }
    }
    println!("");

    // Try to test the transaction with testmempoolaccept
    println!("🔎 Testing transaction with testmempoolaccept...");
    let tx_hex = hex::encode(serialize_transaction(tx));
    let test_result = rpc_client.test_mempool_accept(&tx_hex).await;
    
    match test_result {
        Ok(result) => {
            println!("  Bitcoin Core testmempoolaccept result:");
            println!("  {}", serde_json::to_string_pretty(&result)?);
            
            if let Some(allowed) = result.get("allowed") {
                if allowed.as_bool().unwrap_or(false) {
                    println!("  ✅ Bitcoin Core would ACCEPT this transaction!");
                    println!("  ⚠️  But prevout doesn't exist - this is contradictory!");
                } else {
                    println!("  ❌ Bitcoin Core would REJECT this transaction");
                    if let Some(reject_reason) = result.get("reject-reason") {
                        println!("  Reason: {}", reject_reason);
                    }
                }
            }
        }
        Err(e) => {
            println!("  ⚠️  Error testing transaction: {}", e);
        }
    }

    Ok(())
}

