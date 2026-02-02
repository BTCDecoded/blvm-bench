//! Step 6: Parallel script verification
//!
//! Streams blocks and prevouts in lockstep, verifying scripts using all CPU cores.
//! The prevout file is sorted by (block, tx, input), so we read it sequentially.

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use std::collections::HashMap;

use rayon::prelude::*;

use blvm_consensus::serialization::block::{deserialize_block_with_witnesses, deserialize_block_header};
use blvm_consensus::serialization::transaction::serialize_transaction;
use blvm_consensus::transaction::is_coinbase;
use blvm_consensus::types::{Network, TransactionOutput, ByteString, BlockHeader};
use blvm_consensus::script::{verify_script_with_context_full, SigVersion};
use blvm_consensus::segwit::Witness;
use blvm_consensus::bip113::get_median_time_past;
use blvm_consensus::constants::{
    BIP16_P2SH_ACTIVATION_MAINNET,
    BIP66_ACTIVATION_MAINNET,
    BIP65_ACTIVATION_MAINNET,
    BIP147_ACTIVATION_MAINNET,
};

use crate::chunked_cache::ChunkedBlockIterator;
use super::merge_join::JoinedPrevout;
use hex;

/// Calculate script verification flags based on block height
/// Simplified version - uses height-based activation
pub fn get_script_flags(height: u64, _network: Network) -> u32 {
    let mut flags = 0u32;
    
    // P2SH (BIP16) - activated at height 173805 on mainnet
    if height >= BIP16_P2SH_ACTIVATION_MAINNET {
        flags |= 0x01; // SCRIPT_VERIFY_P2SH
    }
    
    // DERSIG (BIP66) - height 363725 on mainnet
    // CRITICAL FIX: BIP66 also enables SCRIPT_VERIFY_STRICTENC (0x02) and SCRIPT_VERIFY_LOW_S (0x08)
    // This was missing and caused signature verification failures!
    if height >= BIP66_ACTIVATION_MAINNET {
        flags |= 0x02 | 0x04 | 0x08; // SCRIPT_VERIFY_STRICTENC | SCRIPT_VERIFY_DERSIG | SCRIPT_VERIFY_LOW_S
    }
    
    // CHECKLOCKTIMEVERIFY (BIP65) - height 388381
    if height >= BIP65_ACTIVATION_MAINNET {
        flags |= 0x200; // SCRIPT_VERIFY_CHECKLOCKTIMEVERIFY
    }
    
    // CHECKSEQUENCEVERIFY (BIP112) and NULLDUMMY (BIP147) - activated with SegWit at height 481824
    // CRITICAL FIX: These should be enabled together at BIP147 activation, not separately
    if height >= BIP147_ACTIVATION_MAINNET {
        flags |= 0x10 | 0x400; // SCRIPT_VERIFY_NULLDUMMY | SCRIPT_VERIFY_CHECKSEQUENCEVERIFY
    }
    
    // WITNESS (BIP141) - height 481824
    // Note: SCRIPT_VERIFY_WITNESS is set per-transaction based on witness presence,
    // but we enable the base flag here. The actual flag is set in calculate_script_flags_for_block
    // based on whether the transaction has witness data.
    // For step6, we don't have per-transaction witness info here, so we enable it if past activation.
    // However, this is handled in verify_script_with_context_full which uses calculate_script_flags_for_block.
    // So we don't set it here to avoid double-setting.
    
    // Taproot (BIP341) - height 709632
    // Note: Taproot flag (0x8000) is set per-transaction in calculate_script_flags_for_block
    // based on whether outputs are P2TR. We don't set it here globally.
    // The flag is SCRIPT_VERIFY_WITNESS_PUBKEYTYPE (0x8000), not 0x20000
    
    flags
}

/// Prevout reader that streams sorted prevout data
pub struct PrevoutReader {
    reader: BufReader<File>,
    buffer: Vec<u8>,
    leftover: Vec<u8>,
}

