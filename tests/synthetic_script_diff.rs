//! Off-chain script differential. Requires `--features differential`.

#![cfg(feature = "differential")]

#[test]
fn synthetic_scripts_match_the_consensus_library() {
    let n = blvm_bench::synthetic_script_diff::assert_synthetic_script_parity()
        .expect("script verdicts");
    assert!(n > 50, "the sweep shrank to {n} cases");
}
