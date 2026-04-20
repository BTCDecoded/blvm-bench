//! Validate existing checkpoints by running full verification and comparing UTXO sets
#![cfg(feature = "differential")]

use anyhow::Result;
use blvm_bench::checkpoint_persistence::CheckpointManager;
use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_protocol::UtxoSet;
use std::path::PathBuf;

#[tokio::test]
#[ignore = "local chunk cache + checkpoints: set BLOCK_CACHE_DIR and run with --ignored"]
async fn test_validate_checkpoints() -> Result<()> {
    let cache_path = PathBuf::from(std::env::var("BLOCK_CACHE_DIR").expect("BLOCK_CACHE_DIR"));

    let manager = CheckpointManager::new(&cache_path)?;

    // Find all existing checkpoints
    let checkpoint_dir = cache_path.join("differential_checkpoints");
    let mut checkpoint_heights: Vec<u64> = Vec::new();

    for entry in std::fs::read_dir(&checkpoint_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("utxo_") && name_str.ends_with(".bin") {
            if let Some(h) = name_str
                .strip_prefix("utxo_")
                .and_then(|s| s.strip_suffix(".bin"))
                .and_then(|s| s.parse().ok())
            {
                checkpoint_heights.push(h);
            }
        }
    }
    checkpoint_heights.sort();

    println!("\n🔍 CHECKPOINT VALIDATION");
    println!(
        "   Found {} checkpoints to validate: {:?}",
        checkpoint_heights.len(),
        checkpoint_heights
    );

    if checkpoint_heights.is_empty() {
        println!("   No checkpoints to validate!");
        return Ok(());
    }

    // Validate each checkpoint by building UTXO from scratch
    let max_height = *checkpoint_heights.last().unwrap();

    println!(
        "\n📦 Building UTXO set from block 0 to {} with FULL verification...",
        max_height
    );

    let mut utxo_set = UtxoSet::default();
    let mut chunked_iter =
        ChunkedBlockIterator::new(&cache_path, Some(0), Some((max_height + 1) as usize))?
            .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator"))?;

    for height in 0..=max_height {
        let block_bytes = chunked_iter
            .next_block()?
            .ok_or_else(|| anyhow::anyhow!("Block {} not found", height))?;

        // Parse block with witnesses
        let (block, witnesses) =
            blvm_protocol::serialization::block::deserialize_block_with_witnesses(&block_bytes)?;

        // Full validation (no assume-valid)
        let network = blvm_protocol::types::Network::Mainnet;
        let ctx = blvm_protocol::block::block_validation_context_for_connect_ibd(
            None::<&[blvm_protocol::types::BlockHeader]>,
            block.header.timestamp,
            network,
        );
        let (result, new_utxo, _) =
            blvm_protocol::block::connect_block(&block, &witnesses, utxo_set, height, &ctx)?;

        utxo_set = new_utxo;

        if !matches!(result, blvm_protocol::ValidationResult::Valid) {
            println!("❌ Block {} is INVALID: {:?}", height, result);
            return Err(anyhow::anyhow!("Block {} validation failed", height));
        }

        // Check against saved checkpoint at this height
        if checkpoint_heights.contains(&height) {
            println!("\n   🔍 Validating checkpoint at height {}...", height);

            let saved_utxo = manager
                .load_utxo_checkpoint(height)?
                .ok_or_else(|| anyhow::anyhow!("Checkpoint {} not found", height))?;

            // Compare UTXO sets
            if utxo_set.len() != saved_utxo.len() {
                println!(
                    "   ❌ MISMATCH at height {}: built {} UTXOs, saved {} UTXOs",
                    height,
                    utxo_set.len(),
                    saved_utxo.len()
                );
                return Err(anyhow::anyhow!("Checkpoint mismatch at height {}", height));
            }

            // Deep comparison - check all entries match
            let mut mismatches = 0;
            for (outpoint, utxo) in utxo_set.iter() {
                match saved_utxo.get(outpoint) {
                    Some(saved)
                        if saved.value == utxo.value
                            && saved.script_pubkey == utxo.script_pubkey => {}
                    _ => mismatches += 1,
                }
            }

            if mismatches > 0 {
                println!(
                    "   ❌ MISMATCH at height {}: {} UTXOs differ",
                    height, mismatches
                );
                return Err(anyhow::anyhow!(
                    "Checkpoint content mismatch at height {}",
                    height
                ));
            }

            println!(
                "   ✅ Checkpoint at height {} VALID ({} UTXOs match)",
                height,
                utxo_set.len()
            );
        }

        if height % 10000 == 0 {
            println!(
                "   Progress: block {}/{} ({} UTXOs)",
                height,
                max_height,
                utxo_set.len()
            );
        }
    }

    println!("\n✅ ALL CHECKPOINTS VALIDATED!");
    Ok(())
}