impl PrevoutReader {
    pub fn new(path: &Path) -> Result<Self> {
        let file = File::open(path)
            .with_context(|| format!("Failed to open prevout file: {}", path.display()))?;
        Ok(Self {
            reader: BufReader::with_capacity(64 * 1024 * 1024, file),
            buffer: vec![0u8; 256 * 1024],
            leftover: Vec::new(),
        })
    }
    
    /// Skip forward to prevouts for a specific block height
    /// This is needed when resuming from a specific block
    pub fn skip_to_block(&mut self, target_height: u32) -> Result<()> {
        use std::io::Seek;
        
        println!("  Skipping prevouts to block {}...", target_height);
        let mut skipped_records = 0u64;
        let mut last_reported_block = 0u32;
        let start_time = std::time::Instant::now();
        
        // Read records until we find one >= target_height
        loop {
            // Try to parse from leftover first
            if self.leftover.len() >= 27 {
                if let Some((prevout, consumed)) = JoinedPrevout::from_bytes(&self.leftover) {
                    if prevout.spending_block >= target_height {
                        // Found it - put it back in leftover for next read_block_prevouts
                        let elapsed = start_time.elapsed();
                        println!("  ✅ Skipped to block {} ({} records in {:.1}s, {:.0} rec/s)", 
                            target_height, skipped_records, elapsed.as_secs_f64(), 
                            skipped_records as f64 / elapsed.as_secs_f64().max(0.001));
                        return Ok(());
                    }
                    // This prevout is for an earlier block - skip it
                    self.leftover.drain(..consumed);
                    skipped_records += 1;
                    
                    // Progress reporting every 10k records or every 5k blocks
                    if skipped_records % 10_000 == 0 || (prevout.spending_block > last_reported_block + 5_000) {
                        let elapsed = start_time.elapsed();
                        let rate = skipped_records as f64 / elapsed.as_secs_f64();
                        println!("  ⏩ Skipped {} records (at block {}, {:.0} rec/s, {:.1}s elapsed)", 
                            skipped_records, prevout.spending_block, rate, elapsed.as_secs_f64());
                        last_reported_block = prevout.spending_block;
                    }
                    continue;
                }
            }
            
            // Read more data
            let n = self.reader.read(&mut self.buffer)?;
            if n == 0 {
                // EOF - no more prevouts, we've passed the target
                let elapsed = start_time.elapsed();
                eprintln!("  ⚠️  Warning: Reached EOF in prevout file before target block {} (skipped {} records in {:.1}s)", 
                    target_height, skipped_records, elapsed.as_secs_f64());
                return Ok(());
            }
            self.leftover.extend_from_slice(&self.buffer[..n]);
        }
    }
    
    /// Read prevouts for a specific block
    /// Returns prevouts sorted by (tx_idx, input_idx)
    pub fn read_block_prevouts(&mut self, block_height: u32) -> Result<Vec<JoinedPrevout>> {
        let mut prevouts = Vec::new();
        
        loop {
            // Try to parse from leftover
            while self.leftover.len() >= 27 {
                if let Some((prevout, consumed)) = JoinedPrevout::from_bytes(&self.leftover) {
                    if prevout.spending_block < block_height {
                        // This shouldn't happen if files are correct
                        eprintln!("Warning: Prevout for past block {} (expecting {})", 
                            prevout.spending_block, block_height);
                        self.leftover.drain(..consumed);
                        continue;
                    }
                    
                    if prevout.spending_block > block_height {
                        // This prevout is for a future block - don't consume it
                        return Ok(prevouts);
                    }
                    
                    // This prevout is for our block
                    prevouts.push(prevout);
                    self.leftover.drain(..consumed);
                } else {
                    break; // Not enough data for a complete record
                }
            }
            
            // Read more data
            let n = self.reader.read(&mut self.buffer)?;
            if n == 0 {
                return Ok(prevouts); // EOF
            }
            self.leftover.extend_from_slice(&self.buffer[..n]);
        }
    }
}

// Note: We don't use a struct here - we build tasks on-the-fly to reduce memory
// The key optimization is using Arc<> to share all_prevouts across inputs in the same transaction

