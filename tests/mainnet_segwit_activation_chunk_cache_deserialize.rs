//! Local chunked-cache smoke test: deserialize a window around mainnet SegWit activation.
#![cfg(any(feature = "differential", feature = "scan"))]

use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_protocol::constants::SEGWIT_ACTIVATION_MAINNET;
use blvm_protocol::serialization::block::deserialize_block_with_witnesses;
use std::path::Path;

#[test]
#[ignore = "local chunk cache: set BLOCK_CACHE_DIR and run with --ignored"]
fn chunk_cache_deserialize_window_around_mainnet_segwit_activation() {
    let root = std::env::var("BLOCK_CACHE_DIR").expect("BLOCK_CACHE_DIR");
    let chunks_dir = std::path::Path::new(&root);

    let segwit = SEGWIT_ACTIVATION_MAINNET;
    let start = segwit.saturating_sub(4);
    let mut iter = ChunkedBlockIterator::new(chunks_dir, Some(start), None)
        .expect("ChunkedBlockIterator::new failed")
        .expect("No iterator returned");

    for expected_height in start..start + 10 {
        match iter.next_block() {
            Ok(Some(data)) => {
                println!("Block {} has {} bytes", expected_height, data.len());
                match deserialize_block_with_witnesses(&data) {
                    Ok((block, witnesses)) => {
                        println!(
                            "  ✓ Deserialized: {} txs, {} witness sets",
                            block.transactions.len(),
                            witnesses.len()
                        );
                    }
                    Err(e) => {
                        println!("  ✗ Failed to deserialize: {}", e);
                        println!("  First 100 bytes: {:02x?}", &data[..100.min(data.len())]);
                    }
                }
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
