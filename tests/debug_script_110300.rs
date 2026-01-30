//! Debug the script validation failure at block 110300 TX 16
#![cfg(feature = "differential")]

use anyhow::Result;
use blvm_consensus::script::{verify_script, verify_script_with_context_full, SigVersion};
use blvm_consensus::types::{Transaction, TransactionOutput, Network};

#[test]
fn debug_script_110300_tx16() -> Result<()> {
    // Transaction 16 from block 110300
    // Input 0: 0276b76b07f4935c70acf54fbf1f438a4c397a9fb7e633873c4dd3bc062b6b40:0
    
    let script_sig = hex::decode(
        "493046022100d23459d03ed7e9511a47d13292d3430a04627de6235b6e51a40f9cd386f2abe3022100e7d25b080f0bb8d8d5f878bba7d54ad2fda650ea8d158a33ee3cbd11768191fd004104b0e2c879e4daf7b9ab68350228c159766676a14f5815084ba166432aab46198d4cca98fa3e9981d0a90b2effc514b76279476550ba3663fdcaff94c38420e9d5"
    )?;
    
    let script_pubkey = hex::decode("76a914dc44b1164188067c3a32d4780f5996fa14a4f2d988ac")?;
    
    println!("Script sig: {} bytes", script_sig.len());
    println!("Script pubkey: {} bytes", script_pubkey.len());
    
    // Try basic verify_script without transaction context
    println!("\n--- Testing basic verify_script (no sighash) ---");
    let basic_result = verify_script(&script_sig, &script_pubkey, None, 0)?;
    println!("Basic verify_script result: {}", basic_result);
    
    // The script_pubkey is P2PKH: OP_DUP OP_HASH160 <20 bytes> OP_EQUALVERIFY OP_CHECKSIG
    println!("\n--- Decoding scripts ---");
    println!("Script pubkey opcodes:");
    for (i, byte) in script_pubkey.iter().enumerate() {
        let op = match byte {
            0x76 => "OP_DUP".to_string(),
            0xa9 => "OP_HASH160".to_string(),
            0x14 => "PUSH 20 bytes".to_string(),
            0x88 => "OP_EQUALVERIFY".to_string(),
            0xac => "OP_CHECKSIG".to_string(),
            x if *x <= 0x4b => format!("PUSH {} bytes", x),
            x => format!("0x{:02x}", x),
        };
        println!("  [{}] 0x{:02x} = {}", i, byte, op);
    }
    
    println!("\nScript sig opcodes:");
    let mut pos = 0;
    while pos < script_sig.len() {
        let byte = script_sig[pos];
        if byte <= 0x4b {
            // Push data
            let len = byte as usize;
            println!("  [{}] PUSH {} bytes", pos, len);
            if pos + 1 + len <= script_sig.len() {
                let data = &script_sig[pos + 1..pos + 1 + len];
                if len == 73 || len == 72 || len == 71 {
                    // Likely a signature
                    let sighash_byte = data[data.len() - 1];
                    println!("      Signature: {} bytes, sighash=0x{:02x}", data.len() - 1, sighash_byte);
                } else if len == 65 || len == 33 {
                    // Likely a pubkey
                    let key_type = if data[0] == 0x04 { "uncompressed" } else if data[0] == 0x02 || data[0] == 0x03 { "compressed" } else { "unknown" };
                    println!("      Pubkey: {} ({})", key_type, len);
                }
            }
            pos += 1 + len;
        } else {
            println!("  [{}] 0x{:02x}", pos, byte);
            pos += 1;
        }
    }
    
    // Extract the sighash byte
    let sig_len = script_sig[0] as usize;
    let sighash_byte = script_sig[sig_len]; // Last byte of signature
    println!("\n--- Sighash Analysis ---");
    println!("Sighash byte: 0x{:02x}", sighash_byte);
    
    // Test sighash type parsing
    match blvm_consensus::transaction_hash::SighashType::from_byte(sighash_byte) {
        Ok(st) => println!("Sighash type parsed: {:?}", st),
        Err(e) => println!("Sighash type parse error: {:?}", e),
    }
    
    Ok(())
}





