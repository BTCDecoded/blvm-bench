//! Investigate divergences found during differential testing
#![cfg(feature = "differential")]

use anyhow::Result;
use blvm_bench::checkpoint_persistence::CheckpointManager;
use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_consensus::script::{verify_script_with_context_full, SigVersion};
use blvm_consensus::types::{TransactionOutput, Network};
use blvm_consensus::transaction_hash::{calculate_transaction_sighash, SighashType};
use std::path::PathBuf;

#[tokio::test]
async fn investigate_block_124276() -> Result<()> {
    investigate_block(124276, 4).await
}

#[tokio::test] 
async fn investigate_block_129878() -> Result<()> {
    investigate_block(129878, 7).await
}

#[tokio::test]
async fn investigate_block_131326() -> Result<()> {
    investigate_block(131326, 8).await
}

#[tokio::test]
async fn investigate_block_134181() -> Result<()> {
    investigate_block(134181, 62).await
}

async fn investigate_block(target_height: u64, target_tx: usize) -> Result<()> {
    let cache_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let cache_path = PathBuf::from(&cache_dir);
    
    let manager = CheckpointManager::new(&cache_path)?;
    
    // Find closest checkpoint before target - use 25k intervals with _NNNNN format
    let checkpoint_height = (target_height / 25000) * 25000;
    println!("\n🔍 Investigating block {} TX {}...", target_height, target_tx);
    println!("   Loading checkpoint at {}...", checkpoint_height);
    
    let utxo_set = manager.load_utxo_checkpoint(checkpoint_height)?
        .ok_or_else(|| anyhow::anyhow!("Checkpoint {} not found", checkpoint_height))?;
    println!("   Loaded {} UTXOs", utxo_set.len());
    
    // Process blocks from checkpoint to target
    let blocks_to_process = (target_height - checkpoint_height) as usize;
    println!("   Processing {} blocks to reach target...", blocks_to_process);
    
    let mut current_utxo = utxo_set;
    let mut chunked_iter = ChunkedBlockIterator::new(&cache_path, Some(checkpoint_height + 1), Some(blocks_to_process + 1))?
        .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator"))?;
    
    for height in (checkpoint_height + 1)..=target_height {
        let block_bytes = chunked_iter.next_block()?
            .ok_or_else(|| anyhow::anyhow!("Block {} not found", height))?;
        
        let (block, witnesses) = blvm_consensus::serialization::block::deserialize_block_with_witnesses(&block_bytes)?;
        
        if height == target_height {
            println!("\n🔍 Block {} details:", height);
            println!("   Transactions: {}", block.transactions.len());
            
            if target_tx < block.transactions.len() {
                let tx = &block.transactions[target_tx];
                println!("\n🔍 Transaction {} details:", target_tx);
                println!("   Version: {}", tx.version);
                println!("   Inputs: {}", tx.inputs.len());
                println!("   Outputs: {}", tx.outputs.len());
                
                // Build prevouts
                let mut prevouts: Vec<TransactionOutput> = Vec::new();
                for (i, input) in tx.inputs.iter().enumerate() {
                    if let Some(utxo) = current_utxo.get(&input.prevout) {
                        prevouts.push(TransactionOutput {
                            value: utxo.value,
                            script_pubkey: utxo.script_pubkey.clone(),
                        });
                    } else {
                        println!("   ❌ Input {} UTXO not found!", i);
                        return Ok(());
                    }
                }
                
                // Check each input's script
                for (i, input) in tx.inputs.iter().enumerate() {
                    let script_sig = &input.script_sig;
                    let script_pubkey = &prevouts[i].script_pubkey;
                    
                    println!("\n   Input {}:", i);
                    println!("      script_sig ({} bytes): {}", script_sig.len(), 
                             if script_sig.len() <= 100 { hex::encode(script_sig) } else { format!("{}...", hex::encode(&script_sig[..50])) });
                    println!("      script_pubkey ({} bytes): {}", script_pubkey.len(), hex::encode(script_pubkey));
                    
                    // Extract sighash byte if present
                    if script_sig.len() > 1 {
                        let first_push_len = script_sig[0] as usize;
                        if first_push_len > 0 && first_push_len < script_sig.len() {
                            let sighash_byte = script_sig[first_push_len];
                            println!("      Sighash byte: 0x{:02x}", sighash_byte);
                            
                            match SighashType::from_byte(sighash_byte) {
                                Ok(st) => println!("      Sighash type: {:?}", st),
                                Err(e) => println!("      ⚠️  Sighash parse error: {:?}", e),
                            }
                        }
                    }
                    
                    // Try to verify - flags=0 since P2SH/BIP66 not active at this height
                    let flags = 0;
                    let result = verify_script_with_context_full(
                        script_sig,
                        script_pubkey,
                        None,
                        flags,
                        tx,
                        i,
                        &prevouts,
                        Some(target_height),
                        None,
                        Network::Mainnet,
                        SigVersion::Base,
                    );
                    
                    match result {
                        Ok(valid) => println!("      Verification: {}", if valid { "✅ VALID" } else { "❌ INVALID" }),
                        Err(e) => println!("      Verification error: {:?}", e),
                    }
                }
            }
            
            // NOW call connect_block on the target block to see if it actually passes
            println!("\n🔍 Testing connect_block on block {}...", height);
            let network = Network::Mainnet;
            let (result, _, _) = blvm_consensus::block::connect_block(
                &block,
                &witnesses,
                current_utxo.clone(),
                height,
                None,
                block.header.timestamp,
                network,
            )?;
            match &result {
                blvm_consensus::types::ValidationResult::Valid => {
                    println!("   ✅ Block {} PASSED connect_block!", height);
                }
                blvm_consensus::types::ValidationResult::Invalid(msg) => {
                    println!("   ❌ Block {} FAILED connect_block: {}", height, msg);
                }
            }
            break;
        }
        
        // Apply block to UTXO set
        let network = Network::Mainnet;
        let (result, new_utxo, _) = blvm_consensus::block::connect_block(
            &block,
            &witnesses,
            current_utxo,
            height,
            None,
            block.header.timestamp,
            network,
        )?;
        
        // Check result of connect_block
        match &result {
            blvm_consensus::types::ValidationResult::Valid => {
                if height == target_height {
                    println!("   ✅ Block {} PASSED connect_block!", height);
                }
                current_utxo = new_utxo;
            }
            blvm_consensus::types::ValidationResult::Invalid(msg) => {
                println!("   ❌ Block {} FAILED connect_block: {}", height, msg);
                // Continue anyway to see if we can debug further
                current_utxo = new_utxo;
            }
        }
        
        if height % 5000 == 0 {
            println!("   Processed block {} ({} UTXOs)", height, current_utxo.len());
        }
    }
    
    Ok(())
}

