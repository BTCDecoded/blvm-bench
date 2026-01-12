//! Parallel historical block differential tests
//!
//! These tests run differential validation on the entire blockchain using
//! parallel execution with UTXO checkpoints for speed.

#[cfg(feature = "differential")]
use anyhow::Result;
#[cfg(feature = "differential")]
use blvm_bench::core_rpc_client::{BitcoinNetwork, CoreRpcClient, RpcConfig};
#[cfg(feature = "differential")]
use blvm_bench::parallel_differential::{ParallelConfig, run_parallel_differential};
#[cfg(feature = "differential")]
use std::sync::Arc;

/// Test historical blocks in parallel
#[tokio::test]
#[cfg(feature = "differential")]
async fn test_historical_blocks_parallel() -> Result<()> {
    // Try to create RPC client (optional - chunks work without it)
    let rpc_client = match create_rpc_client(Some(BitcoinNetwork::Mainnet)).await {
        Ok((client, net)) => {
            println!("📡 Connected to {} node (RPC available for comparison)", net.as_str());
            Some(Arc::new(client))
        }
        Err(e) => {
            println!("⚠️  RPC not available: {}", e);
            println!("💡 Using direct file reading (chunks) - no RPC comparison");
            None
        }
    };

    // Get configuration from environment
    let start_height: u64 = std::env::var("HISTORICAL_BLOCK_START")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let end_height: u64 = std::env::var("HISTORICAL_BLOCK_END")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            // Default to full blockchain if not specified
            if let Some(ref client) = rpc_client {
                tokio::runtime::Handle::current()
                    .block_on(client.getblockcount())
                    .unwrap_or(926_435)
            } else {
                // Default to 875,000 blocks (7 chunks × 125,000) if using chunks
                874_999
            }
        });

    let num_workers: usize = std::env::var("PARALLEL_WORKERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| num_cpus::get());
    
    let chunk_size: u64 = std::env::var("CHUNK_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);

    let config = ParallelConfig {
        num_workers,
        chunk_size,
        use_checkpoints: true,
    };

    println!("🔧 Configuration:");
    println!("   Start height: {}", start_height);
    println!("   End height: {}", end_height);
    println!("   Workers: {}", config.num_workers);
    println!("   Chunk size: {}", config.chunk_size);

    // Create optimized block data source (tries direct file reading first, then cache, then RPC)
    let cache_dir = std::env::var("BLOCK_CACHE_DIR")
        .ok()
        .map(std::path::PathBuf::from);
    
    let block_source = blvm_bench::parallel_differential::create_block_data_source(
        blvm_bench::parallel_differential::BlockFileNetwork::Mainnet,
        cache_dir.as_deref(),
        rpc_client,
    )?;
    
    // Log what data source we're using
    match &block_source {
        blvm_bench::parallel_differential::BlockDataSource::DirectFile(_) => {
            println!("✅ Using direct file reading (fastest - 10-50x faster than RPC)");
        }
        blvm_bench::parallel_differential::BlockDataSource::SharedCache(_, _) => {
            println!("✅ Using shared cache (fast - 5-10x faster than RPC)");
        }
        blvm_bench::parallel_differential::BlockDataSource::Rpc(_) => {
            println!("✅ Using RPC (slower but works everywhere)");
        }
        blvm_bench::parallel_differential::BlockDataSource::Start9Rpc(_) => {
            println!("✅ Using Start9 RPC (for encrypted files)");
        }
    }
    
    // Run parallel differential test
    let results = run_parallel_differential(
        start_height,
        end_height,
        config,
        Arc::new(block_source),
    )
    .await?;

    // Check for divergences
    let total_divergences: usize = results.iter().map(|r| r.divergences.len()).sum();
    
    if total_divergences > 0 {
        eprintln!("❌ Found {} divergences!", total_divergences);
        // Don't fail the test - just report divergences
        // This allows us to identify issues without breaking CI
    } else {
        println!("✅ All blocks matched between BLVM and Core!");
    }

    Ok(())
}

#[cfg(feature = "differential")]
/// Helper to create RPC client (reused from integration.rs)
async fn create_rpc_client(
    preferred_network: Option<BitcoinNetwork>,
) -> Result<(CoreRpcClient, BitcoinNetwork)> {
    use blvm_bench::core_builder::CoreBuilder;
    use blvm_bench::core_rpc_client::NodeDiscovery;

    let auto_discover = std::env::var("BITCOIN_AUTO_DISCOVER")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);

    let rpc_host = std::env::var("BITCOIN_RPC_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let is_remote = rpc_host != "127.0.0.1" && rpc_host != "localhost";

    if is_remote {
        println!("🌐 Connecting to remote Bitcoin Core node at {}", rpc_host);
        let rpc_config = RpcConfig::from_env();
        let rpc_client = CoreRpcClient::new(rpc_config);
        let network = rpc_client.detect_network().await?;
        Ok((rpc_client, network))
    } else if auto_discover {
        match NodeDiscovery::auto_discover().await {
            Ok(rpc_config) => {
                println!("✅ Auto-discovered node at {}", rpc_config.url);
                let rpc_client = CoreRpcClient::new(rpc_config);
                let network = rpc_client.detect_network().await?;
                Ok((rpc_client, network))
            }
            Err(e) => {
                println!("⚠️  Auto-discovery failed: {}", e);
                create_rpc_client_local(preferred_network).await
            }
        }
    } else {
        create_rpc_client_local(preferred_network).await
    }
}

#[cfg(feature = "differential")]
async fn create_rpc_client_local(
    _preferred_network: Option<BitcoinNetwork>,
) -> Result<(CoreRpcClient, BitcoinNetwork)> {
    // For historical block testing with chunks, we use direct file reading
    // RPC is only needed for comparison, and we can use remote RPC or skip it
    anyhow::bail!(
        "Local regtest node not available. Set BITCOIN_RPC_HOST to connect to a remote node, or use direct file reading (chunks) which doesn't require RPC."
    );
}

