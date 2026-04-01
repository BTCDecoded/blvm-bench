//! Quick test to find block 1 in chunks

#[cfg(feature = "differential")]
use anyhow::Result;
#[cfg(feature = "differential")]
use blvm_bench::chunk_index::build_block_index;
#[cfg(feature = "differential")]
use std::path::PathBuf;

#[test]
#[ignore = "local chunk cache: set BLOCK_CACHE_DIR and run with --ignored"]
#[cfg(feature = "differential")]
fn find_block1_quick() -> Result<()> {
    let chunks_dir = PathBuf::from(std::env::var("BLOCK_CACHE_DIR").expect("BLOCK_CACHE_DIR"));

    println!("🔍 Searching for block 1 in chunks at: {:?}", chunks_dir);

    // Build index - this will show us if block 1 is found
    let (index, _hash_map) = build_block_index(&chunks_dir)?;

    println!("✅ Index built with {} entries", index.len());

    if index.contains_key(&1) {
        let entry = &index[&1];
        println!(
            "✅ Block 1 found! chunk={}, offset={}, hash={}",
            entry.chunk_number,
            entry.offset_in_chunk,
            hex::encode(&entry.block_hash[..8])
        );
    } else {
        println!("❌ Block 1 NOT in index!");
        println!(
            "   Index has heights: {:?}",
            index.keys().take(10).collect::<Vec<_>>()
        );
    }

    Ok(())
}
