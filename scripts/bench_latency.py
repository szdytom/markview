#!/usr/bin/env python3
"""Aggregate `markview --bench-latency` reports across independent processes.

`--bench-latency` is a one-process measurement: the headline first-frame number
is a cold-start sample that cannot be repeated inside a process, and the edit
distribution is only as wide as one process's iterations. This driver runs the
binary several times per fixture and reports the median (and worst) across
processes, which is what a comparison against a target should use.

Edit latency starts after the edited bytes are written, so the reader's 10 ms
file-watch debounce is not included; add it when comparing against an
end-to-end target.
"""
import argparse
import json
import statistics
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def parse_report(stdout):
    start = stdout.find("{")
    if start < 0:
        raise ValueError(f"no JSON report in output:\n{stdout[:2000]}")
    return json.loads(stdout[start:])


def run_once(binary, fixture, iterations, offline):
    command = [
        str(binary),
        "--bench-latency",
        str(fixture),
        "--iterations",
        str(iterations),
    ]
    if offline:
        command.append("--offline")
    output = subprocess.check_output(command, cwd=ROOT, text=True)
    return parse_report(output)


METRICS = {
    "first_frame_ms": ("cold", "process_start_to_first_readable_frame_ms"),
    "init_ms": ("initialization_ms",),
    "cold_complete_ms": ("cold", "process_start_to_complete_ms"),
    "cold_layout_ms": ("cold", "layout_ms"),
    "cold_parse_ms": ("cold", "parse_ms"),
    "edit_top_first_frame_p50": ("edits_top", "first_frame_ms", "p50_ms"),
    "edit_top_first_frame_p95": ("edits_top", "first_frame_ms", "p95_ms"),
    "edit_top_complete_p50": ("edits_top", "complete_ms", "p50_ms"),
    "edit_top_parse_p50": ("edits_top", "parse_ms", "p50_ms"),
    "edit_top_layout_p50": ("edits_top", "layout_ms", "p50_ms"),
    "edit_far_first_frame_p50": ("edits_far", "first_frame_ms", "p50_ms"),
    "edit_far_complete_p50": ("edits_far", "complete_ms", "p50_ms"),
    "rss_after_scroll": ("memory", "after_scroll", "linux_rss_bytes"),
    "peak_rss": ("memory", "after_edits", "linux_peak_rss_bytes"),
    "rss_slope_per_edit": ("memory", "rss_slope_bytes_per_iteration"),
    "gpu_bytes": ("memory", "tracked_gpu_bytes_excluding_driver"),
}
INVARIANTS = ("content_hash", "bytes", "blocks", "adapter", "physical_size")


def value(report, path):
    for key in path:
        report = report[key]
    return report


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", nargs="?", default="target/release/markview")
    parser.add_argument("fixtures", nargs="*", default=[
        "tests/fixtures/ordinary-10k.md",
        "tests/fixtures/text-cjk-100k.md",
    ])
    parser.add_argument("--runs", type=int, default=5,
                        help="independent processes per fixture")
    parser.add_argument("--iterations", type=int, default=60)
    parser.add_argument("--output", default="artifacts/perf-analysis/latency")
    parser.add_argument("--no-offline", action="store_true")
    args = parser.parse_args()

    binary = Path(args.binary)
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)

    summary = {}
    for fixture in args.fixtures:
        reports = [run_once(binary, fixture, args.iterations,
                            not args.no_offline)
                   for _ in range(args.runs)]
        reference = reports[0]
        for report in reports:
            for field in INVARIANTS:
                if report[field] != reference[field]:
                    raise SystemExit(
                        f"{fixture}: incompatible {field}: "
                        f"{report[field]} != {reference[field]}")
        (output / f"{Path(fixture).stem}-raw.json").write_text(
            json.dumps(reports, indent=2) + "\n")
        metrics = {}
        for name, path in METRICS.items():
            samples = [value(r, path) for r in reports]
            samples = [s for s in samples if s is not None]
            if not samples:
                metrics[name] = {"status": "unavailable"}
                continue
            metrics[name] = {
                "median": statistics.median(samples),
                "min": min(samples),
                "max": max(samples),
                "samples": samples,
            }
        summary[fixture] = {
            "bytes": reference["bytes"],
            "blocks": reference["blocks"],
            "metrics": metrics,
        }
        print(f"{Path(fixture).stem}: {reference['bytes']} B, "
              f"{reference['blocks']} blocks", flush=True)

    (output / "latency.json").write_text(json.dumps(summary, indent=2) + "\n")
    lines = ["# Latency benchmark", "",
             "Medians across independent processes; edit timing starts after "
             "the write returns (the 10 ms watch debounce is excluded).", "",
             "| Fixture | Bytes | Blocks | First frame | Init | Edit top P50 "
             "| Edit top P95 | Edit complete | Edit far P50 | RSS | Peak RSS "
             "| RSS slope/edit |", "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for fixture, data in summary.items():
        m = data["metrics"]

        def cell(name, scale=1.0, digits=1, suffix=""):
            entry = m.get(name)
            if not entry or entry.get("status") == "unavailable":
                return "—"
            return f"{entry['median'] / scale:.{digits}f}{suffix}"

        lines.append(
            f"| {Path(fixture).stem} | {data['bytes']} | {data['blocks']} "
            f"| {cell('first_frame_ms')} | {cell('init_ms')} "
            f"| {cell('edit_top_first_frame_p50')} "
            f"| {cell('edit_top_first_frame_p95')} "
            f"| {cell('edit_top_complete_p50')} "
            f"| {cell('edit_far_first_frame_p50')} "
            f"| {cell('rss_after_scroll', 1024 * 1024)} "
            f"| {cell('peak_rss', 1024 * 1024)} "
            f"| {cell('rss_slope_per_edit', 1024, 0, ' KiB')} |")
    (output / "latency.md").write_text("\n".join(lines) + "\n")
    print(f"Wrote {output / 'latency.md'}", flush=True)


if __name__ == "__main__":
    sys.exit(main())
