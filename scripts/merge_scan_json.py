#!/usr/bin/env python3
"""Merge chain scan result JSON files into one."""
import json
import sys
from pathlib import Path

def merge_maps(acc, other):
    for k, v in other.items():
        acc[k] = acc.get(k, 0) + v

def merge_era(acc, other):
    if not other:
        return
    acc["blocks"] = acc.get("blocks", 0) + other.get("blocks", 0)
    acc["total_txs"] = acc.get("total_txs", 0) + other.get("total_txs", 0)
    acc["blocked_txs"] = acc.get("blocked_txs", 0) + other.get("blocked_txs", 0)
    acc["total_weight"] = acc.get("total_weight", 0) + other.get("total_weight", 0)
    acc["blocked_weight"] = acc.get("blocked_weight", 0) + other.get("blocked_weight", 0)


def merge_collateral_era(acc, other):
    if not other:
        return
    acc["collateral_txs"] = acc.get("collateral_txs", 0) + other.get("collateral_txs", 0)
    acc["collateral_weight"] = acc.get("collateral_weight", 0) + other.get("collateral_weight", 0)


def merge_retarget(acc, other):
    """Merge blocked_weight_by_retarget: sum by start_height."""
    if not other:
        return
    for k, v in other.items():
        if k not in acc:
            acc[k] = {"start_height": int(k), "blocked_weight": 0, "total_weight": 0, "blocked_weight_pct": 0.0}
        acc[k]["blocked_weight"] = acc[k].get("blocked_weight", 0) + v.get("blocked_weight", 0)
        acc[k]["total_weight"] = acc[k].get("total_weight", 0) + v.get("total_weight", 0)
        tw = acc[k]["total_weight"]
        acc[k]["blocked_weight_pct"] = (acc[k]["blocked_weight"] * 100.0 / tw) if tw else 0.0

