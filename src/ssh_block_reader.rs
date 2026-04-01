//! SSH-based block reader for a remote Bitcoin Core host
//! Reads blocks via SSH, bypassing mount permission issues

use anyhow::{Context, Result};
use std::process::Command;
use std::path::PathBuf;

pub struct SshBlockReader {
    ssh_key: PathBuf,
    ssh_host: String,
    blocks_dir: String,
}

impl SshBlockReader {
    pub fn new(ssh_key: impl AsRef<std::path::Path>, ssh_host: &str) -> Self {
        Self {
            ssh_key: ssh_key.as_ref().to_path_buf(),
            ssh_host: ssh_host.to_string(),
            blocks_dir: "/embassy-data/package-data/volumes/bitcoind/data/main/blocks".to_string(),
        }
    }

    /// Read a block by height from the remote host
    /// Uses SSH to read the block file and parse it
    pub fn read_block_by_height(&self, height: u64) -> Result<Vec<u8>> {
        // For now, read from blk00000.dat (first file contains early blocks)
        // In the future, we could use Core's block index to find the right file
        let block_file = format!("{}/blk00000.dat", self.blocks_dir);
        
        // Use SSH to read the file and parse blocks sequentially until we reach the desired height
        let mut current_height = 0u64;
        let mut offset = 0usize;
        
        // Read file in chunks via SSH
        let magic: [u8; 4] = [0xf9, 0xbe, 0xb4, 0xd9]; // Mainnet magic
        
        loop {
            // Read magic + size + block data
            // We need to read sequentially, so we'll use a Python script or similar
            // For now, let's use a simpler approach: read the entire first 1MB and parse it
            
            if current_height > height + 10 {
                // Safety limit - don't read too far
                anyhow::bail!("Block {} not found in first file", height);
            }
            
            // Use SSH to read a chunk and parse blocks
            let cmd = format!(
                "ssh -i {} {} 'sudo dd if={} bs=1 skip={} count=1000000 2>/dev/null'",
                self.ssh_key.display(),
                self.ssh_host,
                block_file,
                offset
            );
            
            let output = Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .output()
                .context("Failed to execute SSH command")?;
            
            if !output.status.success() {
                anyhow::bail!("SSH command failed");
            }
            
            let data = output.stdout;
            if data.is_empty() {
                anyhow::bail!("No data read from block file");
            }
            
            // Parse blocks from the data
            let mut pos = 0;
            while pos + 8 < data.len() {
                // Check magic
                if data[pos..pos+4] != magic {
                    break;
                }
                
                // Read size
                let size = u32::from_le_bytes([
                    data[pos+4], data[pos+5], data[pos+6], data[pos+7]
                ]) as usize;
                
                if pos + 8 + size > data.len() {
                    break; // Need more data
                }
                
                // Extract block
                let block_data = data[pos+8..pos+8+size].to_vec();
                
                if current_height == height {
                    return Ok(block_data);
                }
                
                current_height += 1;
                pos += 8 + size;
                offset += 8 + size;
            }
            
            // If we didn't find it, we need to read more
            if current_height < height {
                offset += 1000000; // Move forward
                continue;
            }
            
            anyhow::bail!("Block {} not found", height);
        }
    }
}

