//! Collection-only test - fast block collection without validation
//! Validation happens during chunking

#[cfg(feature = "differential")]
use anyhow::Result;
#[cfg(feature = "differential")]
use blvm_bench::block_file_reader::{BlockFileReader, Network as BlockFileNetwork};
#[cfg(feature = "differential")]
use std::path::PathBuf;

/// Collect blocks only (no validation during collection)
/// Validation happens during chunking
#[tokio::test]
#[ignore = "local BITCOIN_DATA_DIR / BLOCK_CACHE_DIR: run with --ignored"]
#[cfg(feature = "differential")]
async fn collect_blocks_only() -> Result<()> {
    println!("🚀 Starting collection-only mode");
    println!("   Blocks will be validated during chunking");

    // Get data directory from environment or auto-detect
    let data_dir = std::env::var("BITCOIN_DATA_DIR").ok().map(PathBuf::from);

    let cache_dir = std::env::var("BLOCK_CACHE_DIR").ok().map(PathBuf::from);

    // Create block file reader
    let reader = if let Some(dir) = data_dir {
        BlockFileReader::new(dir, BlockFileNetwork::Mainnet)?
    } else {
        BlockFileReader::auto_detect(BlockFileNetwork::Mainnet)?
    };

    println!("📂 Block file reader created");
    println!("💾 Cache directory: {:?}", cache_dir);
    println!("");
    println!("   Collection will:");
    println!("   - Read blocks sequentially (fast)");
    println!("   - Write to temp file");
    println!("   - Chunk every 125,000 blocks");
    println!("   - Validate blocks during chunking");
    println!("   - Compress and move chunks to secondary drive");
    println!("");

    // Read all blocks sequentially - this triggers collection
    // The iterator will automatically write to temp file and chunk incrementally
    let mut iterator = reader.read_blocks_sequential(None, None)?;

    let mut count = 0;
    let start_time = std::time::Instant::now();
    let mut last_report = std::time::Instant::now();

    while let Some(block_result) = iterator.next() {
        match block_result {
            Ok(_block_data) => {
                count += 1;

                // Progress reporting every 10k blocks
                if count % 10000 == 0 {
                    let elapsed = last_report.elapsed().as_secs_f64();
                    let rate = if elapsed > 0.0 {
                        10000.0 / elapsed
                    } else {
                        0.0
                    };
                    let total_elapsed = start_time.elapsed().as_secs_f64();
                    let avg_rate = if total_elapsed > 0.0 {
                        count as f64 / total_elapsed
                    } else {
                        0.0
                    };

                    println!(
                        "   📊 Collected {} blocks | Rate: {:.0} blocks/sec (avg: {:.0})",
                        count, rate, avg_rate
                    );
                    last_report = std::time::Instant::now();
                }
            }
            Err(e) => {
                eprintln!("   ⚠️  Error reading block {}: {}", count, e);
                return Err(e);
            }
        }
    }

    let total_time = start_time.elapsed();
    let avg_rate = if total_time.as_secs_f64() > 0.0 {
        count as f64 / total_time.as_secs_f64()
    } else {
        0.0
    };

    println!("");
    println!("✅ Collection complete!");
    println!("   Total blocks: {}", count);
    println!(
        "   Total time: {:.1} minutes",
        total_time.as_secs_f64() / 60.0
    );
    println!("   Average rate: {:.0} blocks/sec", avg_rate);

    Ok(())
}

/// Create final chunk from remaining temp file blocks
#[tokio::test]
#[ignore = "local temp chunk + BLOCK_CACHE_DIR: run with --ignored"]
#[cfg(feature = "differential")]
async fn create_final_chunk() -> Result<()> {
    use std::path::{Path, PathBuf};

    let temp_file: PathBuf = std::env::var("BLVM_BENCH_BLOCKS_TEMP")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("blvm-bench")
                .join("blvm-bench-blocks-temp.bin")
        });
    let metadata_file = temp_file.with_extension("bin.meta");

    if !temp_file.exists() {
        println!("❌ Temp file doesn't exist");
        return Ok(());
    }

    if !metadata_file.exists() {
        println!("❌ Metadata file doesn't exist");
        return Ok(());
    }

    // Read block count from metadata
    let bytes = std::fs::read(&metadata_file)?;
    if bytes.len() < 8 {
        println!("❌ Metadata file too small");
        return Ok(());
    }

    let count = u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]);

    println!("📊 Temp file has {} blocks", count);

    if count == 0 {
        println!("⚠️  No blocks in temp file");
        return Ok(());
    }

    // Check if chunk_9 already exists
    let chunk_root = PathBuf::from(std::env::var("BLOCK_CACHE_DIR").expect("BLOCK_CACHE_DIR"));
    let chunk_file = chunk_root.join("chunk_9.bin.zst");
    if chunk_file.exists() {
        println!("⚠️  chunk_9 already exists - skipping");
        return Ok(());
    }

    // Create chunk_9 with remaining blocks
    println!("📦 Creating chunk_9 with {} blocks...", count);
    BlockFileReader::create_and_move_chunk_from_file(&temp_file, 9, count as usize)?;

    println!("✅ Created chunk_9 with {} blocks", count);

    // Remove temp file
    std::fs::remove_file(temp_file)?;
    std::fs::remove_file(&metadata_file)?;
    println!("✅ Cleaned up temp file");

    Ok(())
}
