#!/usr/bin/env python3
"""Compare the isolated old implementation with the direct webpage pipeline."""
import argparse
import hashlib
import json
from pathlib import Path


def change(old, new):
    return (new / old - 1) * 100


def compare(baseline, after):
    old_rows = {row["fixture"]: row for row in baseline["samples"]}
    verdicts = []
    for new in after["samples"]:
        old = old_rows[new["fixture"]]
        assert old["fixture_sha256"] == new["fixture_sha256"]
        assert len(old["per_iteration_samples"]) == len(new["per_iteration_samples"]) == 20
        equivalent = old["body_semantics"] == new["body_semantics"]
        differences = [key for key in old["body_semantics"] if old["body_semantics"][key] != new["body_semantics"].get(key)]
        capability = new["fixture_kind"] == "capability"
        phase_changes = {
            f"{phase}_{stat}_change_pct": change(old[phase][f"{stat}_ms"], new[phase][f"{stat}_ms"])
            for phase, stat in (("extract_convert", "median"), ("first_viewport_layout", "p95"), ("full_layout", "p95"))
        }
        phase_changes["peak_rss_change_pct"] = change(old["peak_rss_kb"], new["peak_rss_kb"])
        allocations = new["allocations_per_iteration"]["median"] <= old["allocations_per_iteration"]["median"]
        allocated_bytes = new["allocated_bytes_per_iteration"]["median"] <= old["allocated_bytes_per_iteration"]["median"]
        passed = equivalent and allocations and allocated_bytes and all(value <= (5 if key in ("extract_convert_median_change_pct", "peak_rss_change_pct") else 10) for key, value in phase_changes.items())
        verdicts.append({"fixture": new["fixture"], "semantic_equivalent": equivalent, "semantic_differences": differences, "capability": capability, "allocations_not_increased": allocations, "allocated_bytes_not_increased": allocated_bytes, **phase_changes, "status": "capability_difference" if capability else "pass" if passed else "fail"})
    return verdicts


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("after", type=Path)
    args = parser.parse_args()
    baseline = json.loads(args.baseline.read_text())
    after = json.loads(args.after.read_text())
    verdicts = compare(baseline, after)
    after["whole_refactor_threshold_verdicts"] = verdicts
    if "threshold_verdicts" in after:
        after["replay_threshold_verdicts"] = after.pop("threshold_verdicts")
    after["metadata"]["baseline_kind"] = "isolated_pre_refactor_revision"
    after["metadata"]["true_pre_refactor_baseline"] = True
    after["metadata"]["baseline_note"] = "Whole-refactor verdicts compare baseline.json; replay_threshold_verdicts and semantic_comparison retain the separate current-model pipeline replay."
    snapshot = json.loads((args.after.parent / "snapshot.json").read_text())
    after["metadata"]["source_snapshot_sha256"] = snapshot["source_snapshot_sha256"]
    baseline["metadata"] = {"environment": {**after["metadata"]["environment"], "git_head": baseline["revision"], "git_dirty": True}, "baseline_revision": baseline["revision"], "adapter_sha256": hashlib.sha256(Path("scripts/webpage_baseline.rs").read_bytes()).hexdigest(), "model_and_layout_changes": "none; isolated checkout adds only this measurement adapter and copied fixtures", "peak_rss_method": after["metadata"]["peak_rss_method"], "phases": after["metadata"]["phases"]}
    for row in baseline["samples"]:
        for source, target in (("allocations", "allocations_per_iteration"), ("allocated_bytes", "allocated_bytes_per_iteration")):
            row[target]["p95"] = sorted(sample[source] for sample in row["per_iteration_samples"])[18]
    args.baseline.write_text(json.dumps(baseline, indent=2) + "\n")
    args.after.write_text(json.dumps(after, indent=2) + "\n")
    print(json.dumps(verdicts, indent=2))
    raise SystemExit(any(row["status"] == "fail" for row in verdicts))
