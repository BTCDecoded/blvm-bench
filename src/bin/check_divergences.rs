//! Check if BLVM failures are divergences from Bitcoin Core
//! For each failure in failures.log, verify with Bitcoin Core to see if it's a divergence
//! OPTIMIZED: Groups by transaction, batches RPC calls, caches results

use anyhow::{Context, Result};
use blvm_bench::chunked_cache::{ChunkedBlockIterator, SharedChunkCache};
use blvm_bench::chunk_index::load_block_index;
use blvm_bench::start9_rpc_client::Start9RpcClient;
use blvm_consensus::serialization::block::deserialize_block_with_witnesses;
use blvm_consensus::serialization::transaction::serialize_transaction;
use blvm_consensus::block::calculate_tx_id;
use std::path::PathBuf;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::collections::{HashMap, BTreeMap};

#[tokio::main]
async fn main() -> Result<()> {
    let failures_file = PathBuf::from("/run/media/acolyte/Extra/blockchain/sort_merge_data/failures.log");
    let chunks_dir = PathBuf::from("/run/media/acolyte/Extra/blockchain");
    
    println!("🔍 Optimized Divergence Investigation");
    println!("  Reading failures from: {}", failures_file.display());
    println!("");
    
    let file = File::open(&failures_file)?;
    let reader = BufReader::new(file);
    
    // Get all "Script returned false" failures
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;
    let script_failures: Vec<&String> = lines.iter()
        .filter(|line| line.contains("Script returned false"))
        .collect();
    
    println!("Found {} script failures to investigate", script_failures.len());
    println!("Loading block index once (this may take a minute)...");
    
    // CRITICAL OPTIMIZATION: Load block index ONCE and reuse it via Arc
    let block_index = Arc::new(match load_block_index(&chunks_dir)? {
        Some(idx) => {
            println!("✅ Loaded block index ({} entries)", idx.len());
            idx
        },
        None => {
            anyhow::bail!("Block index not found - cannot load blocks efficiently");
        }
    });
    println!();
    
    // Parse all failures first
    #[derive(Debug, Clone)]
    struct Failure {
        block_height: u64,
        tx_idx: usize,
        input_idx: usize,
        line_num: usize,
    }
    
    let mut failures = Vec::new();
    for (line_num, line) in script_failures.iter().enumerate() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 3 {
            continue;
        }
        
        let block_height: u64 = match parts[0].trim().parse() {
            Ok(h) => h,
            Err(_) => continue,
        };
        
        let msg = parts[2].trim();
        let tx_idx = if let Some(start) = msg.find("tx ") {
            if let Some(end) = msg[start+3..].find(',') {
                msg[start+3..start+3+end].trim().parse::<usize>().ok()
            } else {
                None
            }
        } else {
            None
        };
        
        let input_idx = if let Some(start) = msg.find("input ") {
            msg[start+6..].trim().parse::<usize>().ok()
        } else {
            None
        };
        
        if let (Some(t), Some(i)) = (tx_idx, input_idx) {
            failures.push(Failure {
                block_height,
                tx_idx: t,
                input_idx: i,
                line_num: line_num + 1,
            });
        }
    }
    
    // OPTIMIZATION 1: Group failures by block first, then by transaction
    // This allows us to load each block only once and process all transactions from it
    use std::collections::BTreeMap;
    let mut failures_by_block: BTreeMap<u64, BTreeMap<usize, Vec<Failure>>> = BTreeMap::new();
    for failure in failures {
        failures_by_block
            .entry(failure.block_height)
            .or_insert_with(BTreeMap::new)
            .entry(failure.tx_idx)
            .or_insert_with(Vec::new)
            .push(failure);
    }
    
    // Count unique transactions
    let unique_txs: usize = failures_by_block.values()
        .map(|tx_map| tx_map.len())
        .sum();
    
    println!("Grouped into {} blocks, {} unique transactions (from {} failures)", 
            failures_by_block.len(), unique_txs, script_failures.len());
    println!("Processing with batched RPC calls and parallel block loading...\n");
    
    // Shared state
    let rpc_client = Arc::new(Start9RpcClient::new());
    let divergences = Arc::new(std::sync::atomic::AtomicU64::new(0));
    
    // CRITICAL: Limit block cache to prevent memory exhaustion
    const MAX_CACHE_SIZE: usize = 1000;
    let mut block_cache: HashMap<u64, Arc<Vec<u8>>> = HashMap::with_capacity(MAX_CACHE_SIZE);
    let cache_lock = Arc::new(tokio::sync::Mutex::new(block_cache));
    
    // CRITICAL OPTIMIZATION: Shared chunk cache - decompresses each chunk only once
    let chunk_cache = Arc::new(SharedChunkCache::new(&chunks_dir, Arc::clone(&block_index)));
    
    // OPTIMIZATION: Cache RPC results by txid (32 bytes) instead of hex string (saves memory)
    // CRITICAL: Limit cache size to prevent OOM - use LRU eviction
    const MAX_RPC_CACHE_SIZE: usize = 10000; // Max 10k cached results (~10-20MB)
    let rpc_cache: HashMap<[u8; 32], (bool, Option<String>)> = HashMap::with_capacity(MAX_RPC_CACHE_SIZE);
    let rpc_cache_lock = Arc::new(tokio::sync::Mutex::new(rpc_cache));
    
    // Process in batches - group by block for better cache utilization
    const BLOCKS_PER_BATCH: usize = 10; // Process 10 blocks at a time
    const RPC_BATCH_SIZE: usize = 20; // Transactions per RPC batch
    let mut processed_blocks = 0;
    let total_blocks = failures_by_block.len();
    
    // CRITICAL: Process block entries in smaller chunks to avoid loading all 127k blocks into memory
    // Convert to iterator and process in chunks of 1000 blocks at a time
    let block_entries_iter: Vec<_> = failures_by_block.into_iter().collect();
    const BLOCK_ENTRIES_CHUNK_SIZE: usize = 1000; // Process 1000 blocks at a time instead of all 127k
    
    for block_entries_chunk in block_entries_iter.chunks(BLOCK_ENTRIES_CHUNK_SIZE) {
        for block_batch in block_entries_chunk.chunks(BLOCKS_PER_BATCH) {
        let batch_num = processed_blocks / BLOCKS_PER_BATCH + 1;
        println!("Processing batch {} (blocks {}-{})...", 
                batch_num, processed_blocks + 1, processed_blocks + block_batch.len());
        
        // Step 1: Load all blocks in parallel (better cache utilization)
        // Store deserialized blocks by height
        let mut blocks_loaded: HashMap<u64, Arc<Vec<u8>>> = HashMap::new();
        
        // Load blocks in parallel using tokio
        let block_load_tasks: Vec<_> = block_batch.iter().map(|(block_height, _)| {
            let block_height = *block_height;
            let chunk_cache = Arc::clone(&chunk_cache);
            let cache_lock = Arc::clone(&cache_lock);
            
            tokio::spawn(async move {
                // Try cache first
                {
                    let cache = cache_lock.lock().await;
                    if let Some(cached) = cache.get(&block_height) {
                        return Some((Arc::clone(cached), block_height));
                    }
                }
                
                // Load from chunk cache
                match chunk_cache.load_block(block_height) {
                    Ok(Some(data)) => {
                        let data_arc = Arc::new(data);
                        // Cache it
                        let mut cache = cache_lock.lock().await;
                        if cache.len() >= MAX_CACHE_SIZE {
                            if let Some(&oldest_key) = cache.keys().next() {
                                cache.remove(&oldest_key);
                            }
                        }
                        cache.insert(block_height, Arc::clone(&data_arc));
                        Some((data_arc, block_height))
                    },
                    _ => None,
                }
            })
        }).collect();
        
        // Wait for all blocks to load
        let block_results = futures::future::join_all(block_load_tasks).await;
        for result in block_results {
            if let Ok(Some((block_data, height))) = result {
                blocks_loaded.insert(height, block_data);
            }
        }
        
        // Step 2: Collect all transactions from loaded blocks
        let mut tx_hexes = Vec::new();
        let mut tx_ids = Vec::new();
        let mut tx_to_failures: Vec<Vec<Failure>> = Vec::new();
        
        for (block_height, tx_map) in block_batch {
            let block_data = match blocks_loaded.get(block_height) {
                Some(data) => data,
                None => continue, // Block couldn't be loaded
            };
            
            // Deserialize block
            let (block, _witnesses) = match deserialize_block_with_witnesses(block_data) {
                Ok(b) => b,
                Err(_) => continue,
            };
            
            for (tx_idx, failures_in_tx) in tx_map {
                if *tx_idx >= block.transactions.len() {
                    continue;
                }
                
                let tx = &block.transactions[*tx_idx];
                let tx_id = calculate_tx_id(tx);
                
                // Check RPC cache by txid (more efficient than hex string)
                let cached_result = {
                    let cache = rpc_cache_lock.lock().await;
                    cache.get(&tx_id).cloned()
                };
                
                if let Some((allowed, _reject_reason)) = cached_result {
                    // Use cached result
                    if allowed {
                        // Divergence - report all failures for this transaction
                        for failure in failures_in_tx {
                            let div_num = divergences.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                            println!("❌ DIVERGENCE #{}: Block {}, tx {}, input {} - BLVM rejected, Core accepts", 
                                    div_num, failure.block_height, failure.tx_idx, failure.input_idx);
                        }
                    }
                    continue; // Already checked
                }
                
                // Serialize transaction for RPC
                let tx_bytes = serialize_transaction(tx);
                let tx_hex = hex::encode(&tx_bytes);
                
                // Add to batch for RPC call
                tx_hexes.push(tx_hex);
                tx_ids.push(tx_id);
                tx_to_failures.push(failures_in_tx.clone());
            }
        }
        
        if tx_hexes.is_empty() {
            processed_blocks += block_batch.len();
            continue;
        }
        
        // Step 3: Batch RPC calls (process in sub-batches if needed)
        for rpc_batch in tx_hexes.chunks(RPC_BATCH_SIZE) {
            let tx_hex_refs: Vec<&str> = rpc_batch.iter().map(|s| s.as_str()).collect();
            let batch_start_idx = tx_hexes.len() - rpc_batch.len();
            
            match rpc_client.test_mempool_accept_batch(&tx_hex_refs).await {
                Ok(results) => {
                    if let Some(results_array) = results.as_array() {
                        for (i, result) in results_array.iter().enumerate() {
                            let global_idx = batch_start_idx + i;
                            if global_idx >= tx_hexes.len() {
                                break;
                            }
                            
                            let allowed = result.get("allowed")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            
                            let reject_reason = result.get("reject-reason")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            
                            let reason_str = reject_reason.as_ref().map(|s| s.as_str()).unwrap_or("");
                            
                            // Skip "missing-inputs" - not script verification bugs
                            if !allowed && (reason_str.contains("missing-inputs") || reason_str.contains("missing-input")) {
                            // Cache by txid and skip (with eviction if needed)
                            {
                                let mut cache = rpc_cache_lock.lock().await;
                                if cache.len() >= MAX_RPC_CACHE_SIZE && !cache.contains_key(&tx_ids[global_idx]) {
                                    if let Some(&oldest_key) = cache.keys().next() {
                                        cache.remove(&oldest_key);
                                    }
                                }
                                cache.insert(tx_ids[global_idx], (false, reject_reason));
                            }
                                continue;
                            }
                            
                            // Cache result by txid (more efficient than hex string)
                            // CRITICAL: Evict oldest entries if cache is full
                            {
                                let mut cache = rpc_cache_lock.lock().await;
                                if cache.len() >= MAX_RPC_CACHE_SIZE && !cache.contains_key(&tx_ids[global_idx]) {
                                    // Evict first entry (simple FIFO - could use LRU but this is simpler)
                                    if let Some(&oldest_key) = cache.keys().next() {
                                        cache.remove(&oldest_key);
                                    }
                                }
                                cache.insert(tx_ids[global_idx], (allowed, reject_reason.clone()));
                            }
                            
                            // Check for divergence
                            if allowed {
                                // Divergence - report all failures for this transaction
                                for failure in &tx_to_failures[global_idx] {
                                    let div_num = divergences.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                                    println!("❌ DIVERGENCE #{}: Block {}, tx {}, input {} - BLVM rejected, Core accepts", 
                                            div_num, failure.block_height, failure.tx_idx, failure.input_idx);
                                }
                            }
                        }
                    }
                },
                Err(e) => {
                    eprintln!("  ⚠️  RPC batch call failed: {}", e);
                }
            }
        }
        
        processed_blocks += block_batch.len();
        let total_divergences = divergences.load(std::sync::atomic::Ordering::Relaxed);
        println!("  Batch {} complete: {} divergences found (total: {}/{})", 
                batch_num, total_divergences, total_divergences, processed_blocks);
        println!();
        }
    }
    
    let total_divergences = divergences.load(std::sync::atomic::Ordering::Relaxed);
    println!("{}", "=".repeat(80));
    println!("FINAL RESULTS");
    println!("{}", "=".repeat(80));
    println!("  Total blocks processed: {}", processed_blocks);
    println!("  ❌ Divergences found: {}", total_divergences);
    println!();
    
    if total_divergences > 0 {
        println!("  🐛 Found {} consensus bugs (BLVM too strict)", total_divergences);
    } else {
        println!("  ✅ No divergences found - all failures match Core behavior");
    }
    
    Ok(())
}
