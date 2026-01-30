//! Investigate a specific script verification failure
//! Loads the transaction and prevout, then verifies with both BLVM and Bitcoin Core

use anyhow::{Context, Result};
use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_bench::start9_rpc_client::Start9RpcClient;
use blvm_consensus::serialization::block::deserialize_block_with_witnesses;
use blvm_consensus::block::calculate_tx_id;
use blvm_consensus::script::{verify_script_with_context_full, SigVersion};
use blvm_consensus::types::Network;
use blvm_consensus::serialization::transaction::serialize_transaction;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: {} <block_height> <tx_idx> <input_idx>", args[0]);
        eprintln!("Example: {} 211997 101 23", args[0]);
        std::process::exit(1);
    }

    let block_height: u64 = args[1].parse()?;
    let tx_idx: usize = args[2].parse()?;
    let input_idx: usize = args[3].parse()?;

    let chunks_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let chunks_dir = PathBuf::from(chunks_dir);

    println!("🔍 Investigating script verification failure:");
    println!("  Block: {}", block_height);
    println!("  Transaction index: {}", tx_idx);
    println!("  Input index: {}", input_idx);
    println!("");

    // Load the block
    let mut block_iter = ChunkedBlockIterator::new(&chunks_dir, Some(block_height), Some(1))?
        .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator"))?;

    let block_data = block_iter.next_block()?
        .ok_or_else(|| anyhow::anyhow!("Block {} not found", block_height))?;

    let (block, witnesses) = deserialize_block_with_witnesses(&block_data)
        .context("Failed to deserialize block")?;

    if tx_idx >= block.transactions.len() {
        return Err(anyhow::anyhow!("Transaction index {} out of range (block has {} transactions)", tx_idx, block.transactions.len()));
    }

    let tx = &block.transactions[tx_idx];
    if input_idx >= tx.inputs.len() {
        return Err(anyhow::anyhow!("Input index {} out of range (transaction has {} inputs)", input_idx, tx.inputs.len()));
    }

    let input = &tx.inputs[input_idx];
    let tx_id = calculate_tx_id(tx);
    let prevout_txid = input.prevout.hash;
    let prevout_idx = input.prevout.index;

    println!("📋 Transaction details:");
    println!("  Transaction ID: {}", hex::encode(tx_id));
    println!("  Input {} prevout:", input_idx);
    println!("    Prevout txid: {}", hex::encode(prevout_txid));
    println!("    Prevout index: {}", prevout_idx);
    println!("    Script sig: {}", hex::encode(&input.script_sig));
    println!("");

    // Load prevout from joined_sorted.bin
    println!("🔎 Loading prevout data...");
    use blvm_bench::sort_merge::merge_join::JoinedPrevout;
    use std::fs::File;
    use std::io::{BufReader, Read, Seek, SeekFrom};
    
    let joined_file = std::env::var("SORT_MERGE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain/sort_merge_data".to_string());
    let joined_file = PathBuf::from(joined_file).join("joined_sorted.bin");
    
    // Find the prevout in joined_sorted.bin using JoinedPrevout
    let file = File::open(&joined_file)?;
    let mut reader = BufReader::new(file);
    
    // Search for the joined prevout by spending location
    let mut found_prevout: Option<JoinedPrevout> = None;
    let mut buf = vec![0u8; 1024 * 1024]; // 1MB buffer
    
    reader.seek(SeekFrom::Start(0))?;
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        
        let mut pos = 0;
        while pos < n {
            if let Some((joined, consumed)) = JoinedPrevout::from_bytes(&buf[pos..]) {
                if joined.spending_block == block_height as u32 
                    && joined.spending_tx_idx == tx_idx as u32
                    && joined.spending_input_idx == input_idx as u32 {
                    found_prevout = Some(joined);
                    break;
                }
                pos += consumed;
            } else {
                break;
            }
        }
        
        if found_prevout.is_some() {
            break;
        }
    }
    
    if let Some(joined) = found_prevout {
        println!("  ✅ Found prevout!");
        println!("    Prevout block: {}", joined.prevout_height);
        println!("    Value: {} satoshis", joined.value);
        println!("    Script pubkey: {}", hex::encode(&joined.script_pubkey));
        println!("    Script pubkey length: {} bytes", joined.script_pubkey.len());
        println!("");
        
        // Try to verify with BLVM
        println!("🔐 Verifying script with BLVM...");
        let tx_witnesses = witnesses.get(tx_idx);
        let witness_stack = tx_witnesses.and_then(|w| w.get(input_idx));
        
        // Build prevouts list - need all prevouts for sighash calculation
        let mut prevouts = Vec::new();
        for (i, input) in tx.inputs.iter().enumerate() {
            if i == input_idx {
                prevouts.push(blvm_consensus::types::TransactionOutput {
                    value: joined.value,
                    script_pubkey: joined.script_pubkey.clone(),
                });
            } else {
                // Placeholder for other inputs (we don't have their prevouts here)
                // This might cause issues with sighash calculation, but let's try
                prevouts.push(blvm_consensus::types::TransactionOutput {
                    value: 0,
                    script_pubkey: vec![],
                });
            }
        }
        
        // Calculate script flags for this block
        // Use the same script flags calculation as step6
        use blvm_bench::sort_merge::verify;
        let mut flags = verify::get_script_flags(block_height, Network::Mainnet);
        
        // Add witness flag if transaction has witness data
        if tx_witnesses.is_some() {
            flags |= 0x80; // SCRIPT_VERIFY_WITNESS
        }
        
        match verify_script_with_context_full(
            &input.script_sig,
            &joined.script_pubkey,
            witness_stack,
            flags,
            tx,
            input_idx,
            &prevouts,
            Some(block_height),
            None,
            Network::Mainnet,
            SigVersion::Base,
        ) {
            Ok(true) => {
                println!("  ✅ BLVM verification: PASSED");
                println!("  ⚠️  But step6 reported failure - investigating further...");
            }
            Ok(false) => {
                println!("  ❌ BLVM verification: FAILED (returned false)");
                println!("  💡 This matches the step6 failure report");
            }
            Err(e) => {
                println!("  ❌ BLVM verification: ERROR");
                println!("  Error: {:?}", e);
            }
        }
        
        // Verify with Bitcoin Core
        println!("");
        println!("🔐 Verifying with Bitcoin Core...");
        let rpc_client = Start9RpcClient::new();
        
        // Get the block hash
        match rpc_client.get_block_hash(block_height).await {
            Ok(block_hash) => {
                println!("  Block hash: {}", block_hash);
                
                // Get the block from Core
                let core_block_hex = rpc_client.get_block_hex(&block_hash).await?;
                println!("  Block size: {} bytes", core_block_hex.len() / 2);
                
                // Serialize our transaction
                let tx_hex = hex::encode(serialize_transaction(tx));
                
                // Test with testmempoolaccept
                match rpc_client.test_mempool_accept(&tx_hex).await {
                    Ok(result) => {
                        println!("  Bitcoin Core testmempoolaccept:");
                        println!("  {}", serde_json::to_string_pretty(&result)?);
                        
                        if let Some(arr) = result.as_array() {
                            if let Some(first) = arr.get(0) {
                                if let Some(allowed) = first.get("allowed") {
                                    if allowed.as_bool().unwrap_or(false) {
                                        println!("  ✅ Bitcoin Core would ACCEPT this transaction");
                                        println!("  ⚠️  But BLVM rejected it - this is a CONSENSUS BUG!");
                                    } else {
                                        if let Some(reason) = first.get("reject-reason") {
                                            println!("  ❌ Bitcoin Core would REJECT: {}", reason);
                                            println!("  💡 Checking if this matches BLVM's behavior...");
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        println!("  ⚠️  Error testing with Bitcoin Core: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("  ⚠️  Block {} not found in Bitcoin Core: {}", block_height, e);
            }
        }
    } else {
        println!("  ❌ Prevout not found in joined_sorted.bin");
        println!("  💡 This might be a missing prevout issue");
    }

    Ok(())
}

