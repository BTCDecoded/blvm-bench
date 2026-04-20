//! Direct signature verification test
//! Tests a specific signature with specific flags to identify the root cause

use blvm_protocol::activation::ForkActivationTable;
use blvm_protocol::constants::BIP66_ACTIVATION_MAINNET;
use blvm_protocol::types::Network;
use secp256k1::ecdsa::Signature;

fn main() {
    // Signature from a historic mainnet repro (post–BIP66 activation, tx index 275, input 0)
    // First signature from script_sig: 004730440220392b46c67976c4db35e347bc4f8bff5b5237720c510ea54301f9a8b11fc0431902206e25990fd863590d24cfd5dbdb9b4033bd75e680ff80abacbc641745c8899bad01
    let sig_hex = "30440220392b46c67976c4db35e347bc4f8bff5b5237720c510ea54301f9a8b11fc0431902206e25990fd863590d24cfd5dbdb9b4033bd75e680ff80abacbc641745c8899bad01";
    let signature_bytes = hex::decode(sig_hex).unwrap();

    println!("Testing signature:");
    println!("  Hex: {}", sig_hex);
    println!("  Length: {} bytes", signature_bytes.len());

    // Extract DER signature (without sighash byte)
    let der_sig = &signature_bytes[..signature_bytes.len() - 1];
    let sighash_byte = signature_bytes[signature_bytes.len() - 1];
    println!("  DER length: {} bytes", der_sig.len());
    println!("  Sighash type: 0x{:02x}", sighash_byte);

    // Test BIP66 check (strict DER): mainnet height with BIP66 enforced (activation at BIP66_ACTIVATION_MAINNET)
    println!("\n🔍 Testing BIP66 check...");
    let height = BIP66_ACTIVATION_MAINNET + 1;
    let network = Network::Mainnet;
    let activation = ForkActivationTable::from_network(network);
    let bip66_result = blvm_protocol::bip_validation::check_bip66(der_sig, height, &activation);
    println!("  BIP66 check result: {:?}", bip66_result);

    // Test signature parsing
    println!("\n🔍 Testing signature parsing...");
    match Signature::from_der(der_sig) {
        Ok(sig) => {
            println!("  ✅ Signature parsed successfully");

            // Test LOW_S check
            println!("\n🔍 Testing LOW_S check...");
            let original_compact = sig.serialize_compact();
            let mut normalized = sig;
            normalized.normalize_s();
            let normalized_compact = normalized.serialize_compact();

            println!("  Original compact: {}", hex::encode(original_compact));
            println!("  Normalized compact: {}", hex::encode(normalized_compact));

            if original_compact == normalized_compact {
                println!("  ✅ Signature is already low-S");
            } else {
                println!("  ⚠️  Signature has high S (normalize changed it)");
            }
        }
        Err(e) => {
            println!("  ❌ Failed to parse signature: {:?}", e);
        }
    }

    println!("\n✅ Test complete - signature parsing and checks done");
}
