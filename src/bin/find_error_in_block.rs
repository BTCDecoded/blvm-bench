//! Find script errors in a specific block

use anyhow::{Context, Result};
use blvm_consensus::serialization::block::deserialize_block_with_witnesses;
use blvm_consensus::transaction::is_coinbase;
use blvm_consensus::types::{Network, TransactionOutput};
use blvm_consensus::script::{verify_script_with_context_full, SigVersion};
use blvm_consensus::segwit::Witness;
use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_bench::sort_merge::verify::get_script_flags;
use blvm_bench::sort_merge::merge_join::JoinedPrevout;
use blvm_consensus::bip113::get_median_time_past;
use blvm_consensus::serialization::block::deserialize_block_header;
use blvm_consensus::types::BlockHeader;
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: find_error_in_block <block_height>");
        std::process::exit(1);
    }
    
    let block_height: u64 = args[1].parse().context("Invalid block height")?;
    
    println!("🔍 Finding script errors in block {}...", block_height);
    
    // Load block
    let chunks_dir = Path::new("/run/media/acolyte/Extra/blockchain/chunks");
    let mut block_iter = ChunkedBlockIterator::new(chunks_dir, Some(block_height), Some(1))?
        .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator"))?;
    
    let block_data = block_iter.next_block()?
        .ok_or_else(|| anyhow::anyhow!("Block {} not found", block_height))?;
    
    // Extract header for median_time_past
    let block_header = deserialize_block_header(&block_data[..80.min(block_data.len())])?;
    let median_time_past = Some(get_median_time_past(&[block_header.clone()]));
    
    // Deserialize block
    let (block, witnesses) = deserialize_block_with_witnesses(&block_data)
        .context("Failed to deserialize block")?;
    
    // Load prevouts
    let prevouts_file = Path::new("/run/media/acolyte/Extra/blockchain/sort_merge_data/joined_sorted.bin");
    
    // Use PrevoutReader to read prevouts for this block
    use blvm_bench::sort_merge::verify::PrevoutReader;
    let mut prevout_reader = PrevoutReader::new(prevouts_file)?;
    prevout_reader.skip_to_block(block_height as u32)?;
    let block_prevouts = prevout_reader.read_block_prevouts(block_height as u32)?;
    
    println!("  Loaded {} prevouts for block {}", block_prevouts.len(), block_height);
    
    // Build prevout map
    use std::collections::HashMap;
    let mut prevout_map: HashMap<(u32, u32), &JoinedPrevout> = HashMap::new();
    for prevout in &block_prevouts {
        prevout_map.insert(
            (prevout.spending_tx_idx, prevout.spending_input_idx),
            prevout
        );
    }
    
    let flags = get_script_flags(block_height, Network::Mainnet);
    let network = Network::Mainnet;
    
    // Try to verify all scripts and catch errors
    for (tx_idx, tx) in block.transactions.iter().enumerate() {
        if is_coinbase(tx) {
            continue;
        }
        
        let tx_witnesses = witnesses.get(tx_idx);
        
        for (input_idx, input) in tx.inputs.iter().enumerate() {
            if let Some(prevout) = prevout_map.get(&(tx_idx as u32, input_idx as u32)) {
                let prevout_script = prevout.script_pubkey.clone();
                let witness_stack: Option<&Witness> = tx_witnesses
                    .and_then(|witnesses| witnesses.get(input_idx));
                
                let mut tx_flags = flags;
                if witness_stack.is_some() && block_height >= 481824 {
                    tx_flags |= 0x800; // SCRIPT_VERIFY_WITNESS
                }
                
                // Build all prevouts for this transaction
                let mut all_prevouts = Vec::new();
                for (i, _input) in tx.inputs.iter().enumerate() {
                    if let Some(p) = prevout_map.get(&(tx_idx as u32, i as u32)) {
                        all_prevouts.push(TransactionOutput {
                            value: p.value,
                            script_pubkey: p.script_pubkey.clone(),
                        });
                    } else {
                        all_prevouts.push(TransactionOutput {
                            value: 0,
                            script_pubkey: vec![],
                        });
                    }
                }
                
                match verify_script_with_context_full(
                    &input.script_sig,
                    &prevout_script,
                    witness_stack,
                    tx_flags,
                    tx,
                    input_idx,
                    &all_prevouts,
                    Some(block_height),
                    median_time_past,
                    network,
                    SigVersion::Base,
                ) {
                    Ok(true) => {},
                    Ok(false) => {},
                    Err(e) => {
                        println!("❌ ERROR at block {}, tx {}, input {}: {:?}", 
                                block_height, tx_idx, input_idx, e);
                        println!("   Script sig: {}", hex::encode(&input.script_sig));
                        println!("   Script pubkey: {}", hex::encode(&prevout_script));
                        return Ok(());
                    }
                }
            }
        }
    }
    
    println!("  ✅ No errors found in block {}", block_height);
    Ok(())
}

