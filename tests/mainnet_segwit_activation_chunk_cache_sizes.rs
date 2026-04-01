//! Local chunked-cache smoke test around mainnet SegWit activation (raw byte lengths).
//! Requires `BLOCK_CACHE_DIR`. Run: `BLOCK_CACHE_DIR=/your/cache cargo test ... -- --ignored`
#![cfg(any(feature = "differential", feature = "scan"))]

use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_consensus::constants::SEGWIT_ACTIVATION_MAINNET;
use std::path::Path;

#[test]
#[ignore = "local chunk cache: set BLOCK_CACHE_DIR and run with --ignored"]
fn chunk_cache_sizes_around_mainnet_segwit_activation() {
    let root = std::env::var("BLOCK_CACHE_DIR").expect("BLOCK_CACHE_DIR");
    let chunks_dir = Path::new(&root);

    let segwit = SEGWIT_ACTIVATION_MAINNET;
    let start = segwit.saturating_sub(4);
    let mut iter = ChunkedBlockIterator::new(chunks_dir, Some(start), None)
        .expect("ChunkedBlockIterator::new failed")
        .expect("No iterator returned");

    for expected_height in start..start + 10 {
        match iter.next_block() {
            Ok(Some(data)) => {
                println!("Block {} has {} bytes", expected_height, data.len());
            }
            Ok(None) => {
                println!("Block {} - iterator returned None", expected_height);
            }
            Err(e) => {
                println!("Block {} - iterator error: {}", expected_height, e);
            }
        }
    }
}
