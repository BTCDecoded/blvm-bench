use std::path::PathBuf;

fn main() {
    let chunks_dir = PathBuf::from("/run/media/acolyte/Extra/blockchain");
    
    // Load the index
    match blvm_bench::chunk_index::load_block_index(&chunks_dir) {
        Ok(Some(index)) => {
            println!("Index has {} entries", index.len());
            
            // Check for missing blocks in range 0-912722
            let max_height = 912722u64;
            let mut missing = Vec::new();
            for h in 0..=max_height {
                if !index.contains_key(&h) {
                    missing.push(h);
                }
            }
            
            if missing.is_empty() {
                println!("✅ No missing blocks! Index is complete for heights 0-{}", max_height);
            } else {
                println!("❌ Missing {} block(s):", missing.len());
                for h in &missing[..missing.len().min(10)] {
                    println!("  Height: {}", h);
                }
                if missing.len() > 10 {
                    println!("  ... and {} more", missing.len() - 10);
                }
            }
        }
        Ok(None) => println!("No index found"),
        Err(e) => println!("Error loading index: {}", e),
    }
}
