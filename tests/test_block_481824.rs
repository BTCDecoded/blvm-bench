use std::path::Path;
use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_consensus::serialization::block::deserialize_block_with_witnesses;

#[test]
fn test_block_481824() {
    let chunks_dir = Path::new("/run/media/acolyte/Extra/blockchain");
    
    // Start from block 481820 (a few before the problem block)
    let mut iter = ChunkedBlockIterator::new(chunks_dir, Some(481820), None)
        .expect("ChunkedBlockIterator::new failed")
        .expect("No iterator returned");
    
    for expected_height in 481820..481830 {
        match iter.next_block() {
            Ok(Some(data)) => {
                println!("Block {} has {} bytes", expected_height, data.len());
                match deserialize_block_with_witnesses(&data) {
                    Ok((block, witnesses)) => {
                        println!("  ✓ Deserialized: {} txs, {} witness sets", 
                            block.transactions.len(),
                            witnesses.len());
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
