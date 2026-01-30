use std::path::Path;
use blvm_bench::chunked_cache::ChunkedBlockIterator;

#[test]
fn test_block_481824_size() {
    let chunks_dir = Path::new("/run/media/acolyte/Extra/blockchain");
    
    let mut iter = ChunkedBlockIterator::new(chunks_dir, Some(481820), None)
        .expect("ChunkedBlockIterator::new failed")
        .expect("No iterator returned");
    
    for expected_height in 481820..481830 {
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
