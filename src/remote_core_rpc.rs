//! Bitcoin Core JSON-RPC over SSH + `nsenter` (remote host where `bitcoind` runs in an isolated netns).
//!
//! Reaches `bitcoind` by SSH and `nsenter` into its network namespace, then calls JSON-RPC via local
//! `curl`. Configure with `REMOTE_CORE_*` env vars. Legacy `LAND_NODE_*` and `START9_*` are still read.

use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio::time::sleep;

fn non_empty_env(primary: &str, alt: &str, legacy: &str) -> Option<String> {
    std::env::var(primary)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var(alt).ok().filter(|s| !s.trim().is_empty()))
        .or_else(|| std::env::var(legacy).ok().filter(|s| !s.trim().is_empty()))
}

fn remote_core_ssh_key() -> anyhow::Result<String> {
    non_empty_env("REMOTE_CORE_SSH_KEY", "LAND_NODE_SSH_KEY", "START9_SSH_KEY").ok_or_else(|| {
        anyhow::anyhow!(
            "Set REMOTE_CORE_SSH_KEY to your SSH private key path (legacy: LAND_NODE_SSH_KEY, START9_SSH_KEY)"
        )
    })
}

fn remote_core_ssh_host() -> anyhow::Result<String> {
    non_empty_env("REMOTE_CORE_SSH_HOST", "LAND_NODE_SSH_HOST", "START9_SSH_HOST").ok_or_else(|| {
        anyhow::anyhow!(
            "Set REMOTE_CORE_SSH_HOST (e.g. bitcoin@192.168.x.x) (legacy: LAND_NODE_SSH_HOST, START9_SSH_HOST)"
        )
    })
}

fn remote_core_rpc_user() -> anyhow::Result<String> {
    non_empty_env("REMOTE_CORE_RPC_USER", "LAND_NODE_RPC_USER", "START9_RPC_USER").ok_or_else(|| {
        anyhow::anyhow!(
            "Set REMOTE_CORE_RPC_USER for bitcoind RPC (legacy: LAND_NODE_RPC_USER, START9_RPC_USER)"
        )
    })
}

