//! Test to verify UTXO set state by reading blocks 0-15 and checking transaction outputs

use anyhow::Result;
use blvm_consensus::block::{connect_block, calculate_tx_id};
use blvm_consensus::serialization::block::deserialize_block_with_witnesses;
use blvm_consensus::types::{Network, UtxoSet};
use blvm_bench::block_file_reader::BlockFileReader;

#[tokio::test]
async fn test_verify_utxo_set_blocks_0_15() -> Result<()> {
    println!("🔍 Verifying UTXO Set State for Blocks 0-15");
    println!("==========================================\n");
    
    // Create block file reader for Start9 mount
    use blvm_bench::block_file_reader::Network as BlockFileNetwork;
    let start9_mount = dirs::home_dir().map(|h| h.join("mnt/bitcoin-start9"));
    let reader = if let Some(mount) = start9_mount.as_ref() {
        if mount.exists() {
            println!("✅ Using Start9 mount: {:?}", mount);
            BlockFileReader::new(mount, BlockFileNetwork::Mainnet)?
        } else {
            anyhow::bail!("Start9 mount not found at {:?}", mount);
        }
    } else {
        anyhow::bail!("Could not determine home directory");
    };
    
    let mut utxo_set: UtxoSet = UtxoSet::new();
    let network_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // Track all coinbase txids from blocks 0-14 for comparison
    let mut previous_coinbase_txids: Vec<([u8; 32], u64)> = Vec::new();
    
    // Process blocks 0-15
    println!("📊 Processing blocks 0-15...\n");
    println!("⚠️  CRITICAL: Reading directly from Start9 files (bypassing chunks)");
    println!("   This ensures we get ALL transactions, not just what's in the cache\n");
    
    // Temporarily disable chunk usage by setting BLOCK_CACHE_DIR to a non-existent path
    // This forces file reading instead of chunk reading
    std::env::set_var("BLOCK_CACHE_DIR", "/tmp/nonexistent-chunks-for-verification");
    
    // Force reading from files, not chunks
    let iterator = reader.read_blocks_sequential(Some(0), Some(16))?;
    
    for (idx, block_result) in iterator.enumerate() {
        let height = idx as u64;
        let block_bytes: Vec<u8> = block_result?;
        
        let (block, witnesses) = deserialize_block_with_witnesses(&block_bytes)?;
        
        // Count transactions
        let coinbase_count = block.transactions.iter()
            .filter(|tx| blvm_consensus::transaction::is_coinbase(tx))
            .count();
        let non_coinbase_count = block.transactions.len() - coinbase_count;
        
        // Calculate block hash for verification
        use sha2::{Digest, Sha256};
        let header_bytes = &block_bytes[0..80.min(block_bytes.len())];
        let first_hash = Sha256::digest(header_bytes);
        let second_hash = Sha256::digest(&first_hash);
        let mut block_hash: [u8; 32] = second_hash.as_slice().try_into().unwrap_or([0u8; 32]);
        block_hash.reverse(); // Convert to big-endian
        let block_hash_str: String = block_hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();
        
        println!("Block {}: {} transactions ({} coinbase, {} non-coinbase), block_hash (first 8) = {}", 
                 height, block.transactions.len(), coinbase_count, non_coinbase_count, block_hash_str);
        
        // Verify block 15 hash
        if height == 15 {
            let expected_hash = "00000000d1145790";
            if block_hash_str == expected_hash {
                println!("  ✅ Block hash matches - we have the correct block 15");
            } else {
                println!("  ❌ Block hash mismatch! Expected: {}, Got: {}", expected_hash, block_hash_str);
            }
        }
        
        // Calculate all transaction IDs in this block
        let block_txids: Vec<_> = block.transactions.iter()
            .map(|tx| calculate_tx_id(tx))
            .collect();
        
        // Store coinbase txid for future comparison (before block 15)
        if height < 15 && !block.transactions.is_empty() {
            let coinbase_tx = &block.transactions[0];
            let coinbase_txid = calculate_tx_id(coinbase_tx);
            previous_coinbase_txids.push((coinbase_txid, height));
        }
        
        // For block 15, check if TX 1's prevout matches any previous coinbase
        if height == 15 && block.transactions.len() >= 2 {
            let tx1 = &block.transactions[1];
            if !tx1.inputs.is_empty() {
                let raw_prevout_hash = tx1.inputs[0].prevout.hash;
                let raw_hash_str: String = raw_prevout_hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();
                println!("\n  🔍 CHECKING TX 1 PREVOUT AGAINST ALL PREVIOUS COINBASE TXIDS:");
                println!("     TX 1 prevout (first 8): {}", raw_hash_str);
                println!("     Full prevout hash:       {}", hex::encode(&raw_prevout_hash));
                
                // Check against all previous coinbase txids
                let mut found_match = false;
                for (cb_txid, cb_height) in &previous_coinbase_txids {
                    let cb_str: String = cb_txid.iter().take(8).map(|b| format!("{:02x}", b)).collect();
                    if raw_prevout_hash == *cb_txid {
                        println!("     ✅ EXACT MATCH with block {} coinbase: {}", cb_height, cb_str);
                        found_match = true;
                        break;
                    } else {
                        // Check reversed
                        let reversed_prevout: Vec<u8> = raw_prevout_hash.iter().rev().copied().collect();
                        let reversed_cb: Vec<u8> = cb_txid.iter().rev().copied().collect();
                        if reversed_prevout.as_slice() == reversed_cb.as_slice() {
                            println!("     ⚠️  MATCH when REVERSED with block {} coinbase: {}", cb_height, cb_str);
                            println!("     🔧 FIX NEEDED: Byte order issue in prevout hash!");
                            found_match = true;
                            break;
                        }
                    }
                }
                if !found_match {
                    println!("     ❌ No match found in any previous coinbase txid");
                    println!("     📋 Previous coinbase txids checked:");
                    for (cb_txid, cb_height) in &previous_coinbase_txids {
                        let cb_str: String = cb_txid.iter().take(8).map(|b| format!("{:02x}", b)).collect();
                        println!("        Block {}: {}", cb_height, cb_str);
                    }
                }
            }
        }
        
        // For block 15, verify coinbase txid matches expected
        if height == 15 && !block.transactions.is_empty() {
            let coinbase_tx = &block.transactions[0];
            let calculated_txid = calculate_tx_id(coinbase_tx);
            let calculated_str: String = calculated_txid.iter().take(8).map(|b| format!("{:02x}", b)).collect();
            let expected_str = "c997a5e56e104102";
            
            // Also check if reversing bytes matches
            let calculated_reversed: Vec<u8> = calculated_txid.iter().rev().copied().collect();
            let reversed_str: String = calculated_reversed.iter().take(8).map(|b| format!("{:02x}", b)).collect();
            
            println!("\n  🔍 COINBASE TXID VERIFICATION:");
            println!("     Calculated (first 8): {}", calculated_str);
            println!("     Calculated reversed:  {}", reversed_str);
            println!("     Expected (first 8):   {}", expected_str);
            println!("     Full calculated:     {}", hex::encode(&calculated_txid));
            println!("     Full expected:        c997a5e56e104102fa209c6a852dd90660a20b2d9c352423edce25857fcd3704");
            
            // Check serialization and manual hash calculation
            use blvm_consensus::serialization::transaction::serialize_transaction;
            let serialized = serialize_transaction(coinbase_tx);
            println!("     Serialized size:     {} bytes", serialized.len());
            println!("     Serialized (first 50): {}", hex::encode(&serialized[..serialized.len().min(50)]));
            println!("     Serialized (FULL):    {}", hex::encode(&serialized));
            
            // Also check the raw block bytes to see what the actual transaction looks like
            // Find where the coinbase transaction starts (after 80-byte header + varint for tx count)
            if block_bytes.len() > 80 {
                let mut offset = 80;
                // Skip varint for transaction count
                if offset < block_bytes.len() && block_bytes[offset] < 0xfd {
                    offset += 1;
                }
                // The coinbase transaction starts here
                if offset < block_bytes.len() {
                    let coinbase_in_block = &block_bytes[offset..];
                    let coinbase_len = coinbase_in_block.len().min(134);
                    println!("     Raw in block (first {}): {}", coinbase_len, hex::encode(&coinbase_in_block[..coinbase_len]));
                    if coinbase_len >= 134 {
                        println!("     Raw in block (FULL):    {}", hex::encode(&coinbase_in_block[..134]));
                    }
                }
            }
            
            // Manually calculate hash to verify
            use blvm_consensus::crypto::OptimizedSha256;
            let manual_hash = OptimizedSha256::new().hash256(&serialized);
            let manual_str: String = manual_hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();
            println!("     Manual hash (first 8): {}", manual_str);
            println!("     Full manual hash:      {}", hex::encode(&manual_hash));
            
            // Check if manual hash matches expected
            let expected_full = hex::decode("c997a5e56e104102fa209c6a852dd90660a20b2d9c352423edce25857fcd3704").unwrap();
            if manual_hash == expected_full.as_slice().try_into().unwrap_or([0u8; 32]) {
                println!("     ✅ Manual hash MATCHES expected!");
                println!("     ❌ But calculate_tx_id returned different value - CACHING BUG!");
            } else {
                println!("     ❌ Manual hash also doesn't match - serialization or hash issue");
            }
            
            if calculated_str == expected_str {
                println!("     ✅ MATCH!");
            } else if reversed_str == expected_str {
                println!("     ⚠️  MATCH when reversed - endianness issue!");
            } else {
                println!("     ❌ MISMATCH - Our calculation is wrong!");
            }
        }
        
        // Log non-coinbase transactions and their outputs
        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            if !blvm_consensus::transaction::is_coinbase(tx) {
                let tx_id = calculate_tx_id(tx);
                let txid_str: String = tx_id.iter().take(8).map(|b| format!("{:02x}", b)).collect();
                println!("  TX {}: txid (first 8) = {}, {} inputs, {} outputs",
                         tx_idx, txid_str, tx.inputs.len(), tx.outputs.len());
                
                // Check inputs
                for (input_idx, input) in tx.inputs.iter().enumerate() {
                    let hash_str: String = input.prevout.hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();
                    if let Some(utxo) = utxo_set.get(&input.prevout) {
                        println!("    Input {}: prevout {}:{} ✅ EXISTS (value={}, height={}, coinbase={})",
                                 input_idx, hash_str, input.prevout.index, 
                                 utxo.value, utxo.height, utxo.is_coinbase);
                    } else {
                        println!("    Input {}: prevout {}:{} ❌ MISSING",
                                 input_idx, hash_str, input.prevout.index);
                        
                        // Check if this is from an earlier transaction in this block
                        if tx_idx > 0 {
                            println!("      🔍 Checking if from earlier transaction in this block...");
                            for prev_tx_idx in 0..tx_idx {
                                let prev_txid_str: String = block_txids[prev_tx_idx].iter().take(8).map(|b| format!("{:02x}", b)).collect();
                                if input.prevout.hash == block_txids[prev_tx_idx] {
                                    println!("      ✅ MATCH! This is from TX {} (txid: {}) in this block, output index: {}",
                                             prev_tx_idx, prev_txid_str, input.prevout.index);
                                    println!("      🔍 This is an intra-block dependency - should be available in temp_utxo_set");
                                } else {
                                    println!("      Not from TX {} (txid: {})", prev_tx_idx, prev_txid_str);
                                }
                            }
                        }
                    }
                }
                
                // Log outputs (these will be added to UTXO set)
                for (output_idx, output) in tx.outputs.iter().enumerate() {
                    println!("    Output {}: value={} (will create UTXO {}:{})", 
                             output_idx, output.value, 
                             hex::encode(&tx_id[..8]), output_idx);
                }
            } else {
                // Log coinbase transaction ID for reference
                let tx_id = calculate_tx_id(tx);
                let txid_str: String = tx_id.iter().take(8).map(|b| format!("{:02x}", b)).collect();
                println!("  TX {} (coinbase): txid (first 8) = {}, {} outputs",
                         tx_idx, txid_str, tx.outputs.len());
            }
        }
        
        // Connect block to update UTXO set
        // CRITICAL: For block 15, let's see what happens if we use temp_utxo_set approach
        // (simulating what should happen in production path)
        let mut temp_utxo_set = utxo_set.clone();
        let mut block_valid = true;
        
        // Apply transactions incrementally (like production path does)
        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            let tx_id = calculate_tx_id(tx);
            let txid_str: String = tx_id.iter().take(8).map(|b| format!("{:02x}", b)).collect();
            
            if !blvm_consensus::transaction::is_coinbase(tx) {
                // Check if inputs exist in temp_utxo_set
                for (input_idx, input) in tx.inputs.iter().enumerate() {
                    let hash_str: String = input.prevout.hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();
                    if !temp_utxo_set.contains_key(&input.prevout) {
                        println!("    ⚠️  TX {} input {}: prevout {}:{} NOT in temp_utxo_set", 
                                 tx_idx, input_idx, hash_str, input.prevout.index);
                        // Check if it's from an earlier transaction in this block
                        for prev_tx_idx in 0..tx_idx {
                            if input.prevout.hash == block_txids[prev_tx_idx] {
                                println!("      ✅ But it IS from TX {} in this block - should be in temp_utxo_set!", prev_tx_idx);
                            }
                        }
                    }
                }
            }
            
            // Apply transaction to temp_utxo_set
            use blvm_consensus::block::apply_transaction;
            match apply_transaction(tx, temp_utxo_set.clone(), height) {
                Ok((new_temp_utxo_set, _)) => {
                    temp_utxo_set = new_temp_utxo_set;
                }
                Err(e) => {
                    println!("    ❌ Failed to apply TX {} to temp_utxo_set: {:?}", tx_idx, e);
                    block_valid = false;
                }
            }
        }
        
        let (result, new_utxo_set, _undo_log) = connect_block(
            &block,
            &witnesses,
            utxo_set.clone(),
            height,
            None,
            network_time,
            Network::Mainnet,
        )?;
        
        match result {
            blvm_consensus::types::ValidationResult::Valid => {
                utxo_set = new_utxo_set;
                println!("  ✅ Block {} validated successfully", height);
            }
            blvm_consensus::types::ValidationResult::Invalid(msg) => {
                println!("  ❌ Block {} validation failed: {}", height, msg);
                // Continue to see UTXO set state
            }
        }
        
        // Show UTXO set size after this block
        let coinbase_utxos = utxo_set.iter()
            .filter(|(_, utxo)| utxo.is_coinbase)
            .count();
        let non_coinbase_utxos = utxo_set.len() - coinbase_utxos;
        
        println!("  UTXO set: {} total ({} coinbase, {} non-coinbase)\n", 
                 utxo_set.len(), coinbase_utxos, non_coinbase_utxos);
    }
    
    // Final check: Look for the missing UTXO
    println!("\n🔍 Checking for missing UTXO: c997a5e56e104102:0");
    println!("================================================\n");
    
    let target_hash = hex::decode("c997a5e56e104102fa209c6a852dd90660a20b2d9c352423edce25857fcd3704")?;
    let mut target_hash_array = [0u8; 32];
    target_hash_array.copy_from_slice(&target_hash);
    
    let target_outpoint = blvm_consensus::types::OutPoint {
        hash: target_hash_array,
        index: 0,
    };
    
    if let Some(utxo) = utxo_set.get(&target_outpoint) {
        println!("✅ Found! value={}, height={}, coinbase={}", 
                 utxo.value, utxo.height, utxo.is_coinbase);
    } else {
        println!("❌ Not found in UTXO set");
        println!("\n📋 Checking all UTXOs with similar hash prefix...");
        let mut found_similar = false;
        for (outpoint, utxo) in utxo_set.iter() {
            if outpoint.hash[..8] == target_hash_array[..8] {
                found_similar = true;
                let hash_str: String = outpoint.hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();
                println!("  Similar: {}:{} (value={}, height={}, coinbase={})",
                         hash_str, outpoint.index, utxo.value, utxo.height, utxo.is_coinbase);
            }
        }
        if !found_similar {
            println!("  No UTXOs with similar hash prefix found");
        }
    }
    
    Ok(())
}
