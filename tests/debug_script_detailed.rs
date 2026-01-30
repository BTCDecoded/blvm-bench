//! Detailed debug of script validation at block 110300 TX 16
#![cfg(feature = "differential")]

use anyhow::Result;
use blvm_consensus::script::{verify_script_with_context_full, SigVersion};
use blvm_consensus::types::{Transaction, TransactionOutput, TransactionInput, OutPoint, Network};
use blvm_consensus::transaction_hash::{calculate_transaction_sighash, SighashType};

#[test]
fn debug_script_detailed() -> Result<()> {
    // Transaction 16 from block 110300
    let script_sig = hex::decode(
        "493046022100d23459d03ed7e9511a47d13292d3430a04627de6235b6e51a40f9cd386f2abe3022100e7d25b080f0bb8d8d5f878bba7d54ad2fda650ea8d158a33ee3cbd11768191fd004104b0e2c879e4daf7b9ab68350228c159766676a14f5815084ba166432aab46198d4cca98fa3e9981d0a90b2effc514b76279476550ba3663fdcaff94c38420e9d5"
    )?;
    
    let script_pubkey = hex::decode("76a914dc44b1164188067c3a32d4780f5996fa14a4f2d988ac")?;
    
    // Create the transaction (minimal - just what we need for sighash)
    let prevout_hash = hex::decode("0276b76b07f4935c70acf54fbf1f438a4c397a9fb7e633873c4dd3bc062b6b40")?;
    let mut prevout_hash_arr = [0u8; 32];
    prevout_hash_arr.copy_from_slice(&prevout_hash);
    
    let tx = Transaction {
        version: 1,
        inputs: vec![TransactionInput {
            prevout: OutPoint {
                hash: prevout_hash_arr,
                index: 0,
            },
            script_sig: script_sig.clone(),
            sequence: 0xffffffff,
        }].into(),
        outputs: vec![TransactionOutput {
            value: 4000000,
            script_pubkey: hex::decode("76a914dc44b1164188067c3a32d4780f5996fa14a4f2d988ac")?,
        }].into(),
        lock_time: 0,
    };
    
    // Create prevouts (the UTXO being spent)
    let prevouts = vec![TransactionOutput {
        value: 5000000,
        script_pubkey: script_pubkey.clone(),
    }];
    
    println!("=== Transaction Details ===");
    println!("  Version: {}", tx.version);
    println!("  Inputs: {}", tx.inputs.len());
    println!("  Outputs: {}", tx.outputs.len());
    println!("  Locktime: {}", tx.lock_time);
    
    // Extract sighash byte
    let sighash_byte = script_sig[73]; // After 73-byte signature (0x49 + 72 bytes + 1 sighash)
    println!("\n=== Sighash Analysis ===");
    println!("  Sighash byte position: 73");
    println!("  Sighash byte value: 0x{:02x}", sighash_byte);
    
    // Parse sighash type
    let sighash_type = SighashType::from_byte(sighash_byte);
    println!("  Sighash type: {:?}", sighash_type);
    
    // Calculate sighash manually
    if let Ok(st) = sighash_type {
        println!("\n=== Computing Sighash ===");
        let sighash = calculate_transaction_sighash(&tx, 0, &prevouts, st)?;
        println!("  Sighash: {}", hex::encode(&sighash));
    }
    
    // Verify script with full context
    println!("\n=== Script Verification ===");
    let flags = 0; // No special flags for early blocks
    
    let result = verify_script_with_context_full(
        &script_sig,
        &script_pubkey,
        None, // No witness
        flags,
        &tx,
        0, // input_index
        &prevouts,
        Some(110300), // block_height
        None, // median_time_past
        Network::Mainnet,
        SigVersion::Base,
    )?;
    
    println!("  Result: {}", result);
    
    if !result {
        println!("\n❌ Script verification failed!");
        
        // Try to diagnose why
        // Extract signature and pubkey
        let sig_len = script_sig[0] as usize;
        let sig_bytes = &script_sig[1..1+sig_len];
        let pubkey_start = 1 + sig_len;
        let pubkey_len = script_sig[pubkey_start] as usize;
        let pubkey_bytes = &script_sig[pubkey_start+1..pubkey_start+1+pubkey_len];
        
        println!("\n  Signature ({} bytes): {}", sig_bytes.len(), hex::encode(sig_bytes));
        println!("  Pubkey ({} bytes): {}", pubkey_bytes.len(), hex::encode(pubkey_bytes));
        
        // Check DER signature
        let der_sig = &sig_bytes[..sig_bytes.len()-1]; // Remove sighash byte
        println!("\n  DER signature ({} bytes): {}", der_sig.len(), hex::encode(der_sig));
        
        // Try to parse the signature
        use secp256k1::{Secp256k1, Message, PublicKey};
        use secp256k1::ecdsa::Signature;
        
        let secp = Secp256k1::new();
        
        match Signature::from_der(der_sig) {
            Ok(sig) => {
                println!("  ✅ DER signature parsed successfully");
                
                match PublicKey::from_slice(pubkey_bytes) {
                    Ok(pk) => {
                        println!("  ✅ Public key parsed successfully");
                        
                        // Try verification with the sighash we computed
                        if let Ok(st) = SighashType::from_byte(sighash_byte) {
                            let sighash = calculate_transaction_sighash(&tx, 0, &prevouts, st)?;
                            match Message::from_digest_slice(&sighash) {
                                Ok(msg) => {
                                    let mut normalized_sig = sig;
                                    normalized_sig.normalize_s();
                                    
                                    let verify_result = secp.verify_ecdsa(&msg, &normalized_sig, &pk);
                                    println!("  Direct verification: {:?}", verify_result);
                                }
                                Err(e) => println!("  ❌ Message creation failed: {:?}", e),
                            }
                        }
                    }
                    Err(e) => println!("  ❌ Public key parse failed: {:?}", e),
                }
            }
            Err(e) => println!("  ❌ DER signature parse failed: {:?}", e),
        }
    }
    
    Ok(())
}

