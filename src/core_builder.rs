//! Compatibility re-exports for code and tests that still use the `core_builder` name.
//!
//! Prefer `crate::node_builder` and [`NodeBuilder`] / [`NodeBinaries`] in new code.

pub use crate::node_builder::*;

/// Historical name for [`NodeBuilder`].
pub type CoreBuilder = NodeBuilder;

/// Historical name for [`NodeBinaries`].
pub type CoreBinaries = NodeBinaries;
