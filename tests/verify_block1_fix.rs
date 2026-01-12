//! Test to verify block 1 is found and chained correctly

#[cfg(feature = "differential")]
use anyhow::Result;
#[cfg(feature = "differential")]
use blvm_bench::chunk_index::build_block_index;
#[cfg(feature = "differential")]
use std::path::PathBuf;

#[test]
#[cfg(feature = "differential")]
fn verify_block1_fix() -> Result<()> {
    // Try to find chunks directory
    let chunks_dir = if let Ok(dir) = std::env::var("BLOCK_CACHE_DIR") {
        PathBuf::from(dir)
    } else {
        // Try common locations
        let paths = vec![
            PathBuf::from("/run/media/acolyte/Extra/blockchain"),
            PathBuf::from("/home/acolyte/.cache/blvm-bench/chunks"),
        ];
        paths.into_iter()
            .find(|p| p.exists())
            .ok_or_else(|| anyhow::anyhow!("No chunks directory found. Set BLOCK_CACHE_DIR or ensure chunks exist"))?
    };
    
    println!("🔍 Testing block 1 detection and chaining...");
    println!("   Chunks directory: {:?}", chunks_dir);
    
    // Build the index directly
    let index = build_block_index(&chunks_dir)?;
    
    // Check if block 1 is in the index
    if let Some(block1_entry) = index.get(&1) {
        println!("\n✅✅✅ SUCCESS: Block 1 found in index!");
        println!("   Chunk: {}, Offset: {}", block1_entry.chunk_number, block1_entry.offset_in_chunk);
        println!("   Block hash: {}", hex::encode(&block1_entry.block_hash));
        
        // Verify it's the correct block 1 hash
        let expected_block1_hash = hex::decode("00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048")?;
        let mut expected_hash_array = [0u8; 32];
        expected_hash_array.copy_from_slice(&expected_block1_hash);
        
        if block1_entry.block_hash == expected_hash_array {
            println!("   ✅ Block hash matches expected block 1 hash!");
        } else {
            eprintln!("   ❌ Block hash does NOT match expected block 1 hash!");
            eprintln!("      Expected: {}", hex::encode(&expected_hash_array));
            eprintln!("      Found: {}", hex::encode(&block1_entry.block_hash));
            anyhow::bail!("Block 1 hash mismatch");
        }
        
        // Check if we have at least a few blocks chained
        if index.len() >= 2 {
            println!("   ✅ Index has {} blocks (at least genesis and block 1)", index.len());
            Ok(())
        } else {
            eprintln!("   ❌ Index only has {} block(s) - chaining failed", index.len());
            anyhow::bail!("Chaining failed - only {} blocks indexed", index.len());
        }
    } else {
        eprintln!("\n❌❌❌ FAILURE: Block 1 NOT found in index!");
        eprintln!("   Index only has {} block(s)", index.len());
        if index.len() > 0 {
            if let Some(genesis) = index.get(&0) {
                eprintln!("   Genesis: chunk {}, offset {}", genesis.chunk_number, genesis.offset_in_chunk);
            }
        }
        anyhow::bail!("Block 1 not found in index");
    }
}




