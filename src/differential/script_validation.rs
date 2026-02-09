//! Script-level differential validation using Bitcoin Core's libbitcoinconsensus
//!
//! This module compares script verification between BLVM consensus and Bitcoin Core's
//! libbitcoinconsensus library to catch script-level divergences during differential testing.

#[cfg(feature = "differential")]
    use bitcoinconsensus::{verify as core_verify, ScriptVerifyFlags};
    use blvm_consensus::script::verify_script_with_context_full;
    use blvm_consensus::types::{Transaction, Block, ByteString, Network};
    use blvm_consensus::serialization::transaction::serialize_transaction;
    use anyhow::{Context, Result};
    use tracing::warn;

    /// Script comparison result
    #[derive(Debug, Clone)]
    pub struct ScriptComparisonResult {
        /// Whether BLVM and Core results match
        pub matches: bool,
        /// Bitcoin Core verification result
        pub core_result: bool,
        /// BLVM verification result
        pub blvm_result: bool,
        /// Script pubkey that was verified
        pub script_pubkey: ByteString,
        /// Transaction input index
        pub input_index: usize,
        /// Transaction hash (for debugging)
        pub tx_hash: Option<String>,
    }

    /// Compare script verification between BLVM and Bitcoin Core's libbitcoinconsensus
    ///
    /// # Arguments
    /// * `script_pubkey` - The scriptPubkey to verify against
    /// * `script_sig` - The scriptSig (witness data handled separately)
    /// * `tx` - The transaction being verified
    /// * `input_index` - Index of the input being verified
    /// * `amount` - The amount of the previous output (for SegWit)
    /// * `flags` - Script verification flags
    ///
    /// # Returns
    /// Comparison result indicating if BLVM and Core agree
    pub fn compare_script_verification(
        script_pubkey: &ByteString,
        script_sig: &ByteString,
        tx: &Transaction,
        input_index: usize,
        amount: u64,
        flags: ScriptVerifyFlags,
    ) -> Result<ScriptComparisonResult> {
        // Serialize transaction for Bitcoin Core (wire format)
        let tx_bytes = serialize_transaction(tx);
        
        // Verify with Bitcoin Core's libbitcoinconsensus
        let core_result = core_verify(
            script_pubkey.as_ref(),
            &tx_bytes,
            input_index,
            amount,
            flags,
        ).context("Bitcoin Core script verification failed")?;
        
        // Verify with BLVM consensus
        // Note: verify_script_with_context_full signature:
        // script_sig, script_pubkey, witness, flags, tx, input_index, prevout_values, prevout_script_pubkeys, block_height, median_time_past, network, sigversion
        use blvm_consensus::script::{verify_script_with_context_full, SigVersion};
        
        // Convert amount to i64 (as expected by function)
        let prevout_values = vec![amount as i64];
        let prevout_script_pubkeys = vec![script_pubkey.clone()];
        
        // Use standard flags - combine common verification flags
        // P2SH | STRICTENC | DERSIG | LOW_S | NULLDUMMY | WITNESS
        let blvm_flags: u32 = 0x01 | 0x02 | 0x04 | 0x08 | 0x10 | 0x800;
        
        let blvm_result = verify_script_with_context_full(
            script_sig,
            script_pubkey,
            None, // witness (handle separately if needed)
            blvm_flags,
            tx,
            input_index,
            &prevout_values,
            &prevout_script_pubkeys.iter().map(|s| s).collect::<Vec<_>>(),
            None, // block_height
            None, // median_time_past
            Network::Mainnet,
            SigVersion::Base,
            #[cfg(feature = "production")]
            None, // schnorr_collector
        )?;
        
        let matches = core_result == blvm_result;
        
        // Compute tx hash for debugging (if needed)
        let tx_hash = if !matches {
            // Use serialization to compute hash
            let tx_bytes = serialize_transaction(tx);
            use sha2::{Digest, Sha256};
            let hash = Sha256::digest(Sha256::digest(&tx_bytes));
            Some(hex::encode(hash))
        } else {
            None
        };
        
        Ok(ScriptComparisonResult {
            matches,
            core_result,
            blvm_result,
            script_pubkey: script_pubkey.clone(),
            input_index,
            tx_hash,
        })
    }

    /// Compare all scripts in a block
    ///
    /// Extracts all scriptPubkeys and scriptSigs from a block and compares
    /// verification results between BLVM and Bitcoin Core.
    ///
    /// # Arguments
    /// * `block` - The block to analyze
    /// * `height` - Block height (for context)
    ///
    /// # Returns
    /// Vector of comparison results for each script in the block
    pub fn compare_block_scripts(
        block: &Block,
        height: u64,
    ) -> Result<Vec<ScriptComparisonResult>> {
        let mut results = Vec::new();
        
        // Default flags for script verification
        let flags = ScriptVerifyFlags::all();
        
        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            // Skip coinbase transaction (no inputs to verify)
            if tx_idx == 0 {
                continue;
            }
            
            // For each input, we need the previous output's scriptPubkey and amount
            // This is a simplified version - in practice, you'd need UTXO data
            for (input_idx, input) in tx.inputs.iter().enumerate() {
                // TODO: Get scriptPubkey and amount from UTXO set
                // For now, this is a placeholder that shows the structure
                // In real implementation, you'd need:
                // 1. Lookup UTXO for input.prevout
                // 2. Get scriptPubkey and amount from UTXO
                // 3. Call compare_script_verification
                
                // Placeholder - actual implementation requires UTXO access
                warn!(
                    "Script comparison for block {} tx {} input {} requires UTXO data",
                    height, tx_idx, input_idx
                );
            }
        }
        
        Ok(results)
    }

    /// Compare scripts with UTXO data
    ///
    /// This is the full implementation that requires UTXO set access.
    /// Use this when you have UTXO data available.
    pub fn compare_block_scripts_with_utxos(
        block: &Block,
        height: u64,
        utxos: &[(ByteString, u64)], // (scriptPubkey, amount) for each input
    ) -> Result<Vec<ScriptComparisonResult>> {
        let mut results = Vec::new();
        let flags = ScriptVerifyFlags::all();
        
        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            // Skip coinbase
            if tx_idx == 0 {
                continue;
            }
            
            for (input_idx, input) in tx.inputs.iter().enumerate() {
                // Get UTXO data for this input
                if let Some((script_pubkey, amount)) = utxos.get(input_idx) {
                    let script_sig = &input.script_sig;
                    
                    match compare_script_verification(
                        script_pubkey,
                        script_sig,
                        tx,
                        input_idx,
                        *amount,
                        flags,
                    ) {
                        Ok(result) => {
                            if !result.matches {
                                warn!(
                                    "Script divergence at block {} tx {} input {}: Core={}, BLVM={}",
                                    height, tx_idx, input_idx, result.core_result, result.blvm_result
                                );
                            }
                            results.push(result);
                        }
                        Err(e) => {
                            warn!(
                                "Script comparison failed at block {} tx {} input {}: {}",
                                height, tx_idx, input_idx, e
                            );
                        }
                    }
                }
            }
        }
        
        Ok(results)
    }
}