def main():
    if len(sys.argv) < 4:
        print("Usage: merge_scan_json.py file1.json file2.json file3.json -o merged.json")
        sys.exit(1)

    args = sys.argv[1:]
    if "-o" in args:
        out_idx = args.index("-o")
        output = Path(args[out_idx + 1])
        inputs = [Path(a) for a in args[:out_idx] if a != "-o"]
    else:
        output = Path("scan_merged.json")
        inputs = [Path(a) for a in args]

    acc = {
        "blocks_scanned": 0,
        "total_txs": 0,
        "blocked_txs": 0,
        "blocked_weight": 0,
        "total_weight": 0,
        "blocks_post_inscriptions": 0,
        "total_txs_post_inscriptions": 0,
        "blocked_txs_post_inscriptions": 0,
        "blocked_weight_post_inscriptions": 0,
        "total_weight_post_inscriptions": 0,
        "era_pre_segwit": {},
        "era_segwit": {},
        "era_taproot": {},
        "era_inscriptions": {},
        "violations_by_type": {},
        "classifications": {},
        "witness_element_histogram": {},
        "blocked_txs_with_output_violation": 0,
        "blocked_txs_with_witness_violation": 0,
        "blocked_txs_with_control_violation": 0,
        "blocked_weight_with_output_violation": 0,
        "blocked_weight_with_witness_violation": 0,
        "blocked_weight_with_control_violation": 0,
        "collateral_violations_by_type": {},
        "collateral_witness_element_histogram": {},
        "collateral_by_era": {
            "pre_segwit": {"collateral_txs": 0, "collateral_weight": 0},
            "segwit": {"collateral_txs": 0, "collateral_weight": 0},
            "taproot": {"collateral_txs": 0, "collateral_weight": 0},
            "inscriptions": {"collateral_txs": 0, "collateral_weight": 0},
        },
        "blocked_weight_by_retarget": {},
        "largwitness_spam_by_era": {
            "pre_segwit": 0,
            "segwit": 0,
            "taproot": 0,
            "inscriptions": 0,
        },
        "largwitness_and_witness_blocked": 0,
        "blocked_txs_with_taproot_output": 0,
        "block_txs_with_tapscript_op_if_violation": 0,
        "tapscript_op_if_grandfathered": 0,
        "tapscript_op_if_unspendable": 0,
        "collateral_by_classification": {},
        "spam_txs": 0,
        "spam_by_type": {},
        "spam_and_rule_blocked": 0,
        "spam_and_not_rule_blocked": 0,
        "rule_blocked_and_not_spam": 0,
        "spam_by_confidence": {},
        "spam_by_type_by_era": {
            "pre_segwit": {},
            "segwit": {},
            "taproot": {},
            "inscriptions": {},
        },
    }

    for i, p in enumerate(inputs):
        with open(p) as f:
            other = json.load(f)
        print(f"  {i+1}: {other['blocks_scanned']} blocks, {other['total_txs']} txs, {other['blocked_txs']} blocked")
        acc["blocks_scanned"] += other.get("blocks_scanned", 0)
        acc["total_txs"] += other.get("total_txs", 0)
        acc["blocked_txs"] += other.get("blocked_txs", 0)
        acc["blocked_weight"] += other.get("blocked_weight", 0)
        acc["total_weight"] += other.get("total_weight", 0)
        acc["blocks_post_inscriptions"] += other.get("blocks_post_inscriptions", 0)
        acc["total_txs_post_inscriptions"] += other.get("total_txs_post_inscriptions", 0)
        acc["blocked_txs_post_inscriptions"] += other.get("blocked_txs_post_inscriptions", 0)
        acc["blocked_weight_post_inscriptions"] += other.get("blocked_weight_post_inscriptions", 0)
        acc["total_weight_post_inscriptions"] += other.get("total_weight_post_inscriptions", 0)
        merge_era(acc["era_pre_segwit"], other.get("era_pre_segwit", {}))
        merge_era(acc["era_segwit"], other.get("era_segwit", {}))
        merge_era(acc["era_taproot"], other.get("era_taproot", {}))
        merge_era(acc["era_inscriptions"], other.get("era_inscriptions", {}))
        acc["blocked_txs_with_output_violation"] += other.get("blocked_txs_with_output_violation", 0)
        acc["blocked_txs_with_witness_violation"] += other.get("blocked_txs_with_witness_violation", 0)
        acc["blocked_txs_with_control_violation"] += other.get("blocked_txs_with_control_violation", 0)
        acc["blocked_weight_with_output_violation"] += other.get("blocked_weight_with_output_violation", 0)
        acc["blocked_weight_with_witness_violation"] += other.get("blocked_weight_with_witness_violation", 0)
        acc["blocked_weight_with_control_violation"] += other.get("blocked_weight_with_control_violation", 0)
        acc["spam_txs"] += other.get("spam_txs", 0)
        acc["spam_and_rule_blocked"] += other.get("spam_and_rule_blocked", 0)
        acc["spam_and_not_rule_blocked"] += other.get("spam_and_not_rule_blocked", 0)
        acc["rule_blocked_and_not_spam"] += other.get("rule_blocked_and_not_spam", 0)
        merge_maps(acc["violations_by_type"], other.get("violations_by_type", {}))
        merge_maps(acc["classifications"], other.get("classifications", {}))
        merge_maps(acc["witness_element_histogram"], other.get("witness_element_histogram", {}))
        merge_maps(acc["collateral_violations_by_type"], other.get("collateral_violations_by_type", {}))
        merge_maps(acc["collateral_witness_element_histogram"], other.get("collateral_witness_element_histogram", {}))
        merge_collateral_era(acc["collateral_by_era"]["pre_segwit"], other.get("collateral_by_era", {}).get("pre_segwit", {}))
        merge_collateral_era(acc["collateral_by_era"]["segwit"], other.get("collateral_by_era", {}).get("segwit", {}))
        merge_collateral_era(acc["collateral_by_era"]["taproot"], other.get("collateral_by_era", {}).get("taproot", {}))
        merge_collateral_era(acc["collateral_by_era"]["inscriptions"], other.get("collateral_by_era", {}).get("inscriptions", {}))
        merge_retarget(acc["blocked_weight_by_retarget"], other.get("blocked_weight_by_retarget", {}))
        acc["largwitness_spam_by_era"]["pre_segwit"] += other.get("largwitness_spam_by_era", {}).get("pre_segwit", 0)
        acc["largwitness_spam_by_era"]["segwit"] += other.get("largwitness_spam_by_era", {}).get("segwit", 0)
        acc["largwitness_spam_by_era"]["taproot"] += other.get("largwitness_spam_by_era", {}).get("taproot", 0)
        acc["largwitness_spam_by_era"]["inscriptions"] += other.get("largwitness_spam_by_era", {}).get("inscriptions", 0)
        acc["largwitness_and_witness_blocked"] += other.get("largwitness_and_witness_blocked", 0)
        acc["blocked_txs_with_taproot_output"] += other.get("blocked_txs_with_taproot_output", 0)
        acc["block_txs_with_tapscript_op_if_violation"] += other.get("block_txs_with_tapscript_op_if_violation", 0)
        acc["tapscript_op_if_grandfathered"] += other.get("tapscript_op_if_grandfathered", 0)
        acc["tapscript_op_if_unspendable"] += other.get("tapscript_op_if_unspendable", 0)
        merge_maps(acc["collateral_by_classification"], other.get("collateral_by_classification", {}))
        merge_maps(acc["spam_by_type"], other.get("spam_by_type", {}))
        for era in ("pre_segwit", "segwit", "taproot", "inscriptions"):
            merge_maps(
                acc["spam_by_type_by_era"][era],
                other.get("spam_by_type_by_era", {}).get(era, {}),
            )
        merge_maps(acc["spam_by_confidence"], other.get("spam_by_confidence", {}))

    with open(output, "w") as f:
        json.dump(acc, f, indent=2)

    # Validation checks (when batch files include new fields from scan)
    collateral_sum = (
        acc["collateral_by_era"]["pre_segwit"]["collateral_txs"]
        + acc["collateral_by_era"]["segwit"]["collateral_txs"]
        + acc["collateral_by_era"]["taproot"]["collateral_txs"]
        + acc["collateral_by_era"]["inscriptions"]["collateral_txs"]
    )
    has_new_fields = collateral_sum > 0 or len(acc.get("blocked_weight_by_retarget", {})) > 0

    if has_new_fields:
        errors = []
        if collateral_sum != acc.get("rule_blocked_and_not_spam", 0):
            errors.append(
                f"collateral_by_era sum ({collateral_sum}) != rule_blocked_and_not_spam ({acc.get('rule_blocked_and_not_spam', 0)})"
            )

        retarget_blocked_sum = sum(
            v.get("blocked_weight", 0) for v in acc.get("blocked_weight_by_retarget", {}).values()
        )
        if retarget_blocked_sum != acc.get("blocked_weight", 0):
            errors.append(
                f"blocked_weight_by_retarget sum ({retarget_blocked_sum}) != blocked_weight ({acc.get('blocked_weight', 0)})"
            )

        largwitness_sum = (
            acc["largwitness_spam_by_era"]["pre_segwit"]
            + acc["largwitness_spam_by_era"]["segwit"]
            + acc["largwitness_spam_by_era"]["taproot"]
            + acc["largwitness_spam_by_era"]["inscriptions"]
        )
        largwitness_total = acc.get("spam_by_type", {}).get("LargeWitness", 0)
        if largwitness_sum != largwitness_total:
            errors.append(
                f"largwitness_spam_by_era sum ({largwitness_sum}) != spam_by_type.LargeWitness ({largwitness_total})"
            )

        collateral_class_sum = sum(acc.get("collateral_by_classification", {}).values())
        # Only validate when batch files have collateral_by_classification (from rescan)
        if collateral_class_sum > 0 and collateral_class_sum != acc.get("rule_blocked_and_not_spam", 0):
            errors.append(
                f"collateral_by_classification sum ({collateral_class_sum}) != rule_blocked_and_not_spam ({acc.get('rule_blocked_and_not_spam', 0)})"
            )

        if errors:
            print("\n❌ Validation FAILED:")
            for e in errors:
                print(f"   {e}")
            sys.exit(1)
        print("\n✅ Validation passed (collateral_by_era, blocked_weight_by_retarget, largwitness_spam_by_era)")
    else:
        print("\n⚠️  Validation skipped: batch files lack new fields. Re-run scan for full validation.")

    print(f"\nMerged: {acc['blocks_scanned']} blocks, {acc['total_txs']} txs, {acc['blocked_txs']} blocked")
    if acc.get("total_weight", 0) > 0:
        pct_tx = 100 * acc["blocked_txs"] / acc["total_txs"] if acc["total_txs"] else 0
        pct_wt = 100 * acc["blocked_weight"] / acc["total_weight"]
        print(f"  Full chain: {pct_tx:.2}% txs, {pct_wt:.2}% weight blocked")
    if acc.get("total_txs_post_inscriptions", 0) > 0:
        pct_tx = 100 * acc["blocked_txs_post_inscriptions"] / acc["total_txs_post_inscriptions"]
        tw = acc.get("total_weight_post_inscriptions", 0)
        pct_wt = 100 * acc["blocked_weight_post_inscriptions"] / tw if tw else 0
        print(f"  Post-inscriptions: {pct_tx:.2}% txs, {pct_wt:.2}% weight blocked")
    print(f"Written to {output}")

if __name__ == "__main__":
    main()
