//! Debug the REAL transaction at block 110300 TX 16
#![cfg(feature = "differential")]

use anyhow::Result;
use blvm_bench::checkpoint_persistence::CheckpointManager;
use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_consensus::script::{verify_script_with_context_full, SigVersion};
use blvm_consensus::types::{TransactionOutput, Network};
use blvm_consensus::transaction_hash::{calculate_transaction_sighash, SighashType};
use std::path::PathBuf;

#[tokio::test]
async fn debug_real_110300() -> Result<()> {
    let cache_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let cache_path = PathBuf::from(&cache_dir);
    
    let manager = CheckpointManager::new(&cache_path)?;
    
    // Load checkpoint at 99999
    println!("\n🔍 Loading checkpoint at 99999...");
    let utxo_set = manager.load_utxo_checkpoint(99999)?
        .ok_or_else(|| anyhow::anyhow!("Checkpoint 99999 not found"))?;
    println!("   Loaded {} UTXOs", utxo_set.len());
    
    // Process blocks 100000 to 110300 to get the correct UTXO set
    println!("\n📦 Processing blocks 100000 to 110299...");
    let mut current_utxo = utxo_set;
    
    let mut chunked_iter = ChunkedBlockIterator::new(&cache_path, Some(100000), Some(10301))?
        .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator"))?;
    
    for height in 100000..=110299 {
        let block_bytes = chunked_iter.next_block()?
            .ok_or_else(|| anyhow::anyhow!("Block {} not found", height))?;
        
        let (block, witnesses) = blvm_consensus::serialization::block::deserialize_block_with_witnesses(&block_bytes)?;
        
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
        
        if !matches!(result, blvm_consensus::ValidationResult::Valid) {
            println!("❌ Block {} invalid: {:?}", height, result);
            return Ok(());
        }
        
        current_utxo = new_utxo;
        
        if height % 2000 == 0 {
            println!("   Processed block {} ({} UTXOs)", height, current_utxo.len());
        }
    }
    
    // Now get block 110300
    println!("\n🔍 Loading block 110300...");
    let block_bytes = chunked_iter.next_block()?
        .ok_or_else(|| anyhow::anyhow!("Block 110300 not found"))?;
    
    let (block, witnesses) = blvm_consensus::serialization::block::deserialize_block_with_witnesses(&block_bytes)?;
    
    println!("   Block has {} transactions", block.transactions.len());
    
    // Look at TX 16
    let tx_idx = 16;
    let tx = &block.transactions[tx_idx];
    
    println!("\n🔍 Transaction {} details:", tx_idx);
    println!("   Version: {}", tx.version);
    println!("   Inputs: {}", tx.inputs.len());
    println!("   Outputs: {}", tx.outputs.len());
    println!("   Locktime: {}", tx.lock_time);
    
    // Build prevouts from UTXO set
    println!("\n🔍 Building prevouts from UTXO set...");
    let mut prevouts: Vec<TransactionOutput> = Vec::new();
    for (input_idx, input) in tx.inputs.iter().enumerate() {
        let hash_hex: String = input.prevout.hash.iter().map(|b| format!("{:02x}", b)).collect();
        println!("   Input {}: {}:{}", input_idx, hash_hex, input.prevout.index);
        
        if let Some(utxo) = current_utxo.get(&input.prevout) {
            println!("      ✅ UTXO found: value={}", utxo.value);
            prevouts.push(TransactionOutput {
                value: utxo.value,
                script_pubkey: utxo.script_pubkey.clone(),
            });
        } else {
            println!("      ❌ UTXO NOT FOUND!");
            return Err(anyhow::anyhow!("UTXO not found for input {}", input_idx));
        }
    }
    
    // Now verify input 0
    let input_idx = 0;
    let input = &tx.inputs[input_idx];
    let script_sig = &input.script_sig;
    let script_pubkey = &prevouts[input_idx].script_pubkey;
    
    println!("\n🔍 Verifying input {}...", input_idx);
    println!("   script_sig ({} bytes): {}", script_sig.len(), hex::encode(script_sig));
    println!("   script_pubkey ({} bytes): {}", script_pubkey.len(), hex::encode(script_pubkey));
    
    // Extract sighash byte
    let sig_len = script_sig[0] as usize;
    let sighash_byte = script_sig[sig_len];
    println!("   Sighash byte: 0x{:02x}", sighash_byte);
    
    // Calculate sighash
    let sighash_type = SighashType::from_byte(sighash_byte)?;
    println!("   Sighash type: {:?}", sighash_type);
    
    let sighash = calculate_transaction_sighash(tx, input_idx, &prevouts, sighash_type)?;
    println!("   Computed sighash: {}", hex::encode(&sighash));
    
    // Verify with context
    let flags = 0;
    let result = verify_script_with_context_full(
        script_sig,
        script_pubkey,
        None,
        flags,
        tx,
        input_idx,
        &prevouts,
        Some(110300),
        None,
        Network::Mainnet,
        SigVersion::Base,
    )?;
    
    println!("\n   Verification result: {}", if result { "✅ VALID" } else { "❌ INVALID" });
    
    if !result {
        // Manual signature verification
        let der_sig = &script_sig[1..1+sig_len-1]; // Skip push byte and sighash byte
        let pubkey_start = 1 + sig_len;
        let pubkey_len = script_sig[pubkey_start] as usize;
        let pubkey_bytes = &script_sig[pubkey_start+1..pubkey_start+1+pubkey_len];
        
        use secp256k1::{Secp256k1, Message, PublicKey};
        use secp256k1::ecdsa::Signature;
        
        let secp = Secp256k1::new();
        
        println!("\n   Manual verification:");
        println!("   DER sig ({} bytes): {}", der_sig.len(), hex::encode(der_sig));
        println!("   Pubkey ({} bytes): {}", pubkey_bytes.len(), hex::encode(pubkey_bytes));
        
        match Signature::from_der(der_sig) {
            Ok(sig) => {
                println!("   ✅ Signature parsed");
                match PublicKey::from_slice(pubkey_bytes) {
                    Ok(pk) => {
                        println!("   ✅ Pubkey parsed");
                        match Message::from_digest_slice(&sighash) {
                            Ok(msg) => {
                                let mut normalized = sig;
                                normalized.normalize_s();
                                let verify = secp.verify_ecdsa(&msg, &normalized, &pk);
                                println!("   Verification: {:?}", verify);
                            }
                            Err(e) => println!("   ❌ Message error: {:?}", e),
                        }
                    }
                    Err(e) => println!("   ❌ Pubkey error: {:?}", e),
                }
            }
            Err(e) => println!("   ❌ Signature error: {:?}", e),
        }
    }
    
    Ok(())
}

