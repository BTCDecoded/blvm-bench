//! Compatibility re-exports for code and tests that still use the `core_rpc_client` name.
//!
//! Prefer `crate::node_rpc_client` and [`NodeRpcClient`] in new code.

pub use crate::node_rpc_client::*;

/// Historical name for [`NodeRpcClient`].
pub type CoreRpcClient = NodeRpcClient;
