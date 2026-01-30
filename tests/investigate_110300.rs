//! Investigate the script validation failure at block 110300
#![cfg(feature = "differential")]

use anyhow::Result;
use blvm_bench::checkpoint_persistence::CheckpointManager;
use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_consensus::UtxoSet;
use std::path::PathBuf;

#[tokio::test]
async fn investigate_block_110300() -> Result<()> {
    let cache_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let cache_path = PathBuf::from(&cache_dir);
    
    let manager = CheckpointManager::new(&cache_path)?;
    
    // Load checkpoint at 99999 (closest to 110300)
    println!("\n🔍 Loading checkpoint at 99999...");
    let utxo_set = manager.load_utxo_checkpoint(99999)?
        .ok_or_else(|| anyhow::anyhow!("Checkpoint 99999 not found"))?;
    println!("   Loaded {} UTXOs", utxo_set.len());
    
    // Process blocks 100000 to 110300
    println!("\n📦 Processing blocks 100000 to 110300...");
    let mut current_utxo = utxo_set;
    
    let mut chunked_iter = ChunkedBlockIterator::new(&cache_path, Some(100000), Some(10301))?
        .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator"))?;
    
    for height in 100000..=110300 {
        let block_bytes = chunked_iter.next_block()?
            .ok_or_else(|| anyhow::anyhow!("Block {} not found", height))?;
        
        let (block, witnesses) = blvm_consensus::serialization::block::deserialize_block_with_witnesses(&block_bytes)?;
        
        let network = blvm_consensus::types::Network::Mainnet;
        if height == 110300 {
            println!("\n🔍 Block 110300 details:");
            println!("   Transactions: {}", block.transactions.len());
            
            // Look at transaction 16 specifically BEFORE validation
            if block.transactions.len() > 16 {
                let tx = &block.transactions[16];
                println!("\n🔍 Transaction 16 details:");
                println!("   Inputs: {}", tx.inputs.len());
                println!("   Outputs: {}", tx.outputs.len());
                
                for (i, input) in tx.inputs.iter().enumerate() {
                    let hash_hex: String = input.prevout.hash.iter().map(|b| format!("{:02x}", b)).collect();
                    println!("   Input {}: {}:{}", i, hash_hex, input.prevout.index);
                    println!("      script_sig len: {}", input.script_sig.len());
                    println!("      script_sig hex: {}", hex::encode(&input.script_sig));
                    
                    // Check if UTXO exists
                    if let Some(utxo) = current_utxo.get(&input.prevout) {
                        println!("      UTXO found: value={}, script_pubkey_len={}", utxo.value, utxo.script_pubkey.len());
                        println!("      script_pubkey hex: {}", hex::encode(&utxo.script_pubkey));
                    } else {
                        println!("      ❌ UTXO NOT FOUND");
                    }
                }
                
                for (i, output) in tx.outputs.iter().enumerate() {
                    println!("   Output {}: value={}, script_pubkey_len={}", i, output.value, output.script_pubkey.len());
                }
            }
        }
        
        let (result, new_utxo, _) = blvm_consensus::block::connect_block(
            &block,
            &witnesses,
            current_utxo,
            height,
            None,
            block.header.timestamp,
            network,
        )?;
        
        if height == 110300 {
            println!("\n   Validation result: {:?}", result);
            if let blvm_consensus::ValidationResult::Invalid(msg) = &result {
                println!("\n❌ INVALID: {}", msg);
            }
            break;
        }
        
        current_utxo = new_utxo;
        
        if height % 1000 == 0 {
            println!("   Processed block {} ({} UTXOs)", height, current_utxo.len());
        }
    }
    
    Ok(())
}

