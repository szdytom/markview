#!/usr/bin/env python3
"""Compare preserved release binaries using alternating, independent GPU runs.

Run on an idle desktop with the same fonts/backend for both binaries. Reports
retain raw samples; a failed or unstable comparison needs investigation, not a
larger threshold. No personal settings are loaded by Markview's benchmark mode.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = [f"tests/fixtures/{name}-10k.md" for name in
            ("ordinary", "math", "code", "long-code")] + [
                "examples/images.md", "tests/fixtures/emoji-fallback.md"]
METRICS = {
    "first_open_ms": ("first_open", "total_ms"),
    "full_p50_ms": ("full_layout_reopens", "p50_ms"),
    "full_p95_ms": ("full_layout_reopens", "p95_ms"),
    "cached_p50_ms": ("cached_refreshes", "p50_ms"),
    "cached_p95_ms": ("cached_refreshes", "p95_ms"),
    "rss_bytes": ("memory_after_scroll", "linux_rss_bytes"),
    "peak_rss_bytes": ("memory_after_scroll", "linux_peak_rss_bytes"),
    "gpu_bytes": ("tracked_gpu_bytes_excluding_driver",),
}
INVARIANTS = ("adapter", "content_hash", "bytes", "physical_size", "scale",
              "column_width", "font_size", "reading_text_index_bytes",
              "degraded_paragraphs", "formula_errors")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def value(report, path):
    for key in path:
        report = report[key]
    return report


def summarize(reports, limit):
    summary = {}
    for fixture, sides in reports.items():
        reference = sides["baseline"][0]
        for side in sides.values():
            for report in side:
                for field in INVARIANTS:
                    if report[field] != reference[field]:
                        raise ValueError(f"{fixture}: incompatible {field}")
        metrics = {}
        for name, path in METRICS.items():
            samples = {side: [value(r, path) for r in runs]
                       for side, runs in sides.items()}
            if any(v is None for vs in samples.values() for v in vs):
                metrics[name] = {"status": "unavailable"}
                continue
            before = statistics.median(samples["baseline"])
            after = statistics.median(samples["candidate"])
            delta = (after / before - 1) * 100 if before else (0 if not after else float("inf"))
            metrics[name] = {"baseline": before, "candidate": after,
                             "change_percent": delta,
                             "status": "pass" if after <= before * (1 + limit / 100) else "regression",
                             "samples": samples}
        # Small stage timings are diagnostic: retain their medians without
        # interpreting timer resolution / scheduler noise as a user-visible cost.
        stages = {}
        for phase, array in (("full", "full_layout_samples"), ("cached", "cached_samples")):
            stages[phase] = {
                side: {key: statistics.median(statistics.median(t[key] for t in r[array]) for r in runs)
                       for key in runs[0][array][0]}
                for side, runs in sides.items()
            }
        summary[fixture] = {"metrics": metrics, "stages": stages,
                            "initialization_ms": {side: statistics.median(r["initialization_ms"] for r in runs)
                                                  for side, runs in sides.items()}}
    return summary



def write_comparison(reports, output, limit):
    summary = summarize(reports, limit)
    (output / "comparison.json").write_text(json.dumps(summary, indent=2) + "\n")
    lines = ["# Release performance comparison", "", "All supplied process groups are retained; each metric uses their median.", "",
             "| Fixture | Metric | Baseline | Candidate | Change | Result |", "|---|---|---:|---:|---:|---|"]
    failed = False
    for fixture, data in summary.items():
        for metric, values in data["metrics"].items():
            failed |= values["status"] != "pass"
            if values["status"] == "unavailable":
                lines.append(f"| {Path(fixture).stem} | {metric} | — | — | — | unavailable |")
            else:
                lines.append(f"| {Path(fixture).stem} | {metric} | {values['baseline']:.3f} | {values['candidate']:.3f} | {values['change_percent']:+.2f}% | {values['status']} |")
    (output / "comparison.md").write_text("\n".join(lines) + "\n")
    print(f"Comparison: {output / 'comparison.md'}", flush=True)
    return int(failed)


def merge_reports(directories, output, limit):
    reports = {}
    reference = None
    for directory in directories:
        metadata = json.loads((directory / "metadata.json").read_text())
        if reference is None:
            reference = metadata
        for key in ("binary_sha256", "platform", "rustc", "fonts_sha256",
                    "cpu_affinity", "backend_environment", "profile", "iterations"):
            if metadata.get(key) != reference.get(key):
                raise ValueError(f"{directory}: incompatible {key}; cannot merge runs")
        baseline_files = sorted(directory.glob("*-baseline.json"))
        if not baseline_files:
            raise ValueError(f"{directory}: no raw process reports")
        for baseline_file in baseline_files:
            baseline = json.loads(baseline_file.read_text())
            candidate = json.loads(baseline_file.with_name(
                baseline_file.name.removesuffix("-baseline.json") + "-candidate.json").read_text())
            fixture = Path(baseline["file"])
            try:
                fixture = fixture.relative_to(ROOT)
            except ValueError:
                pass
            sides = reports.setdefault(str(fixture), {"baseline": [], "candidate": []})
            sides["baseline"].append(baseline)
            sides["candidate"].append(candidate)
    output.mkdir(parents=True, exist_ok=False)
    reference = dict(reference, merged_from=[str(d.resolve()) for d in directories],
                     groups_per_fixture={f: len(r["baseline"]) for f, r in reports.items()},
                     limit_percent=limit)
    reference.pop("groups", None)
    (output / "metadata.json").write_text(json.dumps(reference, indent=2) + "\n")
    return write_comparison(reports, output, limit)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--candidate", type=Path, default=ROOT / "target/release/markview")
    parser.add_argument("--baseline-revision")
    parser.add_argument("--merge", type=Path, nargs="+", help="Combine all raw groups from compatible runs without rerunning binaries")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--groups", type=int, default=5)
    parser.add_argument("--iterations", type=int, default=100)
    parser.add_argument("--limit-percent", type=float, default=5.0)
    parser.add_argument("--fixture", action="append", dest="fixtures")
    args = parser.parse_args()
    if args.groups < 1 or not 1 <= args.iterations <= 10000 or args.limit_percent < 0:
        parser.error("groups must be positive, iterations 1..10000, limit nonnegative")
    if args.merge:
        if len(set(d.resolve() for d in args.merge)) != len(args.merge):
            parser.error("each merge input must be unique")
        return merge_reports(args.merge, args.output, args.limit_percent)
    if args.baseline is None or args.baseline_revision is None:
        parser.error("--baseline and --baseline-revision are required unless --merge is used")
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    for binary in binaries.values():
        if not binary.is_file():
            parser.error(f"Missing binary: {binary}")
    args.output.mkdir(parents=True, exist_ok=False)
    fonts = subprocess.run(["fc-list"], capture_output=True, text=True).stdout if sys.platform == "linux" else ""
    metadata = {"platform": platform.platform(),
                "cpu_affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
                "rustc": command("rustc", "--version"),
                "baseline_revision": args.baseline_revision,
                "candidate_revision": command("git", "rev-parse", "HEAD"),
                "candidate_changes": command("git", "status", "--short"),
                "profile": "release, thin LTO, one codegen unit (caller builds binaries)",
                "binary_sha256": {side: digest(p) for side, p in binaries.items()},
                "fonts_sha256": hashlib.sha256("\n".join(sorted(fonts.splitlines())).encode()).hexdigest(),
                "backend_environment": {key: os.environ.get(key) for key in
                                        ("WGPU_BACKEND", "WGPU_ADAPTER_NAME", "DISPLAY", "WAYLAND_DISPLAY")},
                "groups": args.groups, "iterations": args.iterations, "limit_percent": args.limit_percent}
    (args.output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    reports = {}
    for fixture in args.fixtures or FIXTURES:
        sides = reports[fixture] = {"baseline": [], "candidate": []}
        name = Path(fixture).stem
        for group in range(args.groups):
            order = ("baseline", "candidate") if group % 2 == 0 else ("candidate", "baseline")
            for side in order:
                output = args.output / f"{name}-{group + 1}-{side}.json"
                print(f"{name}: group {group + 1}/{args.groups} {side}", flush=True)
                subprocess.run([str(binaries[side]), "bench", str(ROOT / fixture), "--offline",
                                "--iterations", str(args.iterations), "--output", str(output.resolve())],
                               cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
                sides[side].append(json.loads(output.read_text()))
    return write_comparison(reports, args.output, args.limit_percent)


if __name__ == "__main__":
    raise SystemExit(main())
