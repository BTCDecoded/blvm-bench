//! Debug printout of tx_count varint and first-tx prefix at mainnet SegWit activation height.
#![cfg(any(feature = "differential", feature = "scan"))]

use blvm_bench::chunked_cache::ChunkedBlockIterator;
use blvm_consensus::constants::SEGWIT_ACTIVATION_MAINNET;
use blvm_consensus::serialization::decode_varint;
use std::path::Path;

#[test]
#[ignore = "local chunk cache: set BLOCK_CACHE_DIR and run with --ignored"]
fn chunk_cache_wire_trace_at_mainnet_segwit_activation() {
    let root = std::env::var("BLOCK_CACHE_DIR").expect("BLOCK_CACHE_DIR");
    let chunks_dir = std::path::Path::new(&root);

    let mut iter = ChunkedBlockIterator::new(chunks_dir, Some(SEGWIT_ACTIVATION_MAINNET), None)
        .expect("ChunkedBlockIterator::new failed")
        .expect("No iterator returned");

    let data = iter
        .next_block()
        .expect("iterator error")
        .expect("no block");

    println!("Block data size: {} bytes", data.len());
    println!("Header (first 80 bytes):");
    println!(
        "  Version (bytes 0-3): {:02x} {:02x} {:02x} {:02x}",
        data[0], data[1], data[2], data[3]
    );

    let (tx_count, varint_len) = decode_varint(&data[80..]).expect("varint decode failed");
    println!("TX count: {} (varint len: {})", tx_count, varint_len);

    let offset = 80 + varint_len;
    println!("First tx starts at offset: {}", offset);
    println!(
        "First 10 bytes of first tx: {:02x?}",
        &data[offset..offset + 10.min(data.len() - offset)]
    );

    if data.len() >= offset + 6 {
        let tx_version = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        println!("First tx version: {} (0x{:08x})", tx_version, tx_version);
        println!(
            "Bytes after version: {:02x} {:02x}",
            data[offset + 4],
            data[offset + 5]
        );
        if data[offset + 4] == 0x00 && data[offset + 5] == 0x01 {
            println!("  -> SegWit marker detected!");
        }
    }
}