/// Verify all scripts in the blockchain using streamed prevout data
pub fn verify_scripts(
    chunks_dir: &Path,
    prevouts_file: &Path,
    start_height: u64,
    end_height: u64,
    progress_interval: u64,
    network: Network,
) -> Result<(u64, u64, Vec<(u64, String)>)> {
    println!("\n{}", "═".repeat(60));
    println!("STEP 6: Parallel Script Verification");
    println!("{}", "═".repeat(60));
    println!("  Chunks dir: {}", chunks_dir.display());
    println!("  Blocks: {} to {}", start_height, end_height);
    println!("  Prevouts: {}", prevouts_file.display());
    println!("  Using {} threads", rayon::current_num_threads());
    
    let start_time = Instant::now();
    
    // Create block iterator
    let mut block_iter = ChunkedBlockIterator::new(chunks_dir, Some(start_height), None)?
        .ok_or_else(|| anyhow::anyhow!("Failed to create block iterator - chunks.meta not found?"))?;
    
    // Create prevout reader
    let mut prevout_reader = PrevoutReader::new(prevouts_file)?;
    
    // Skip to the start block if resuming
    if start_height > 0 {
        println!("  Skipping prevouts to block {}...", start_height);
        prevout_reader.skip_to_block(start_height as u32)?;
        println!("  ✅ Skipped to block {}", start_height);
    }
    
    let total_verified = Arc::new(AtomicU64::new(0));
    let total_failed = Arc::new(AtomicU64::new(0));
    let mut divergences: Vec<(u64, String)> = Vec::new();
    
    // Failure statistics by type
    let mut failure_stats: HashMap<String, u64> = HashMap::new();
    failure_stats.insert("Missing prevout".to_string(), 0);
    failure_stats.insert("Script returned false".to_string(), 0);
    failure_stats.insert("Script error".to_string(), 0);
    
    // Write failures to a file for analysis (truncate on restart to avoid mixing old/new data)
    let failures_file = prevouts_file.parent()
        .unwrap_or(Path::new("."))
        .join("failures.log");
    let mut failures_writer = BufWriter::new(
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)  // CRITICAL FIX: Clear log on restart, don't append
            .open(&failures_file)?
    );
    writeln!(failures_writer, "# Block Height | Error Type | Details | TX Hex")?;
    
    let mut height = start_height;
    let mut last_report = Instant::now();
    let mut sample_counter = 0u64;
    
    // CRITICAL FIX: Keep buffer of last 11 block headers for median_time_past calculation (BIP113)
    // This is required for timestamp-based CLTV validation (BIP65)
    let mut recent_headers: Vec<BlockHeader> = Vec::with_capacity(11);
    
    // OPTIMIZATION: Reusable buffers to avoid allocations per block
    // NOTE: prevout_map cannot be reused because it contains references to block_prevouts (block-scoped)
    use blvm_consensus::types::{OutPoint, TransactionOutput};
    use std::sync::Arc;
    let mut intra_block_utxos: std::collections::HashMap<OutPoint, TransactionOutput> = 
        std::collections::HashMap::with_capacity(1000);
    
    while height < end_height {
        // Get next block
        if height == start_height || height % 1000 == 0 || (height >= start_height && height < start_height + 10) {
            println!("  🔄 Loading block {} (height < end_height: {}, end_height: {})", height, height < end_height, end_height);
        }
        let block_data = match block_iter.next_block() {
            Ok(Some(data)) => {
                if height <= start_height + 10 || height % 1000 == 0 {
                    println!("  ✅ Got block {} from iterator ({} bytes)", height, data.len());
                }
                data
            },
            Ok(None) => {
                println!("  ✅ Reached end of blocks at height {} (current_height < end_height: {})", height, height < end_height);
                break;
            }
            Err(e) => {
                eprintln!("  ❌ FATAL: Error loading block {}: {:?}", height, e);
                eprintln!("  ❌ This will cause the process to exit. Check block iterator.");
                return Err(e).context("Failed to load block from iterator");
            }
        };
        
        // OPTIMIZATION: Calculate median_time_past BEFORE deserializing full block
        // Extract header from block_data directly (avoids double deserialization)
        // CRITICAL FIX: Extract block header for median_time_past calculation (BIP113)
        // This is required for timestamp-based CLTV validation (BIP65)
        if block_data.len() < 80 {
            anyhow::bail!("Block {} too short: {} bytes", height, block_data.len());
        }
        let block_header = deserialize_block_header(&block_data[..80])
            .with_context(|| format!("Failed to deserialize block header {}", height))?;
        
        // CRITICAL FIX: BIP113 requires median of last 11 blocks BEFORE current block
        // For block N, we need blocks N-11 through N-1 (NOT including N)
        // So we calculate median from previous headers, then add current header for next iteration
        let median_time_past = if recent_headers.len() >= 11 {
            // We have 11 previous headers - calculate median from them (not including current block)
            Some(get_median_time_past(&recent_headers))
        } else if recent_headers.len() >= 1 {
            // Fewer than 11 headers available - use what we have
            Some(get_median_time_past(&recent_headers))
        } else {
            // No previous headers - can't calculate median
            None
        };
        
        // Update recent headers buffer AFTER calculating median (for next block)
        // OPTIMIZATION: Avoid clone by moving header (we don't need it after this)
        recent_headers.push(block_header);
        if recent_headers.len() > 11 {
            recent_headers.remove(0);
        }
        
        // Deserialize block
        // CRITICAL FIX: witnesses is now Vec<Vec<Witness>> (one Vec per transaction, each containing one Witness per input)
        // OPTIMIZATION: deserialize_block_with_witnesses will deserialize header again, but that's unavoidable
        // since it needs the full block structure. The header extraction above is just for median_time_past.
        let (block, witnesses) = deserialize_block_with_witnesses(&block_data)
            .with_context(|| format!("Failed to deserialize block {}", height))?;
        
        // Get prevouts for this block
        let block_prevouts = match prevout_reader.read_block_prevouts(height as u32) {
            Ok(prevouts) => prevouts,
            Err(e) => {
                eprintln!("  ❌ FATAL: Error reading prevouts for block {}: {:?}", height, e);
                return Err(e).context(format!("Failed to read prevouts for block {}", height));
            }
        };
        
        // OPTIMIZATION: Clear and reuse buffers instead of allocating new ones
        intra_block_utxos.clear();
        
        // NOTE: prevout_map, tx_prevouts, and verification_tasks cannot be reused because they contain
        // references to block-scoped data (block_prevouts, witnesses, block.transactions)
        let mut prevout_map: std::collections::HashMap<(u32, u32), &JoinedPrevout> = 
            std::collections::HashMap::with_capacity(block_prevouts.len());
        let mut tx_prevouts: Vec<(Arc<Vec<TransactionOutput>>, Option<&Vec<Witness>>, bool)> = 
            Vec::with_capacity(block.transactions.len().max(100));
        // OPTIMIZATION: Estimate verification tasks capacity upfront to avoid reallocations
        let estimated_inputs = block.transactions.iter()
            .map(|tx| tx.inputs.len())
            .sum::<usize>();
        let mut verification_tasks: Vec<_> = Vec::with_capacity(estimated_inputs);
        
        // OPTIMIZATION: Reserve capacity for intra_block_utxos if needed (HashMap already has correct capacity)
        if block.transactions.len() * 2 > intra_block_utxos.capacity() {
            intra_block_utxos.reserve(block.transactions.len() * 2 - intra_block_utxos.capacity());
        }
        
        // Index prevouts by (tx_idx, input_idx) for fast lookup
        for prevout in &block_prevouts {
            prevout_map.insert(
                (prevout.spending_tx_idx, prevout.spending_input_idx),
                prevout
            );
        }
        
        // OPTIMIZATION: Only build intra-block UTXO map lazily when we encounter a missing prevout
        // This avoids expensive calculate_tx_id calls for every transaction in every block
        // Most blocks don't have intra-block spending, so this is a significant win
        let mut intra_block_utxos_built = false;
        
        // OPTIMIZATION: Calculate base script flags once per block (same for all transactions)
        // Transaction-specific flags (witness, Taproot) are added per-transaction below
        let base_flags = get_script_flags(height, network);
        
        // OPTIMIZATION: Pre-calculate height-based flags once per block (same for all transactions)
        let height_has_segwit = height >= 481824;
        let height_has_taproot = height >= 709632;
        
        // OPTIMIZATION: Cache coinbase status to avoid repeated is_coinbase() calls
        // Build tx_prevouts with coinbase flags
        let mut tx_is_coinbase: Vec<bool> = Vec::with_capacity(block.transactions.len());
        
        // Calculate script flags per-transaction (important for Taproot which checks transaction outputs)
        // Replicate logic from calculate_script_flags_for_block since it's private
        
        // PARALLEL VERIFICATION: Build all verification tasks with shared prevouts
        // KEY OPTIMIZATION: Use Arc<> to share all_prevouts across inputs in the same transaction
        // This avoids cloning the entire prevout vector for every single input
        use std::sync::Arc;
        
        // Build shared prevouts per transaction (Arc to share across inputs)
        // CRITICAL FIX: witnesses is now Vec<Vec<Witness>>, so we store Vec<Witness> per transaction
        // Also store whether transaction has missing prevouts (affects verification)
        // OPTIMIZATION: tx_prevouts already has capacity reserved, no need to reserve again
        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            let is_cb = is_coinbase(tx);
            tx_is_coinbase.push(is_cb);
            
            if is_cb {
                tx_prevouts.push((Arc::new(Vec::new()), None, false));
                continue;
            }
            
            // Build all prevouts for this transaction (needed for sighash - same for all inputs)
            // CRITICAL: If ANY prevout is missing, we can't verify ANY inputs in this transaction
            // because sighash calculation requires ALL prevouts to be correct
            // OPTIMIZATION: Pre-allocate with exact capacity to avoid reallocations
            let mut all_prevouts: Vec<TransactionOutput> = Vec::with_capacity(tx.inputs.len());
            let mut has_missing_prevout = false;
            
            for (i, input) in tx.inputs.iter().enumerate() {
                // First try the merge-join prevout map
                if let Some(prevout) = prevout_map.get(&(tx_idx as u32, i as u32)) {
                    // NOTE: Clone is necessary because sighash needs owned TransactionOutput values
                    // and we can't move from prevout_map (it contains references)
                    all_prevouts.push(TransactionOutput {
                        value: prevout.value,
                        script_pubkey: prevout.script_pubkey.clone(),
                    });
                } else {
                    // Missing from merge-join - try intra-block lookup
                    // OPTIMIZATION: Build intra-block UTXOs lazily only when needed
                    if !intra_block_utxos_built {
                        intra_block_utxos.clear();
                        use blvm_consensus::block::calculate_tx_id;
                        for (tx_idx, tx) in block.transactions.iter().enumerate() {
                            let tx_id = calculate_tx_id(tx);
                            for (output_idx, output) in tx.outputs.iter().enumerate() {
                                let outpoint = OutPoint {
                                    hash: tx_id,
                                    index: output_idx as u64,
                                };
                                intra_block_utxos.insert(outpoint, TransactionOutput {
                                    value: output.value,
                                    script_pubkey: output.script_pubkey.clone(),
                                });
                            }
                        }
                        intra_block_utxos_built = true;
                    }
                    if let Some(output) = intra_block_utxos.get(&input.prevout) {
                        // Found in same block! Use it
                        all_prevouts.push(output.clone());
                    } else {
                        // Still missing - mark this transaction as unverifiable
                        has_missing_prevout = true;
                        // Still add empty entry to maintain length, but we'll skip verification
                        all_prevouts.push(TransactionOutput {
                            value: 0,
                            script_pubkey: vec![],
                        });
                    }
                }
            }
            
            // Get witness stacks for this transaction (one Witness per input)
            let tx_witnesses = witnesses.get(tx_idx);
            // Store the missing flag with the prevouts
            tx_prevouts.push((Arc::new(all_prevouts), tx_witnesses, has_missing_prevout));
        }
        
        // OPTIMIZATION: verification_tasks already has capacity reserved, no need to reserve again
        // Build verification tasks (now with shared prevouts via Arc)
        // OPTIMIZATION: Use cached coinbase status instead of calling is_coinbase() again
        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            if tx_is_coinbase[tx_idx] {
                continue;
            }
            
            let (all_prevouts_arc, tx_witnesses, has_missing) = &tx_prevouts[tx_idx];
            
            // CRITICAL: If transaction has ANY missing prevout, skip ALL verifications
            // Sighash calculation requires ALL prevouts to be correct
            if *has_missing {
                // Count missing prevouts directly without creating verification tasks
                // This avoids inflating the failure count with tasks that can't be verified
                for (input_idx, input) in tx.inputs.iter().enumerate() {
                    let prevout_opt = prevout_map.get(&(tx_idx as u32, input_idx as u32))
                        .map(|p| (p.script_pubkey.clone(), p.value))
                        .or_else(|| {
                            // OPTIMIZATION: Build intra-block UTXOs lazily only when needed
                            if !intra_block_utxos_built {
                                intra_block_utxos.clear();
                                use blvm_consensus::block::calculate_tx_id;
                                for (tx_idx, tx) in block.transactions.iter().enumerate() {
                                    let tx_id = calculate_tx_id(tx);
                                    for (output_idx, output) in tx.outputs.iter().enumerate() {
                                        let outpoint = OutPoint {
                                            hash: tx_id,
                                            index: output_idx as u64,
                                        };
                                        intra_block_utxos.insert(outpoint, TransactionOutput {
                                            value: output.value,
                                            script_pubkey: output.script_pubkey.clone(),
                                        });
                                    }
                                }
                                intra_block_utxos_built = true;
                            }
                            intra_block_utxos.get(&input.prevout)
                                .map(|output| (output.script_pubkey.clone(), output.value))
                        });
                    
                    if prevout_opt.is_none() {
                        // Count missing prevout directly (don't create verification task)
                        let prevout_txid_hex = hex::encode(&input.prevout.hash);
                        let error_msg = format!("Missing prevout: tx {}, input {} (looking for txid: {}, idx: {})", 
                            tx_idx, input_idx, prevout_txid_hex, input.prevout.index);
                        
                        // Count as failure directly
                        total_failed.fetch_add(1, Ordering::Relaxed);
                        *failure_stats.entry("Missing prevout".to_string()).or_insert(0) += 1;
                        
                        // Log sample
                        sample_counter += 1;
                        let should_log = sample_counter % 1000 == 0 
                            || sample_counter <= 1000
                            || height % 10000 == 0
                            || height % 5000 == 0 && height > 400000;
                        
                        if should_log {
                            writeln!(failures_writer, "{} | {} | {}", height, "Missing prevout", error_msg)?;
                            failures_writer.flush()?;
                        }
                    }
                }
                continue;
            }
            
            // All prevouts available - create verification tasks
            // CRITICAL: prevout_opt MUST match what's in all_prevouts (at same index)
            // Since has_missing is false, all_prevouts[i] should match the prevout for input i
            
            // Calculate flags for this transaction (checks for Taproot outputs)
            // OPTIMIZATION: Start with base flags calculated once per block
            let mut tx_flags_base = base_flags;
            
            // Add witness flag if transaction has witness data (per-transaction check)
            // OPTIMIZATION: Use pre-calculated height_has_segwit instead of checking height every time
            if tx_witnesses.is_some() && height_has_segwit {
                tx_flags_base |= 0x800; // SCRIPT_VERIFY_WITNESS
            }
            
            // Add Taproot flag (0x8000 = SCRIPT_VERIFY_WITNESS_PUBKEYTYPE) if transaction has Taproot outputs
            // This must be checked per-transaction, not per-block
            // OPTIMIZATION: Use pre-calculated height_has_taproot instead of checking height every time
            if height_has_taproot {
                use blvm_consensus::constants::TAPROOT_SCRIPT_LENGTH;
                for output in &tx.outputs {
                    let script = &output.script_pubkey;
                    // P2TR format: OP_1 (0x51) + push 32 (0x20) + 32-byte program = 34 bytes
                    if script.len() == TAPROOT_SCRIPT_LENGTH && script[0] == 0x51 && script[1] == 0x20 {
                        tx_flags_base |= 0x8000; // SCRIPT_VERIFY_WITNESS_PUBKEYTYPE (Taproot)
                        break;
                    }
                }
            }
            
            // OPTIMIZATION: verification_tasks already has capacity reserved, no need to reserve per transaction
            for (input_idx, _input) in tx.inputs.iter().enumerate() {
                // OPTIMIZATION: Don't clone prevout_script here - we'll access it directly from all_prevouts_arc
                // This avoids unnecessary allocation in the hot path
                // NOTE: Arc::clone is cheap (just increments reference count), so this is fine
                verification_tasks.push((tx_idx, input_idx, tx, tx_witnesses, Arc::clone(all_prevouts_arc), tx_flags_base));
            }
        }
        
        // OPTIMIZATION: Verify inputs in parallel using rayon
        // Increased chunk size for better CPU utilization and reduced overhead
        // Larger chunks = less rayon overhead, better cache locality, more work per thread
        let chunk_size = 2000; // Process 2000 inputs at a time (was 1000)
        let mut results: Vec<(bool, Option<(usize, usize)>, u8)> = Vec::new();
        
        // OPTIMIZATION: Pre-allocate results vector with estimated capacity
        // This avoids reallocations during parallel processing
        results.reserve(verification_tasks.len());
        
        for chunk in verification_tasks.chunks(chunk_size) {
            let chunk_results: Vec<_> = chunk.par_iter()
                .map(|(tx_idx, input_idx, tx, tx_witnesses, all_prevouts_arc, tx_flags_base)| {
                    // OPTIMIZATION: Access prevout directly from Arc without cloning
                    let prevout = &all_prevouts_arc[*input_idx];
                    let prevout_script = &prevout.script_pubkey;
                    let _prevout_value = prevout.value;
                    
                    // CRITICAL FIX: Extract witness data for this specific input
                    // tx_witnesses is Option<&Vec<Witness>> where each Witness is for one input
                    // Now we pass the full witness stack (Witness) to support proper P2WSH-in-P2SH execution
                    let witness_stack: Option<&Witness> = tx_witnesses
                        .and_then(|witnesses| witnesses.get(*input_idx));
                    
                    // Use flags calculated per-transaction (includes Taproot check)
                    // calculate_script_flags_for_block already handles witness flag
                    let tx_flags = *tx_flags_base;
                    
                    let input = &tx.inputs[*input_idx];
                    match verify_script_with_context_full(
                        &input.script_sig,
                        prevout_script, // Use reference directly, no clone
                        witness_stack,
                        tx_flags, // Use per-transaction flags with witness flag
                        tx,
                        *input_idx,
                        all_prevouts_arc.as_slice(), // Use Arc'd prevouts
                        Some(height),
                        median_time_past, // OPTIMIZATION: Calculated once per block (same for all txs in block)
                        network,
                        SigVersion::Base,
                    ) {
                        Ok(true) => (true, None, 0u8), // Use u8 tag instead of string
                        Ok(false) => {
                            // OPTIMIZATION: Store indices only, format message later (avoids string alloc in hot path)
                            (false, Some((*tx_idx, *input_idx)), 1u8) // 1 = Script returned false
                        },
                        Err(_e) => {
                            // OPTIMIZATION: Store indices only, format message later
                            (false, Some((*tx_idx, *input_idx)), 2u8) // 2 = Script error
                        },
                    }
                })
                .collect();
            results.extend(chunk_results);
            // chunk_results is moved into results.extend() and automatically dropped
        }
        
        // Process results
        // NOTE: tx_prevouts must live until after verification_tasks is processed
        // because verification_tasks contains references to tx_witnesses from tx_prevouts
        for (success, indices_opt, failure_type_tag) in results {
            if success {
                total_verified.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            
            // OPTIMIZATION: Use string literals directly instead of allocating new Strings
            let failure_type = match failure_type_tag {
                1u8 => "Script returned false",
                2u8 => "Script error",
                _ => continue, // Unknown/success
            };
            
            total_failed.fetch_add(1, Ordering::Relaxed);
            // OPTIMIZATION: Use entry API with string literal to avoid allocation
            *failure_stats.entry(failure_type.to_string()).or_insert(0) += 1;
            
            if let Some((tx_idx, input_idx)) = indices_opt {
                // OPTIMIZATION: Format message only when logging (not in hot path)
                let msg = format!("{}:{}", tx_idx, input_idx);
                
                // OPTIMIZATION: Format full error message only if we'll log it
                let is_error = failure_type == "Script error";
                
                // Sample failures: write every Nth failure to disk for analysis
                // But ALWAYS log errors since they're rare
                sample_counter += 1;
                let should_log = is_error  // Always log errors
                    || sample_counter % 1000 == 0 
                    || sample_counter <= 1000
                    || height % 10000 == 0
                    || height % 5000 == 0 && height > 400000;
                
                if should_log {
                    // OPTIMIZATION: Format full message only when logging
                    let full_msg = format!("Script {}: tx {}, input {}", 
                        if failure_type == "Script error" { "error" } else { "returned false" },
                        tx_idx, input_idx);
                    
                    // Include tx hex for divergence checking (avoids re-reading blocks later)
                    let tx_hex = hex::encode(serialize_transaction(&block.transactions[tx_idx]));
                    
                    writeln!(failures_writer, "{} | {} | {} | {}", height, failure_type, full_msg, tx_hex)?;
                    failures_writer.flush()?;
                }
                
                // Keep first 100 in memory for final report
                if divergences.len() < 100 {
                    divergences.push((height, msg));
                }
            }
        }
        
        
        // Progress report - ALWAYS report after processing, or every progress_interval blocks, or every 10 seconds
        let processed = height - start_height + 1;
        if processed % progress_interval == 0 || last_report.elapsed().as_secs() >= 10 || (height >= start_height && height < start_height + 100) {
            let elapsed = start_time.elapsed().as_secs_f64();
            let rate = processed as f64 / elapsed;
            let remaining = (end_height - height) as f64 / rate;
            let v = total_verified.load(Ordering::Relaxed);
            let f = total_failed.load(Ordering::Relaxed);
            
            // Show failure breakdown
            let missing = failure_stats.get("Missing prevout").unwrap_or(&0);
            let script_false = failure_stats.get("Script returned false").unwrap_or(&0);
            let script_err = failure_stats.get("Script error").unwrap_or(&0);
            
            println!(
                "  Block {}/{} ({:.1}%) - ✓{} ✗{} (M:{} F:{} E:{}) - {:.0} blk/s - ETA: {:.0}m",
                height, end_height,
                (height - start_height) as f64 / (end_height - start_height) as f64 * 100.0,
                v, f,
                missing, script_false, script_err,
                rate,
                remaining / 60.0
            );
            last_report = Instant::now();
        }
        
        height += 1;
    }
    
    println!("  📍 Loop exited: height={}, end_height={}, height < end_height: {}", 
             height, end_height, height < end_height);
    
    let elapsed = start_time.elapsed();
    let verified_final = total_verified.load(Ordering::Relaxed);
    let failed_final = total_failed.load(Ordering::Relaxed);
    let blocks_processed = height - start_height;
    
    failures_writer.flush()?;
    
    println!("{}", "─".repeat(60));
    println!("  ✅ Step 6 Complete!");
    println!("  Verified: {}", verified_final);
    println!("  Failed: {}", failed_final);
    println!("  Failure breakdown:");
    for (failure_type, count) in &failure_stats {
        println!("    {}: {} ({:.2}%)", 
            failure_type, 
            count, 
            *count as f64 / failed_final as f64 * 100.0
        );
    }
    println!("  Divergences sampled: {} (see {})", divergences.len(), failures_file.display());
    println!("  Blocks processed: {}", blocks_processed);
    println!("  Time: {:.1}m", elapsed.as_secs_f64() / 60.0);
    println!("  Rate: {:.0} blocks/sec", blocks_processed as f64 / elapsed.as_secs_f64());
    
    Ok((verified_final, failed_final, divergences))
}
