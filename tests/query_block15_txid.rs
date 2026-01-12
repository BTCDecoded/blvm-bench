//! Quick test to query Bitcoin Core for block 15's coinbase txid

use anyhow::Result;
use blvm_bench::start9_rpc_client::Start9RpcClient;
use serde_json::json;

#[tokio::test]
async fn test_query_block15_coinbase() -> Result<()> {
    let client = Start9RpcClient::new();
    
    // Get block 1 first to verify its prev_hash
    let block1_hash_result = client.call("getblockhash", json!([1])).await?;
    let block1_hash = block1_hash_result["result"].as_str().unwrap();
    println!("Block 1 hash: {}", block1_hash);
    
    let block1_result = client.call("getblock", json!([block1_hash, 1])).await?;
    let block1 = block1_result["result"].as_object();
    if let Some(b1) = block1 {
        if let Some(prev_hash) = b1.get("previousblockhash") {
            println!("Block 1 prev_hash (from Core): {}", prev_hash.as_str().unwrap());
        }
    }
    
    // Get block 15
    let block_hash_result = client.call("getblockhash", json!([15])).await?;
    let block_hash = block_hash_result["result"].as_str().unwrap();
    println!("Block 15 hash: {}", block_hash);
    
    // Get block with transactions
    let block_result = client.call("getblock", json!([block_hash, 2])).await?;
    let block = block_result["result"].as_object();
    
    if block.is_none() {
        println!("❌ Failed to get block: {}", serde_json::to_string_pretty(&block_result)?);
        return Ok(());
    }
    let block = block.unwrap();
    
    // Get coinbase transaction
    let txids = block.get("tx").and_then(|v| v.as_array());
    if txids.is_none() {
        println!("❌ No transactions in block");
        return Ok(());
    }
    let txids = txids.unwrap();
    let coinbase_txid = txids[0].as_str().unwrap();
    println!("Coinbase txid (from Core): {}", coinbase_txid);
    
    // Also get the full transaction to see its structure
    let tx_result = client.call("getrawtransaction", json!([coinbase_txid, true])).await?;
    let tx = &tx_result["result"];
    println!("Coinbase transaction: {}", serde_json::to_string_pretty(tx)?);
    
    // Check what TX 1 (second transaction) is trying to spend
    if txids.len() > 1 {
        let tx1_id = txids[1].as_str().unwrap();
        println!("\nTX 1 txid: {}", tx1_id);
        
        let tx1_result = client.call("getrawtransaction", json!([tx1_id, true])).await?;
        let tx1 = &tx1_result["result"];
        println!("TX 1 transaction: {}", serde_json::to_string_pretty(tx1)?);
        
        if let Some(inputs) = tx1["vin"].as_array() {
            if !inputs.is_empty() {
                let prevout = &inputs[0]["txid"];
                let vout = &inputs[0]["vout"];
                println!("\nTX 1 input 0:");
                println!("  prevout txid: {}", prevout);
                println!("  prevout vout: {}", vout);
            }
        }
    }
    
    Ok(())
}
