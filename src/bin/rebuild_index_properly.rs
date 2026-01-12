//! Rebuild the index properly by checking ALL blocks, not just gaps
//! 
//! This fixes the issue where blocks are incorrectly marked as missing
//! when they're actually in chunks.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    // Set up panic handler to log panics
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("   ❌ PANIC: {:?}", panic_info);
        eprintln!("   💡 This should not happen - please report this error");
    }));
    
    let chunks_dir = std::env::var("BLOCK_CACHE_DIR")
        .unwrap_or_else(|_| "/run/media/acolyte/Extra/blockchain".to_string());
    let chunks_dir = PathBuf::from(chunks_dir);
    
    println!("🔨 Rebuilding index properly...");
    println!("   Chunks directory: {}", chunks_dir.display());
    
    // Backup existing index
    let index_path = chunks_dir.join("chunks.index");
    if index_path.exists() {
        let backup_path = chunks_dir.join("chunks.index.backup");
        println!("   💾 Backing up existing index...");
        std::fs::copy(&index_path, &backup_path)?;
        println!("   ✅ Backed up to {}", backup_path.display());
        
        // Check if we should force a full rebuild or resume
        let should_force_rebuild = std::env::var("FORCE_REBUILD").is_ok();
        if should_force_rebuild {
            println!("   🗑️  FORCE_REBUILD set - deleting existing index to force full rebuild...");
            std::fs::remove_file(&index_path)?;
            println!("\n🚀 Building index from scratch...");
        } else {
            println!("   ✅ Preserving existing index - will resume and fill gaps only");
            println!("   💡 Set FORCE_REBUILD=1 to force a full rebuild from scratch");
            println!("\n🚀 Building/resuming index...");
        }
    } else {
        println!("\n🚀 Building index from scratch (no existing index found)...");
    }
    use blvm_bench::chunk_index_rpc::build_block_index_via_rpc;
    
    let index = match build_block_index_via_rpc(&chunks_dir, Some(249999)).await {
        Ok(idx) => {
            println!("\n✅ Index rebuilt successfully!");
            idx
        }
        Err(e) => {
            eprintln!("\n❌ Failed to rebuild index: {}", e);
            eprintln!("   💡 Error details: {:?}", e);
            return Err(e);
        }
    };
    
    println!("   Total blocks indexed: {}", index.len());
    
    // Count how many are actually missing
    let missing_count = index.values().filter(|e| e.chunk_number == 999).count();
    println!("   Blocks in chunks: {}", index.len() - missing_count);
    println!("   Blocks truly missing: {}", missing_count);
    
    // Final save
    use blvm_bench::chunk_index::save_block_index;
    save_block_index(&chunks_dir, &index)?;
    println!("   💾 Final index saved");
    
    Ok(())
}