fn remote_core_rpc_password() -> anyhow::Result<String> {
    non_empty_env(
        "REMOTE_CORE_RPC_PASSWORD",
        "LAND_NODE_RPC_PASSWORD",
        "START9_RPC_PASSWORD",
    )
    .ok_or_else(|| {
        anyhow::anyhow!(
            "Set REMOTE_CORE_RPC_PASSWORD for bitcoind RPC (legacy: LAND_NODE_RPC_PASSWORD, START9_RPC_PASSWORD)"
        )
    })
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

const MAX_RETRIES: u32 = 2; // Reduced retries for faster failure (dedicated machine)
const RETRY_DELAY_MS: u64 = 50; // Faster retry (dedicated machine)
const PROCESS_ID_CACHE_TTL: Duration = Duration::from_secs(60);

/// RPC client for a remote Bitcoin Core instance (SSH + nsenter).
pub struct RemoteCoreRpcClient {
    /// Cached process ID (refreshed periodically)
    cached_pid: Arc<RwLock<Option<(String, Instant)>>>,
    /// Last successful connection time
    last_success: Arc<RwLock<Option<Instant>>>,
    /// Connection health status
    is_healthy: Arc<RwLock<bool>>,
}

impl RemoteCoreRpcClient {
    pub fn new() -> Self {
        Self {
            cached_pid: Arc::new(RwLock::new(None)),
            last_success: Arc::new(RwLock::new(None)),
            is_healthy: Arc::new(RwLock::new(true)),
        }
    }

    /// Get bitcoind process ID (with caching)
    async fn get_bitcoind_pid(&self) -> Result<String> {
        // Check cache first
        {
            let cache = self.cached_pid.read().await;
            if let Some((pid, cached_at)) = cache.as_ref() {
                if cached_at.elapsed() < PROCESS_ID_CACHE_TTL {
                    return Ok(pid.clone());
                }
            }
        }

        // Cache expired or missing - fetch new PID
        let ssh_key = remote_core_ssh_key()?;
        let ssh_host = remote_core_ssh_host()?;
        let pid_cmd = "pgrep -f 'bitcoind -onion' | head -1";
        let output = Command::new("ssh")
            .arg("-i")
            .arg(&ssh_key)
            .arg("-o")
            .arg("StrictHostKeyChecking=no")
            .arg("-o")
            .arg("ConnectTimeout=5")
            .arg("-o")
            .arg("ControlMaster=auto") // Enable connection sharing for speed (LAN)
            .arg("-o")
            .arg("ControlPath=~/.ssh/control-%r@%h:%p") // Control socket path
            .arg("-o")
            .arg("ControlPersist=300") // Keep connection open for 5 minutes
            .arg(&ssh_host)
            .arg(pid_cmd)
            .output()
            .await
            .context("Failed to execute SSH command to get process ID")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to get bitcoind process ID: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let pid = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if pid.is_empty() {
            anyhow::bail!("bitcoind process not found");
        }

        // Update cache
        {
            let mut cache = self.cached_pid.write().await;
            *cache = Some((pid.clone(), Instant::now()));
        }

        Ok(pid)
    }

    /// Make an RPC call via nsenter with retry logic
    /// Uses synchronous process with stdin to avoid tokio issues
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let body = serde_json::json!({
            "jsonrpc": "1.0",
            "method": method,
            "params": params,
            "id": 1
        });

        let body_str = body.to_string();
        let ssh_key = remote_core_ssh_key()?;
        let ssh_host = remote_core_ssh_host()?;
        let rpc_user = remote_core_rpc_user()?;
        let rpc_password = remote_core_rpc_password()?;
        let mut last_error = None;
        let mut delay = Duration::from_millis(RETRY_DELAY_MS);

        for attempt in 0..=MAX_RETRIES {
            // Get process ID (cached)
            let pid = match self.get_bitcoind_pid().await {
                Ok(p) => p,
                Err(e) => {
                    last_error = Some(e);
                    if attempt < MAX_RETRIES {
                        sleep(delay).await;
                        delay *= 2;
                        continue;
                    }
                    return Err(last_error.unwrap());
                }
            };

            // For large bodies (>50KB), use a temp file to avoid "Argument list too long"
            // For smaller bodies, use echo piped through SSH
            let escaped_body = body_str.replace('\'', "'\\''");
            let body_len = body_str.len();

            // Run synchronously (simpler, avoids tokio task issues)
            use std::process::{Command as SyncCommand, Stdio};

            let output = if body_len > 100_000 {
                // Very large body: write to temp file
                let temp_path = std::env::temp_dir().join("blvm_rpc_body.json");
                let body_to_write = escaped_body.replace("'\\''", "'");
                if let Err(e) = std::fs::write(&temp_path, &body_to_write) {
                    last_error = Some(anyhow::anyhow!("Failed to write temp file: {}", e));
                    if attempt < MAX_RETRIES {
                        sleep(delay).await;
                        delay *= 2;
                        continue;
                    }
                    return Err(last_error.unwrap());
                }

                let full_cmd = format!(
                    "ssh -i {} -o StrictHostKeyChecking=no -o ConnectTimeout=10 -o BatchMode=yes {} \"sudo nsenter -t {} -n curl -s --max-time 60 --user {}:{} --data-binary @- -H 'content-type: text/plain;' http://127.0.0.1:8332/\" < {}",
                    shell_single_quote(&ssh_key),
                    shell_single_quote(&ssh_host),
                    pid,
                    shell_single_quote(&rpc_user),
                    shell_single_quote(&rpc_password),
                    temp_path.display()
                );

                match SyncCommand::new("bash")
                    .arg("-c")
                    .arg(&full_cmd)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                {
                    Ok(o) => o,
                    Err(e) => {
                        last_error = Some(anyhow::anyhow!("Command failed: {}", e));
                        if attempt < MAX_RETRIES {
                            sleep(delay).await;
                            delay *= 2;
                            continue;
                        }
                        return Err(last_error.unwrap());
                    }
                }
            } else {
                // Normal body: use echo
                let full_cmd = format!(
                    "echo '{}' | ssh -i {} -o StrictHostKeyChecking=no -o ConnectTimeout=10 {} \"sudo nsenter -t {} -n curl -s --max-time 30 --user {}:{} --data-binary @- -H 'content-type: text/plain;' http://127.0.0.1:8332/\"",
                    escaped_body,
                    shell_single_quote(&ssh_key),
                    shell_single_quote(&ssh_host),
                    pid,
                    shell_single_quote(&rpc_user),
                    shell_single_quote(&rpc_password)
                );

                match SyncCommand::new("bash")
                    .arg("-c")
                    .arg(&full_cmd)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                {
                    Ok(o) => o,
                    Err(e) => {
                        last_error = Some(anyhow::anyhow!("Command failed: {}", e));
                        if attempt < MAX_RETRIES {
                            sleep(delay).await;
                            delay *= 2;
                            continue;
                        }
                        return Err(last_error.unwrap());
                    }
                }
            };

            if !output.status.success() {
                let error_msg = String::from_utf8_lossy(&output.stderr);
                last_error = Some(anyhow::anyhow!(
                    "RPC call failed (attempt {}/{}): {}",
                    attempt + 1,
                    MAX_RETRIES + 1,
                    error_msg
                ));

                // Retry on transient failures (network errors, timeouts)
                if attempt < MAX_RETRIES && self.is_transient_error(&error_msg) {
                    // Clear PID cache on error (process might have restarted)
                    {
                        let mut cache = self.cached_pid.write().await;
                        *cache = None;
                    }
                    sleep(delay).await;
                    delay *= 2;
                    continue;
                }

                return Err(last_error.unwrap());
            }

            // Parse response
            let response: Value = match serde_json::from_slice(&output.stdout) {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(anyhow::anyhow!("Failed to parse RPC response: {}", e));
                    if attempt < MAX_RETRIES {
                        sleep(delay).await;
                        delay *= 2;
                        continue;
                    }
                    return Err(last_error.unwrap());
                }
            };

            // Check for RPC-level errors
            if let Some(error) = response.get("error") {
                if !error.is_null() {
                    // Don't retry on RPC errors (application-level, not transient)
                    anyhow::bail!("RPC error: {}", error);
                }
            }

            // Success - update health status
            {
                let mut last = self.last_success.write().await;
                *last = Some(Instant::now());
            }
            {
                let mut healthy = self.is_healthy.write().await;
                *healthy = true;
            }

            return Ok(response);
        }

        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("RPC call failed after {} retries", MAX_RETRIES)))
    }

    /// Check if error is transient (should retry)
    fn is_transient_error(&self, error: &str) -> bool {
        let error_lower = error.to_lowercase();
        error_lower.contains("timeout")
            || error_lower.contains("connection")
            || error_lower.contains("network")
            || error_lower.contains("refused")
            || error_lower.contains("unreachable")
            || error_lower.contains("nsenter")
            || error_lower.contains("cannot open")
            || error_lower.contains("/proc/")
            || error_lower.contains("no such file")
    }

    /// Get block count
    pub async fn get_block_count(&self) -> Result<u64> {
        let response = self.call("getblockcount", serde_json::json!([])).await?;
        let count = response
            .get("result")
            .and_then(|v| v.as_u64())
            .context("Invalid getblockcount response")?;
        Ok(count)
    }

    /// Get block hash by height
    pub async fn get_block_hash(&self, height: u64) -> Result<String> {
        let response = self
            .call("getblockhash", serde_json::json!([height]))
            .await?;
        let hash = response
            .get("result")
            .and_then(|v| v.as_str())
            .context("Invalid getblockhash response")?
            .to_string();
        Ok(hash)
    }

    /// Get block by hash (hex format)
    pub async fn get_block_hex(&self, hash: &str) -> Result<String> {
        let response = self.call("getblock", serde_json::json!([hash, 0])).await?;
        let hex = response
            .get("result")
            .and_then(|v| v.as_str())
            .context("Invalid getblock response")?
            .to_string();
        Ok(hex)
    }

    /// Get raw block bytes (hex)
    pub async fn get_block_raw(&self, hash: &str) -> Result<Vec<u8>> {
        let hex = self.get_block_hex(hash).await?;
        hex::decode(&hex).context("Failed to decode block hex")
    }

    /// Get raw transaction by txid
    pub async fn get_raw_transaction(&self, txid: &str, verbose: bool) -> Result<String> {
        let response = self
            .call("getrawtransaction", serde_json::json!([txid, verbose]))
            .await?;
        let hex = response
            .get("result")
            .and_then(|v| v.as_str())
            .context("Invalid getrawtransaction response")?
            .to_string();
        Ok(hex)
    }

    /// Test if transaction would be accepted to mempool
    pub async fn test_mempool_accept(&self, tx_hex: &str) -> Result<serde_json::Value> {
        let response = self
            .call("testmempoolaccept", serde_json::json!([[tx_hex]]))
            .await?;
        let result = response
            .get("result")
            .context("Invalid testmempoolaccept response")?
            .clone();
        Ok(result)
    }

    /// BATCH: Test multiple transactions in a single RPC call
    /// Much more efficient than individual calls
    pub async fn test_mempool_accept_batch(&self, tx_hexes: &[&str]) -> Result<serde_json::Value> {
        if tx_hexes.is_empty() {
            return Ok(serde_json::json!([]));
        }
        let tx_array: Vec<&str> = tx_hexes.to_vec();
        let response = self
            .call("testmempoolaccept", serde_json::json!([tx_array]))
            .await?;
        let result = response
            .get("result")
            .context("Invalid testmempoolaccept batch response")?
            .clone();
        Ok(result)
    }

    /// BATCH: Get multiple block hashes via parallel individual calls
    /// Uses SSH connection multiplexing for efficiency
    pub async fn get_block_hashes_batch(
        &self,
        heights: &[u64],
    ) -> Result<Vec<(u64, Result<String>)>> {
        if heights.is_empty() {
            return Ok(vec![]);
        }

        // Make individual calls in parallel (SSH multiplexing makes this efficient)
        let futures: Vec<_> = heights
            .iter()
            .map(|&h| async move {
                match self.get_block_hash(h).await {
                    Ok(hash) => (h, Ok(hash)),
                    Err(e) => (h, Err(e)),
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;
        Ok(results)
    }

    /// BATCH: Get multiple blocks via parallel individual calls
    /// Uses SSH connection multiplexing for efficiency
    pub async fn get_blocks_batch(&self, hashes: &[&str]) -> Result<Vec<Result<String>>> {
        if hashes.is_empty() {
            return Ok(vec![]);
        }

        // Make individual calls in parallel
        let futures: Vec<_> = hashes
            .iter()
            .map(|&h| {
                let hash = h.to_string();
                async move { self.get_block_hex(&hash).await }
            })
            .collect();

        let results = futures::future::join_all(futures).await;
        Ok(results)
    }

    /// Check connection health
    pub async fn is_healthy(&self) -> bool {
        *self.is_healthy.read().await
    }

    /// Get time since last successful request
    pub async fn time_since_last_success(&self) -> Option<Duration> {
        self.last_success
            .read()
            .await
            .map(|instant| instant.elapsed())
    }

    /// Perform a health check and update status
    pub async fn health_check(&self) -> bool {
        match self.get_block_count().await {
            Ok(_) => {
                {
                    let mut last = self.last_success.write().await;
                    *last = Some(Instant::now());
                }
                {
                    let mut healthy = self.is_healthy.write().await;
                    *healthy = true;
                }
                true
            }
            Err(_) => {
                {
                    let mut healthy = self.is_healthy.write().await;
                    *healthy = false;
                }
                false
            }
        }
    }

    /// Clear cached process ID (useful if process restarts)
    pub async fn clear_pid_cache(&self) {
        let mut cache = self.cached_pid.write().await;
        *cache = None;
    }
}

impl Default for RemoteCoreRpcClient {
    fn default() -> Self {
        Self::new()
    }
}
